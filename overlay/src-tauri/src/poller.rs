//! Background loop: find the client, follow the gameflow, poll champ select / live data, fetch the
//! aggregate build for the champion in play, run the engine, publish panel state, auto-import.
use crate::{AggState, App};
use featherstorm_core::aggregate::{self, Aggregate, Position};
use featherstorm_core::champselect::{self, Lobby};
use featherstorm_core::ddragon::{self, Catalog};
use featherstorm_core::engine::{self, Inputs, Plan};
use featherstorm_core::lcu::Lcu;
use featherstorm_core::live::{self, LiveClient, LiveSnapshot};
use featherstorm_core::pack::ChampionPack;
use featherstorm_core::state::{Flash, LiveView, LobbyView};
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tauri::AppHandle;

const AGGREGATE_RETRY_MS: u64 = 30_000;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn compute(
    st: &App,
    catalog: &Catalog,
    champion: &str,
    pack: Option<&ChampionPack>,
    aggregate: Option<&Aggregate>,
    enemies: &[String],
    live: Option<&LiveSnapshot>,
) -> Plan {
    engine::plan(&Inputs { champion, pack, aggregate, traits: &st.traits, catalog, enemies, live })
}

/// One line for the log: the path with tags, the NEXT item, spells, runes and the why lines.
fn plan_summary(plan: &Plan) -> String {
    let path: Vec<String> = plan
        .path
        .iter()
        .map(|p| match &p.tag {
            Some(t) => format!("{} ({t})", p.short),
            None => p.short.clone(),
        })
        .collect();
    let next = match &plan.next {
        Some(n) => match &n.buy_now {
            Some(c) if c.id != n.id => format!("{} (buy {})", n.name, c.name),
            _ => n.name.clone(),
        },
        None => "-".to_string(),
    };
    format!(
        "path {}; next {next}; spells {:?}; runes {}; why {:?}",
        path.join(" > "),
        plan.spells,
        plan.runes_summary,
        plan.why
    )
}

fn lobby_view(lobby: &Lobby, catalog: &Catalog) -> LobbyView {
    LobbyView {
        allies: lobby.allies.iter().map(|k| catalog.champion_name(*k)).collect(),
        enemies: lobby.enemies.iter().map(|k| catalog.champion_name(*k)).collect(),
        my_position: lobby.my_position.clone(),
    }
}

/// The aggregate build for this champion (and assigned position), fetched once and kept; a failed
/// fetch is retried every `AGGREGATE_RETRY_MS`. No lock is held across the await.
async fn aggregate_for(st: &App, champion_key: u32, requested: Option<Position>) -> Option<Arc<Aggregate>> {
    let key = (champion_key, requested);
    {
        let a = st.aggregate.lock().unwrap();
        if a.key == Some(key) && (a.value.is_some() || now_ms() < a.next_try_ms) {
            return a.value.clone();
        }
    }
    let (region, tier) = {
        let s = st.settings.lock().unwrap();
        (s.region.clone(), s.tier.clone())
    };
    let dir = crate::settings::data_dir().join("aggregate");
    match aggregate::load(&dir, &region, &tier, champion_key, requested).await {
        Ok(a) => {
            log::info!(
                "aggregate: champion {champion_key} as {} ({}, patch {}): spells {:?}, core {:?}, boots {:?}, skills {}",
                a.position.label(),
                a.describe(),
                a.patch,
                a.spells.ids,
                a.core.ids,
                a.boots.as_ref().map(|b| b.ids.clone()).unwrap_or_default(),
                a.skill_order.iter().collect::<String>()
            );
            let a = Arc::new(a);
            *st.aggregate.lock().unwrap() = AggState { key: Some(key), value: Some(a.clone()), error: None, next_try_ms: 0 };
            Some(a)
        }
        Err(e) => {
            log::warn!("aggregate: champion {champion_key}: {e}");
            *st.aggregate.lock().unwrap() = AggState {
                key: Some(key),
                value: None,
                error: Some(e.to_string()),
                next_try_ms: now_ms() + AGGREGATE_RETRY_MS,
            };
            None
        }
    }
}

