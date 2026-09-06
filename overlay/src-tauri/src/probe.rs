//! `featherstorm.exe --probe`: no window at all. Loads Data Dragon, talks to the client if it is
//! running, runs the engine on the live lobby (or a sample one), prints a JSON report to stdout
//! and to %LOCALAPPDATA%\Featherstorm\probe.json, then exits. For checking the plumbing from
//! WSL while the screen is busy.
use featherstorm_core::aggregate::{self, Position};
use featherstorm_core::champselect::{self, Lobby};
use featherstorm_core::ddragon;
use featherstorm_core::engine::{self, Inputs};
use featherstorm_core::lcu::Lcu;
use featherstorm_core::live::{self, LiveClient, LiveSnapshot};
use featherstorm_core::pack;
use serde_json::{json, Value};

/// One coherent source for diagnostic planning. Sample inputs never mix with actual live data.
struct ProbeTarget {
    champion: String,
    champion_key: Option<u32>,
    position: Option<Position>,
    enemies: Vec<String>,
    source: &'static str,
    use_live: bool,
    note: Option<String>,
}

impl ProbeTarget {
    fn factual_pack<'a>(&self, pack: &'a pack::ChampionPack) -> Option<&'a pack::ChampionPack> {
        (ddragon::normalize(&self.champion) == ddragon::normalize(&pack.champion)).then_some(pack)
    }
}

