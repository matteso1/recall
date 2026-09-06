//! `featherstorm.exe --probe`: no window at all. Loads Data Dragon, talks to the client if it is
//! running, runs the engine on the live lobby (or a sample one), prints a JSON report to stdout
//! and to %LOCALAPPDATA%\Featherstorm\probe.json, then exits. For checking the plumbing from
//! WSL while the screen is busy.
use featherstorm_core::aggregate::{self, Position};
use featherstorm_core::champselect;
use featherstorm_core::ddragon;
use featherstorm_core::engine::{self, Inputs};
use featherstorm_core::lcu::Lcu;
use featherstorm_core::live::{self, LiveClient};
use featherstorm_core::pack;
use serde_json::{json, Value};

fn print_and_save(report: &Value) {
    let text = serde_json::to_string_pretty(report).unwrap_or_default();
    println!("{text}");
    let _ = std::fs::write(crate::settings::data_dir().join("probe.json"), &text);
}

pub fn run() -> i32 {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(async {
        let mut report = json!({ "version": env!("CARGO_PKG_VERSION") });
        let cache = crate::settings::data_dir().join("ddragon");
        let catalog = match ddragon::load(&cache).await {
            Ok(c) => c,
            Err(e) => {
                report["ddragon_error"] = json!(e.to_string());
                print_and_save(&report);
                return 2;
            }
        };
        report["ddragon"] = json!({
            "version": catalog.version,
            "items": catalog.items.len(),
            "champions": catalog.champions.len(),
            "runes": catalog.runes.len(),
        });
        let pack = pack::load_xayah().expect("data pack");
        let traits = pack::load_traits().expect("champion traits");

        match Lcu::connect() {
            Ok(lcu) => {
                let mut client = json!({ "port": lcu.port() });
                match lcu.gameflow_phase().await {
                    Ok(p) => client["gameflow"] = json!(p),
                    Err(e) => client["gameflow_error"] = json!(e.to_string()),
                }
                match lcu.current_summoner().await {
                    Ok(me) => {
                        client["summoner"] = json!(format!(
                            "{}#{}",
                            me.get("gameName").and_then(Value::as_str).unwrap_or("?"),
                            me.get("tagLine").and_then(Value::as_str).unwrap_or("")
                        ))
                    }
                    Err(e) => client["summoner_error"] = json!(e.to_string()),
                }
                if let Ok(Some(session)) = lcu.champ_select_session().await {
                    client["champselect"] = json!(champselect::extract(&session));
                }
                report["client"] = client;
            }
            Err(e) => report["client"] = json!({ "error": e.to_string() }),
        }

        let snap = match LiveClient::new() {
            Ok(l) => l.all_game_data().await.map(|d| live::summarize(&d)),
            Err(_) => None,
        };
        report["live"] = match &snap {
            Some(s) => json!({
                "mode": s.mode,
                "time": s.game_time,
                "me": s.me.as_ref().map(|m| m.player.champion.clone()),
                "enemies": s.enemies.iter().map(|p| p.champion.clone()).collect::<Vec<_>>(),
            }),
            None => json!(null),
        };

        // The aggregate build for the sample champion (what every champion gets at champ select).
        let agg = aggregate::load(
            &crate::settings::data_dir().join("aggregate"),
            aggregate::DEFAULT_REGION,
            aggregate::DEFAULT_TIER,
            catalog.champion_key(&pack.champion).unwrap_or(498),
            Some(Position::Adc),
        )
        .await;
        report["aggregate"] = match &agg {
            Ok(a) => json!({
                "source": a.describe(),
                "patch": a.patch,
                "position": a.position.label(),
                "games": a.games,
                "spells": a.spells.ids.iter().map(|&id| featherstorm_core::runes::spell_name(id).unwrap_or("?")).collect::<Vec<_>>(),
                "runes": a.runes.as_ref().map(|r| {
                    r.perks
                        .iter()
                        .map(|&id| featherstorm_core::runes::shard_name(id).map(str::to_string).unwrap_or_else(|| catalog.rune_name(id)))
                        .collect::<Vec<_>>()
                }),
                "skills": a.skill_order.iter().collect::<String>(),
                "starters": a.starters.ids.iter().map(|&id| catalog.item_name(id)).collect::<Vec<_>>(),
                "core": a.core.ids.iter().map(|&id| catalog.item_name(id)).collect::<Vec<_>>(),
                "boots": a.boots.as_ref().and_then(|b| b.ids.first()).map(|&id| catalog.item_name(id)),
            }),
            Err(e) => json!({ "error": e.to_string() }),
        };

        let live_enemies: Vec<String> = snap
            .as_ref()
            .map(|s| s.enemies.iter().map(|p| p.champion.clone()).collect())
            .unwrap_or_default();
        let enemies: Vec<String> = if live_enemies.is_empty() {
            ["Tristana", "Soraka", "Malphite", "Ornn", "Thresh"].iter().map(|s| s.to_string()).collect()
        } else {
            live_enemies
        };
        let plan = engine::plan(&Inputs {
            champion: &pack.champion,
            pack: Some(&pack),
            aggregate: agg.as_ref().ok(),
            traits: &traits,
            catalog: &catalog,
            enemies: &enemies,
            live: snap.as_ref(),
        });
        report["engine"] = json!({
            "enemies": enemies,
            "source": plan.source,
            "start": plan.start.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
            "spells": plan.spells,
            "runes": plan.runes_summary,
            "skill_label": plan.skill.label,
            "path": plan.path.iter().map(|p| match &p.tag {
                Some(t) => format!("{} ({})", p.short, t),
                None => p.short.clone(),
            }).collect::<Vec<_>>(),
            "next": plan.next.as_ref().map(|n| json!({ "item": n.name, "buy_now": n.buy_now.as_ref().map(|c| c.name.clone()) })),
            "skill_next": plan.skill.next,
            "why": plan.why,
            "matchup": plan.matchup,
        });
        print_and_save(&report);
        0
    })
}