async fn load_catalog(app: &AppHandle, st: &App) -> Arc<Catalog> {
    let cache = crate::settings::data_dir().join("ddragon");
    loop {
        match ddragon::load(&cache).await {
            Ok(c) => {
                let c = Arc::new(c);
                *st.catalog.lock().unwrap() = Some(c.clone());
                let version = c.version.clone();
                st.update(app, |p| p.ddragon = Some(version));
                return c;
            }
            Err(e) => {
                log::warn!("Data Dragon: {e}");
                st.update(app, |p| p.message = Some(format!("Data Dragon unavailable: {e}")));
                tokio::time::sleep(Duration::from_secs(15)).await;
            }
        }
    }
}

async fn connect(app: &AppHandle, st: &App) -> Option<Lcu> {
    if let Some(l) = st.lcu.lock().unwrap().clone() {
        return Some(l);
    }
    match Lcu::connect() {
        Ok(lcu) => {
            match lcu.current_summoner().await {
                Ok(me) => {
                    let name = format!(
                        "{}#{}",
                        me.get("gameName").and_then(|v| v.as_str()).unwrap_or("?"),
                        me.get("tagLine").and_then(|v| v.as_str()).unwrap_or("")
                    );
                    *st.summoner_id.lock().unwrap() = me.get("summonerId").and_then(|v| v.as_u64());
                    log::info!("connected to client on port {} as {name}", lcu.port());
                    st.update(app, |p| {
                        p.summoner = Some(name);
                        p.message = None;
                    });
                }
                Err(e) => {
                    log::warn!("client found but not answering yet: {e}");
                    return None;
                }
            }
            *st.lcu.lock().unwrap() = Some(lcu.clone());
            Some(lcu)
        }
        Err(_) => None,
    }
}

fn drop_client(app: &AppHandle, st: &App) {
    *st.lcu.lock().unwrap() = None;
    *st.lobby.lock().unwrap() = None;
    st.update(app, |p| {
        p.phase = "noclient".into();
        p.gameflow.clear();
        p.summoner = None;
        p.lobby = None;
        p.live = None;
        p.message = Some("Waiting for the League client...".into());
    });
}

fn no_data_message(champion: &str, error: Option<&str>) -> String {
    match error {
        Some(e) => format!("No build data for {champion} yet ({e})"),
        None => format!("No build data for {champion} yet"),
    }
}

/// Run one auto-import and log its outcome (the panel's button state is set by the import itself).
async fn auto(which: &str, f: impl Future<Output = Result<String, String>>) {
    match f.await {
        Ok(m) => log::info!("auto-import {which}: {m}"),
        Err(e) => log::warn!("auto-import {which}: {e}"),
    }
}