fn select_target(
    catalog: &ddragon::Catalog,
    phase: Option<&str>,
    lobby: Option<&Lobby>,
    live: Option<&LiveSnapshot>,
) -> ProbeTarget {
    let lobby = lobby.filter(|lobby| lobby.my_cell >= 0 && lobby.my_champion > 0);
    // A previous game's Live Client endpoint can linger into a new champion select.
    if phase != Some("ChampSelect") {
        if let Some((snapshot, me)) = live
            .and_then(|snapshot| snapshot.me.as_ref().map(|me| (snapshot, me)))
            .filter(|(_, me)| !me.player.champion.trim().is_empty())
        {
            let champion_key = catalog.champion_key(&me.player.champion);
            let matching_lobby = lobby.filter(|lobby| champion_key == Some(lobby.my_champion));
            return ProbeTarget {
                champion: me.player.champion.clone(), champion_key,
                position: Position::parse(&me.player.position)
                    .or_else(|| matching_lobby.and_then(|lobby| Position::parse(&lobby.my_position))),
                enemies: snapshot.enemies.iter().map(|player| player.champion.clone()).collect(),
                source: "live", use_live: true,
                note: champion_key.is_none().then(|| "Observed champion is absent from this catalog; no other champion's build will be substituted.".into()),
            };
        }
    }
    if let Some(lobby) = lobby {
        return ProbeTarget {
            champion: catalog.champion_name(lobby.my_champion),
            champion_key: Some(lobby.my_champion),
            position: Position::parse(&lobby.my_position),
            enemies: lobby
                .enemies
                .iter()
                .map(|id| catalog.champion_name(*id))
                .collect(),
            source: "champselect",
            use_live: false,
            note: None,
        };
    }
    ProbeTarget {
        champion: "Xayah".into(), champion_key: catalog.champion_key("Xayah"), position: Some(Position::Adc),
        enemies: ["Tristana", "Soraka", "Malphite", "Ornn", "Thresh"].iter().map(|name| name.to_string()).collect(),
        source: "offline_sample", use_live: false,
        note: Some("Offline sample only: no current live or champion-select player is identified. Xayah and the enemy roster below are examples.".into()),
    }
}

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
        let mut phase = None;
        let mut observed_lobby = None;

        match Lcu::connect() {
            Ok(lcu) => {
                let mut client = json!({ "port": lcu.port() });
                match lcu.gameflow_phase().await {
                    Ok(p) => {
                        client["gameflow"] = json!(p);
                        phase = Some(p);
                    }
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
                    let mut lobby = champselect::extract(&session);
                    let own_rows = session.get("myTeam").and_then(Value::as_array).map(|team| team.iter()
                        .filter(|player| player.get("cellId").and_then(Value::as_i64) == Some(lobby.my_cell)).count()).unwrap_or(0);
                    if lobby.my_cell < 0 || own_rows != 1 { lobby.my_champion = 0; lobby.my_locked = false; }
                    client["champselect"] = json!(lobby);
                    observed_lobby = Some(lobby);
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

        let target = select_target(&catalog, phase.as_deref(), observed_lobby.as_ref(), snap.as_ref());
        report["target"] = json!({
            "source": target.source,
            "champion": target.champion,
            "champion_key": target.champion_key,
            "requested_position": target.position.map(|position| position.label()),
            "offline_sample": target.source == "offline_sample",
            "note": target.note,
        });
        let agg = match target.champion_key {
            Some(key) => aggregate::load(
                &crate::settings::data_dir().join("aggregate"),
                aggregate::DEFAULT_REGION,
                aggregate::DEFAULT_TIER,
                key,
                target.position,
            ).await,
            None => Err(anyhow::anyhow!("No catalog champion ID for {}; no substitute aggregate was fetched", target.champion)),
        };
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

        let plan = engine::plan(&Inputs {
            champion: &target.champion,
            pack: target.factual_pack(&pack),
            aggregate: agg.as_ref().ok(),
            traits: &traits,
            catalog: &catalog,
            enemies: &target.enemies,
            live: if target.use_live { snap.as_ref() } else { None },
        });
        report["engine"] = json!({
            "champion": plan.champion,
            "position": plan.position,
            "enemies": target.enemies,
            "supported": !plan.path.is_empty(),
            "note": plan.note,
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

#[cfg(test)]
mod tests {
    use super::*;
    use featherstorm_core::champselect::Lobby;
    use featherstorm_core::live::{LiveSnapshot, Me, Player};

    fn catalog() -> ddragon::Catalog {
        ddragon::Catalog::from_json(
            "16.17.1",
            &json!({"data": {}}),
            &json!({"data": {
                "Xayah": {"key": "498", "id": "Xayah", "name": "Xayah"},
                "Ahri": {"key": "103", "id": "Ahri", "name": "Ahri"},
                "Lulu": {"key": "117", "id": "Lulu", "name": "Lulu"},
                "Zed": {"key": "238", "id": "Zed", "name": "Zed"}
            }}),
            &json!([]),
        )
    }

    fn live(champion: &str, position: &str) -> LiveSnapshot {
        LiveSnapshot {
            game_time: 420.0,
            mode: "CLASSIC".into(),
            me: Some(Me {
                player: Player {
                    champion: champion.into(),
                    position: position.into(),
                    level: 9,
                    ..Default::default()
                },
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn probe_uses_actual_live_champion_role_and_roster_without_a_xayah_pack() {
        let cat = catalog();
        let mut live = live("Ahri", "MIDDLE");
        live.enemies.push(Player {
            champion: "Zed".into(),
            ..Default::default()
        });
        let target = select_target(&cat, None, None, Some(&live));
        assert_eq!(target.champion, "Ahri");
        assert_eq!(target.champion_key, Some(103));
        assert_eq!(target.position, Some(Position::Mid));
        assert_eq!(target.enemies, ["Zed"]);
        assert_eq!(target.source, "live");
        assert!(target.use_live);
        assert!(target.factual_pack(&pack::load_xayah().unwrap()).is_none());
    }

    #[test]
    fn real_empty_lobby_roster_is_not_filled_with_sample_enemies() {
        let cat = catalog();
        let lobby = Lobby {
            my_cell: 2,
            my_champion: 117,
            my_position: "utility".into(),
            ..Default::default()
        };
        let target = select_target(&cat, Some("ChampSelect"), Some(&lobby), None);
        assert_eq!(target.champion, "Lulu");
        assert_eq!(target.champion_key, Some(117));
        assert_eq!(target.position, Some(Position::Support));
        assert!(target.enemies.is_empty());
        assert_eq!(target.source, "champselect");
        assert!(!target.use_live);
    }

    #[test]
    fn another_champions_lobby_cannot_supply_live_role_or_enemies() {
        let cat = catalog();
        let lobby = Lobby {
            my_cell: 1,
            my_champion: 498,
            my_position: "bottom".into(),
            enemies: vec![238],
            ..Default::default()
        };
        let live = live("Ahri", "");
        let target = select_target(&cat, Some("InProgress"), Some(&lobby), Some(&live));
        assert_eq!(target.champion, "Ahri");
        assert_eq!(target.position, None);
        assert!(target.enemies.is_empty());
    }

    #[test]
    fn live_role_can_fall_back_only_to_the_same_identified_lobby_champion() {
        let cat = catalog();
        let lobby = Lobby {
            my_cell: 1,
            my_champion: 117,
            my_position: "utility".into(),
            ..Default::default()
        };
        let target = select_target(
            &cat,
            Some("InProgress"),
            Some(&lobby),
            Some(&live("Lulu", "")),
        );
        assert_eq!(target.position, Some(Position::Support));
        assert_eq!(target.source, "live");
    }

    #[test]
    fn current_champion_select_takes_precedence_over_a_leftover_live_response() {
        let cat = catalog();
        let lobby = Lobby {
            my_cell: 2,
            my_champion: 117,
            my_position: "utility".into(),
            ..Default::default()
        };
        let target = select_target(
            &cat,
            Some("ChampSelect"),
            Some(&lobby),
            Some(&live("Ahri", "MIDDLE")),
        );
        assert_eq!(target.champion, "Lulu");
        assert_eq!(target.position, Some(Position::Support));
        assert!(!target.use_live);
    }

    #[test]
    fn unknown_actual_champion_never_falls_back_to_xayahs_aggregate() {
        let cat = catalog();
        let target = select_target(&cat, None, None, Some(&live("NewChampion", "TOP")));
        assert_eq!(target.champion, "NewChampion");
        assert_eq!(target.champion_key, None);
        assert_eq!(target.position, Some(Position::Top));
        assert_eq!(target.source, "live");
        assert!(target.factual_pack(&pack::load_xayah().unwrap()).is_none());
    }

    #[test]
    fn only_an_explicit_offline_sample_uses_xayah_and_never_attaches_unknown_live_data() {
        let cat = catalog();
        let unknown = LiveSnapshot::default();
        let target = select_target(&cat, None, None, Some(&unknown));
        assert_eq!(target.champion, "Xayah");
        assert_eq!(target.position, Some(Position::Adc));
        assert_eq!(target.source, "offline_sample");
        assert!(!target.use_live);
        assert!(target
            .note
            .as_ref()
            .is_some_and(|note| note.contains("Offline sample")));
        assert!(target.factual_pack(&pack::load_xayah().unwrap()).is_some());
    }
}
