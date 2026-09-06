//! `recall.exe --demo <champselect|ingame|idle>`: show the panel with a realistic sample state
//! and no client, for design work and screenshots. Real Data Dragon, real aggregate, the real engine;
//! only the lobby and the live numbers are staged (the Swiftplay game of 2026-09-05).
use crate::App;
use recall_core::aggregate::{self, Position};
use recall_core::ddragon;
use recall_core::engine::{self, Inputs};
use recall_core::live;
use recall_core::state::{Flash, Imports, LiveView, LobbyView};
use std::sync::Arc;
use tauri::AppHandle;

const LIVE_FIXTURE: &str = include_str!("../../../m0/tests/fixtures/allgamedata.json");
const ENEMIES: [&str; 5] = ["Yone", "Vi", "Katarina", "Vayne", "Lux"];
const ALLIES: [&str; 5] = ["Gwen", "Master Yi", "Aurora", "Xayah", "Shen"];

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub async fn run(app: AppHandle, st: Arc<App>, phase: String) {
    let cache = crate::settings::data_dir();
    let catalog = match ddragon::load(&cache.join("ddragon")).await {
        Ok(c) => Arc::new(c),
        Err(e) => {
            log::warn!("demo: Data Dragon: {e}");
            return;
        }
    };
    *st.catalog.lock().unwrap() = Some(catalog.clone());
    let version = catalog.version.clone();
    let agg = aggregate::load(
        &cache.join("aggregate"),
        aggregate::DEFAULT_REGION,
        aggregate::DEFAULT_TIER,
        498,
        Some(Position::Adc),
    )
    .await
    .map_err(|e| log::warn!("demo: aggregate: {e}"))
    .ok();
    let enemies: Vec<String> = ENEMIES.iter().map(|s| s.to_string()).collect();
    let allies: Vec<String> = ALLIES.iter().map(|s| s.to_string()).collect();
    let pack = st.pack_for("Xayah");
    let lobby = LobbyView {
        allies,
        enemies: enemies.clone(),
        my_position: "bottom".into(),
    };

    st.update(&app, |p| {
        p.demo = true;
        p.summoner = Some("Demo player".into());
        p.ddragon = Some(version.clone());
        p.message = Some("Demo — staged lobby and inventory; no client imports".into());
    });
    match phase.as_str() {
        p if p.starts_with("ingame") => {
            // Level 9 at 12:23 with Yun Tal, boots and a B. F. Sword towards IE; 1,204 gold; 4/1/2.
            let mut data: serde_json::Value =
                serde_json::from_str(LIVE_FIXTURE).expect("live fixture");
            if let Some(me) = data["allPlayers"]
                .as_array_mut()
                .and_then(|a| a.iter_mut().find(|p| p["riotId"] == "matteso#NA1"))
            {
                me["items"] = serde_json::json!([
                    {"itemID": 3032, "displayName": "Yun Tal Wildarrows", "count": 1, "slot": 0},
                    {"itemID": 3006, "displayName": "Berserker's Greaves", "count": 1, "slot": 1},
                    {"itemID": 1038, "displayName": "B. F. Sword", "count": 1, "slot": 2},
                    {"itemID": 3340, "displayName": "Stealth Ward", "count": 1, "slot": 6}
                ]);
                me["level"] = serde_json::json!(9);
                me["scores"]["kills"] = serde_json::json!(4);
                me["scores"]["deaths"] = serde_json::json!(1);
                me["scores"]["assists"] = serde_json::json!(2);
            }
            data["activePlayer"]["currentGold"] = serde_json::json!(1204.0);
            data["activePlayer"]["level"] = serde_json::json!(9);
            data["gameData"]["gameTime"] = serde_json::json!(743.0);
            for (k, lvl) in [("Q", 1), ("W", 2), ("E", 5), ("R", 1)] {
                data["activePlayer"]["abilities"][k]["abilityLevel"] = serde_json::json!(lvl);
            }
            let snap = live::summarize(&data);
            let plan = engine::plan(&Inputs {
                champion: "Xayah",
                pack,
                aggregate: agg.as_ref(),
                traits: &st.traits,
                catalog: &catalog,
                enemies: &enemies,
                live: Some(&snap),
            });
            let flash = phase.ends_with("flash").then(|| Flash {
                skill: 'E',
                until_ms: now_ms() + 3500,
            });
            *st.plan.lock().unwrap() = Some(plan.clone());
            st.update(&app, |p| {
                p.phase = "ingame".into();
                p.gameflow = "InProgress".into();
                p.champion = Some("Xayah".into());
                p.supported = !plan.path.is_empty();
                p.lobby = Some(lobby.clone());
                p.plan = Some(plan.clone());
                p.live = Some(LiveView {
                    game_time: 743.0,
                    gold: 1204.0,
                    level: 9,
                    kda: "4/1/2".into(),
                });
                p.flash = flash.clone();
            });
        }
        "champselect" => {
            let plan = engine::plan(&Inputs {
                champion: "Xayah",
                pack,
                aggregate: agg.as_ref(),
                traits: &st.traits,
                catalog: &catalog,
                enemies: &enemies,
                live: None,
            });
            *st.plan.lock().unwrap() = Some(plan.clone());
            st.update(&app, |p| {
                p.phase = "champselect".into();
                p.gameflow = "ChampSelect".into();
                p.champion = Some("Xayah".into());
                p.supported = !plan.path.is_empty();
                p.lobby = Some(lobby.clone());
                p.plan = Some(plan.clone());
                p.imports = Imports {
                    itemset: "idle".into(),
                    runes: "done".into(),
                    spells: "done".into(),
                };
            });
        }
        _ => {
            st.update(&app, |p| {
                p.phase = "idle".into();
                p.gameflow = "None".into();
                p.message = Some("Ready. Start a game.".into());
            });
        }
    }
    log::info!("demo: showing {phase}");
}