pub async fn run(app: AppHandle, st: Arc<App>) {
    let catalog = load_catalog(&app, &st).await;
    let live_client = LiveClient::new().expect("http client");
    let mut tick: u64 = 0;
    let mut last_level: Option<u32> = None;
    let mut last_live_ok = false;
    let mut last_phase = String::new();
    let mut last_summary = String::new();
    // Auto-import bookkeeping for the current champ select: the champion the runes/spells were set
    // for, and the (champion, path) the item set was pushed for.
    let mut auto_done: Option<u32> = None;
    let mut itemset_done: Option<(u32, String)> = None;

    loop {
        tick += 1;
        tokio::time::sleep(Duration::from_secs(1)).await;

        let Some(lcu) = connect(&app, &st).await else {
            drop_client(&app, &st);
            last_phase.clear();
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        };

        let phase = match lcu.gameflow_phase().await {
            Ok(p) => p,
            Err(e) => {
                log::info!("client went away: {e}");
                drop_client(&app, &st);
                last_phase.clear();
                continue;
            }
        };
        if phase != last_phase {
            log::info!("gameflow {} -> {phase}", if last_phase.is_empty() { "-" } else { last_phase.as_str() });
            last_phase = phase.clone();
        }

        // Expire the level-up flash.
        st.update(&app, |p| {
            if p.flash.as_ref().map(|f| f.until_ms < now_ms()).unwrap_or(false) {
                p.flash = None;
            }
            p.gameflow = phase.clone();
        });

        match phase.as_str() {
            "ChampSelect" => {
                last_level = None;
                let session = match lcu.champ_select_session().await {
                    Ok(Some(s)) => s,
                    Ok(None) => continue,
                    Err(e) => {
                        log::warn!("champ select: {e}");
                        continue;
                    }
                };
                let lobby = champselect::extract(&session);
                let enemies: Vec<String> = lobby.enemies.iter().map(|k| catalog.champion_name(*k)).collect();
                let champion = (lobby.my_champion > 0).then(|| catalog.champion_name(lobby.my_champion));
                let requested = Position::parse(&lobby.my_position);
                let agg = match lobby.my_champion {
                    0 => None,
                    key => aggregate_for(&st, key, requested).await,
                };
                let pack = champion.as_deref().and_then(|c| st.pack_for(c));
                let plan = compute(&st, &catalog, champion.as_deref().unwrap_or(""), pack, agg.as_deref(), &enemies, None);
                let supported = agg.is_some() || pack.is_some();
                let agg_error = st.aggregate.lock().unwrap().error.clone();
                let position_label = plan
                    .position
                    .clone()
                    .unwrap_or_else(|| if lobby.my_position.is_empty() { "no position".to_string() } else { lobby.my_position.clone() });
                let summary = format!(
                    "champ select: {} ({position_label}) vs {:?}; {}",
                    champion.as_deref().unwrap_or("(no pick yet)"),
                    enemies,
                    plan_summary(&plan)
                );
                if summary != last_summary {
                    log::info!("{summary}");
                    last_summary = summary;
                }
                let view = lobby_view(&lobby, &catalog);
                let (my_champion, my_locked) = (lobby.my_champion, lobby.my_locked);
                *st.lobby.lock().unwrap() = Some(lobby);
                *st.plan.lock().unwrap() = Some(plan.clone());
                st.update(&app, |p| {
                    p.phase = "champselect".into();
                    p.champion = champion.clone();
                    p.supported = supported;
                    p.lobby = Some(view.clone());
                    p.plan = Some(plan.clone());
                    p.live = None;
                    p.message = match &champion {
                        None => Some("Pick a champion".into()),
                        Some(c) if !supported => Some(no_data_message(c, agg_error.as_deref())),
                        _ => None,
                    };
                });

                // Auto-import: runes + spells as soon as the champion is known; the item set once it is
                // locked, and again if enemy locks change the path.
                if supported && my_champion > 0 {
                    let (auto_runes, auto_spells, auto_itemset) = {
                        let s = st.settings.lock().unwrap();
                        (s.auto_runes, s.auto_spells, s.auto_itemset)
                    };
                    if auto_done != Some(my_champion) {
                        auto_done = Some(my_champion);
                        if auto_runes {
                            auto("runes", crate::commands::do_import_runes(&app, &st)).await;
                        }
                        if auto_spells {
                            auto("spells", crate::commands::do_import_spells(&app, &st)).await;
                        }
                    }
                    if auto_itemset && my_locked {
                        let sig = plan.path.iter().map(|p| p.id.to_string()).collect::<Vec<_>>().join(",");
                        if itemset_done.as_ref() != Some(&(my_champion, sig.clone())) {
                            itemset_done = Some((my_champion, sig));
                            auto("item set", crate::commands::do_import_item_set(&app, &st)).await;
                        }
                    }
                }
            }
            "GameStart" => {
                st.update(&app, |p| {
                    p.phase = "loading".into();
                    p.message = Some("Loading...".into());
                });
            }
            "InProgress" | "Reconnect" => {
                if tick % 2 != 0 {
                    continue; // live data every 2 s
                }
                match live_client.all_game_data().await {
                    Some(data) => {
                        let snap = live::summarize(&data);
                        let (champion, enemies) = match &snap.me {
                            Some(me) => {
                                let enemies: Vec<String> = if snap.enemies.is_empty() {
                                    st.lobby
                                        .lock()
                                        .unwrap()
                                        .as_ref()
                                        .map(|l| l.enemies.iter().map(|k| catalog.champion_name(*k)).collect())
                                        .unwrap_or_default()
                                } else {
                                    snap.enemies.iter().map(|p| p.champion.clone()).collect()
                                };
                                (Some(me.player.champion.clone()), enemies)
                            }
                            None => (None, Vec::new()),
                        };
                        let champion_key = champion.as_deref().and_then(|c| catalog.champion_key(c));
                        let requested = st.lobby.lock().unwrap().as_ref().and_then(|l| Position::parse(&l.my_position));
                        let agg = match champion_key {
                            Some(key) => aggregate_for(&st, key, requested).await,
                            None => None,
                        };
                        let pack = champion.as_deref().and_then(|c| st.pack_for(c));
                        let plan = compute(&st, &catalog, champion.as_deref().unwrap_or(""), pack, agg.as_deref(), &enemies, Some(&snap));
                        let supported = agg.is_some() || pack.is_some();
                        let agg_error = st.aggregate.lock().unwrap().error.clone();
                        let level = snap.me.as_ref().map(|m| m.player.level).unwrap_or(0);
                        let summary = format!(
                            "live: {} lvl {level} vs {:?}; {}",
                            champion.as_deref().unwrap_or("?"),
                            enemies,
                            plan_summary(&plan)
                        );
                        if summary != last_summary {
                            log::info!("{summary}");
                            last_summary = summary;
                        }
                        let flash = match last_level {
                            Some(prev) if level > prev && plan.skill.next.is_some() => {
                                let skill = plan.skill.next.unwrap();
                                log::info!("level {prev} -> {level}: recommend {skill}");
                                Some(Flash { skill, until_ms: now_ms() + 3500 })
                            }
                            _ => None,
                        };
                        last_level = Some(level);
                        last_live_ok = true;
                        let live_view = snap.me.as_ref().map(|m| LiveView {
                            game_time: snap.game_time,
                            gold: m.gold,
                            level: m.player.level,
                            kda: m.player.kda(),
                        });
                        let allies: Vec<String> = snap.allies.iter().map(|p| p.champion.clone()).collect();
                        *st.plan.lock().unwrap() = Some(plan.clone());
                        st.update(&app, |p| {
                            p.phase = "ingame".into();
                            p.champion = champion.clone();
                            p.supported = supported;
                            p.plan = Some(plan.clone());
                            p.live = live_view.clone();
                            if let Some(f) = &flash {
                                p.flash = Some(f.clone());
                            }
                            if p.lobby.is_none() || p.lobby.as_ref().map(|l| l.enemies.is_empty()).unwrap_or(false) {
                                p.lobby = Some(LobbyView { allies, enemies: enemies.clone(), my_position: String::new() });
                            }
                            p.message = match &champion {
                                Some(c) if !supported => Some(no_data_message(c, agg_error.as_deref())),
                                _ => None,
                            };
                        });
                    }
                    None => {
                        if !last_live_ok {
                            st.update(&app, |p| {
                                p.phase = "loading".into();
                                p.message = Some("Waiting for game data...".into());
                            });
                        }
                    }
                }
            }
            _ => {
                // None | Lobby | Matchmaking | ReadyCheck | WaitingForStats | PreEndOfGame | EndOfGame
                last_level = None;
                last_live_ok = false;
                if matches!(phase.as_str(), "None" | "Lobby" | "Matchmaking" | "ReadyCheck") {
                    *st.lobby.lock().unwrap() = None;
                    *st.plan.lock().unwrap() = None;
                    auto_done = None;
                    itemset_done = None;
                }
                st.update(&app, |p| {
                    p.phase = "idle".into();
                    p.live = None;
                    p.flash = None;
                    if matches!(phase.as_str(), "None" | "Lobby" | "Matchmaking" | "ReadyCheck") {
                        p.lobby = None;
                        p.plan = None;
                        p.champion = None;
                        p.supported = false;
                        p.imports = Default::default();
                    }
                    p.message = Some(match phase.as_str() {
                        "Lobby" => "In lobby. Runes and spells are set when you pick.".to_string(),
                        "Matchmaking" => "In queue...".to_string(),
                        "ReadyCheck" => "Match found!".to_string(),
                        "EndOfGame" | "PreEndOfGame" | "WaitingForStats" => "Game over. GG.".to_string(),
                        _ => "Ready. Start a game.".to_string(),
                    });
                });
            }
        }
    }
}
