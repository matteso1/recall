//! Background loop: find the client, follow the gameflow, poll champ select / live data,
//! run the engine, publish panel state.
use crate::App;
use featherstorm_core::champselect::{self, Lobby};
use featherstorm_core::ddragon::{self, Catalog};
use featherstorm_core::engine::{self, Inputs, Plan};
use featherstorm_core::lcu::Lcu;
use featherstorm_core::live::{self, LiveClient, LiveSnapshot};
use featherstorm_core::state::{Flash, LiveView, LobbyView};
use std::sync::Arc;
use std::time::Duration;
use tauri::AppHandle;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn compute(st: &App, catalog: &Catalog, enemies: &[String], live: Option<&LiveSnapshot>) -> Plan {
    engine::plan(&Inputs { pack: &st.pack, traits: &st.traits, catalog, enemies, live })
}

/// One line for the log: the path with tags, the NEXT item and the why lines.
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
    format!("path {}; next {next}; why {:?}", path.join(" > "), plan.why)
}

fn lobby_view(lobby: &Lobby, catalog: &Catalog) -> LobbyView {
    LobbyView {
        allies: lobby.allies.iter().map(|k| catalog.champion_name(*k)).collect(),
        enemies: lobby.enemies.iter().map(|k| catalog.champion_name(*k)).collect(),
        my_position: lobby.my_position.clone(),
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

pub async fn run(app: AppHandle, st: Arc<App>) {
    let catalog = load_catalog(&app, &st).await;
    let live_client = LiveClient::new().expect("http client");
    let mut tick: u64 = 0;
    let mut last_level: Option<u32> = None;
    let mut last_live_ok = false;
    let mut last_phase = String::new();
    let mut last_summary = String::new();

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
                let supported = champion
                    .as_deref()
                    .map(|c| ddragon::normalize(c) == ddragon::normalize(&st.pack.champion))
                    .unwrap_or(false);
                let plan = compute(&st, &catalog, &enemies, None);
                let summary = format!(
                    "champ select: {} vs {:?}; {}",
                    champion.as_deref().unwrap_or("(no pick yet)"),
                    enemies,
                    plan_summary(&plan)
                );
                if summary != last_summary {
                    log::info!("{summary}");
                    last_summary = summary;
                }
                let view = lobby_view(&lobby, &catalog);
                *st.lobby.lock().unwrap() = Some(lobby);
                *st.plan.lock().unwrap() = Some(plan.clone());
                st.update(&app, |p| {
                    p.phase = "champselect".into();
                    p.champion = champion.clone();
                    p.supported = supported;
                    p.lobby = Some(view.clone());
                    p.plan = Some(plan.clone());
                    p.live = None;
                    p.message = if champion.is_none() {
                        Some("Pick a champion".into())
                    } else if !supported {
                        Some(format!("Featherstorm only knows {} so far (M1)", st.pack.champion))
                    } else {
                        None
                    };
                });
            }
            "GameStart" => {
                st.update(&app, |p| {
                    p.phase = "loading".into();
                    p.message = Some("Loading... item set can be imported now".into());
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
                        let supported = champion
                            .as_deref()
                            .map(|c| ddragon::normalize(c) == ddragon::normalize(&st.pack.champion))
                            .unwrap_or(false);
                        let plan = compute(&st, &catalog, &enemies, Some(&snap));
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
                            p.message = if !supported && champion.is_some() {
                                Some(format!("Featherstorm only knows {} so far (M1)", st.pack.champion))
                            } else {
                                None
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
                        "Lobby" => "In lobby. Pick Xayah in champ select.".to_string(),
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
