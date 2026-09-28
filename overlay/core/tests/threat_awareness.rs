//! From the draft game of 2026-09-26 (Lux Mid vs Azir; Darius Top 14/1 by 20:24, Graves, Yasuo,
//! Karma). Lux won lane but Darius, fed from 1/0 at 1:24 to 6/0 at 13:54, killed her at 13:48
//! with Azir's help, again at 19:36, and assisted at 16:12. The panel kept the core line (Luden's,
//! boots, Stormsurge) the whole game with Zhonya's fifth, and its tags and tail order flickered:
//! the threat weight used a ratio of levels, so an enemy at level 3 against level 2 counted 50%
//! more dangerous and every early level-up swung the defensive scores across the tag threshold.
//! The states below are rebuilt by hand from the scoreboard and kill feed; no recorded payload or
//! Riot ID is included.
use recall_core::{
    aggregate::{self, Aggregate, Position},
    ddragon::Catalog,
    engine::{self, Inputs, Plan, PlannerPreferences},
    live::{self, LiveSnapshot},
    pack,
};
use serde_json::{json, Value};

fn catalog() -> Catalog {
    Catalog::from_json(
        "16.17.1",
        &serde_json::from_str(include_str!("../../../m0/tests/fixtures/item_subset.json")).unwrap(),
        &serde_json::from_str(include_str!(
            "../../../m0/tests/fixtures/champion_subset.json"
        ))
        .unwrap(),
        &json!([]),
    )
}

fn aggregate() -> Aggregate {
    let raw =
        serde_json::from_str(include_str!("../../../m0/tests/fixtures/opgg_lux_mid.json")).unwrap();
    aggregate::decode(&raw, 99, Position::Mid, "global", "emerald_plus").unwrap()
}

const ZHONYAS: u32 = 3157;
const STORMSURGE: u32 = 4646;
const LUDENS: u32 = 6655;
const DORANS_RING: u32 = 1056;
const BOOTS: u32 = 1001;
const WARD: u32 = 3340;

/// One enemy on the scoreboard: champion, position, level, items, kills, deaths.
type Enemy<'a> = (&'a str, &'a str, u32, &'a [u32], u32, u32);

fn lineup<'a>(darius_items: &'a [u32], darius_kills: u32, darius_deaths: u32) -> [Enemy<'a>; 5] {
    [
        (
            "Darius",
            "TOP",
            13,
            darius_items,
            darius_kills,
            darius_deaths,
        ),
        ("Graves", "JUNGLE", 11, &[6676, 3047], 2, 0),
        ("Azir", "MIDDLE", 11, &[3020, 1058], 2, 3),
        ("Yasuo", "BOTTOM", 10, &[3031], 0, 5),
        ("Karma", "UTILITY", 10, &[3158], 1, 2),
    ]
}

/// A live state for Lux (level, gold, items, measured armor/MR/health) against `enemies`, with
/// her deaths as (game time, killer index, assister indices) into `enemies`.
fn snapshot(
    seconds: f64,
    level: u32,
    gold: f64,
    mine: &[u32],
    enemies: &[Enemy],
    deaths: &[(f64, usize, &[usize])],
) -> LiveSnapshot {
    let items = |ids: &[u32]| -> Vec<Value> {
        ids.iter()
            .enumerate()
            .map(|(slot, id)| {
                let slot = if *id == WARD { 6 } else { slot };
                json!({"itemID": id, "count": 1, "slot": slot})
            })
            .collect()
    };
    let mut players = vec![json!({
        "riotId": "Player#TEST", "riotIdGameName": "Player", "championName": "Lux",
        "team": "ORDER", "position": "MIDDLE", "level": level, "items": items(mine),
        "scores": {"kills": 3, "deaths": deaths.len(), "assists": 1, "creepScore": 70}
    })];
    for (i, (champion, position, level, owned, kills, died)) in enemies.iter().enumerate() {
        players.push(json!({
            "riotId": format!("Enemy{i}#TEST"), "riotIdGameName": format!("Enemy{i}"),
            "championName": champion, "team": "CHAOS", "position": position, "level": level,
            "items": items(owned),
            "scores": {"kills": kills, "deaths": died, "assists": 0, "creepScore": 100}
        }));
    }
    let events: Vec<Value> = deaths
        .iter()
        .enumerate()
        .map(|(id, (time, killer, assisters))| {
            json!({
                "EventID": id, "EventName": "ChampionKill", "EventTime": time,
                "KillerName": format!("Enemy{killer}"), "VictimName": "Player",
                "Assisters": assisters.iter().map(|a| format!("Enemy{a}")).collect::<Vec<_>>()
            })
        })
        .collect();
    live::summarize(&json!({
        "activePlayer": {
            "riotId": "Player#TEST", "riotIdGameName": "Player", "level": level,
            "currentGold": gold,
            "championStats": {"armor": 52.0, "magicResist": 45.0, "maxHealth": 1640.0,
                               "currentHealth": 1640.0}
        },
        "allPlayers": players,
        "events": {"Events": events},
        "gameData": {"gameTime": seconds, "gameMode": "CLASSIC"}
    }))
}

fn plan_with(live: &LiveSnapshot, preferences: &PlannerPreferences) -> Plan {
    let (cat, traits, a) = (catalog(), pack::load_traits().unwrap(), aggregate());
    let enemies: Vec<String> = live.enemies.iter().map(|p| p.champion.clone()).collect();
    engine::plan_with_preferences(
        &Inputs {
            champion: "Lux",
            pack: None,
            aggregate: Some(&a),
            traits: &traits,
            catalog: &cat,
            enemies: &enemies,
            live: Some(live),
        },
        preferences,
    )
}

fn plan(live: &LiveSnapshot) -> Plan {
    plan_with(live, &PlannerPreferences::default())
}

fn position(p: &Plan, id: u32) -> Option<usize> {
    p.path.iter().position(|i| i.id == id)
}

const AFTER_LUDENS: [u32; 4] = [DORANS_RING, LUDENS, BOOTS, WARD];
const DARIUS_FED: [u32; 3] = [1055, 3078, 3009];

#[test]
fn the_kill_feed_names_who_killed_you() {
    let live = snapshot(
        840.0,
        11,
        500.0,
        &AFTER_LUDENS,
        &lineup(&DARIUS_FED, 6, 0),
        &[(828.0, 0, &[2])],
    );
    assert_eq!(live.my_deaths.len(), 1);
    assert_eq!(live.my_deaths[0].killer, "Darius");
    assert_eq!(live.my_deaths[0].assisters, ["Azir"]);
}

#[test]
fn a_fed_enemy_without_a_kill_on_you_does_not_move_the_build() {
    // (a) 13:10 after Luden's: Darius is 5/0 but has not killed Lux. Nothing on the kill feed
    // ties him to her yet, so the core line stays and Zhonya's is not moved ahead.
    let p = plan(&snapshot(
        790.0,
        11,
        300.0,
        &AFTER_LUDENS,
        &lineup(&DARIUS_FED, 5, 0),
        &[],
    ));
    assert_ne!(p.next.as_ref().map(|n| n.id), Some(ZHONYAS), "{:?}", p.why);
    assert!(
        position(&p, STORMSURGE) < position(&p, ZHONYAS),
        "{:?}",
        p.path
    );
}

#[test]
fn a_losing_enemy_and_no_deaths_keep_the_core_line() {
    // (c) Control: same moment, but Darius is 0/3 and has not killed Lux.
    let p = plan(&snapshot(
        840.0,
        11,
        500.0,
        &AFTER_LUDENS,
        &lineup(&[1055, 3009], 0, 3),
        &[],
    ));
    assert_ne!(p.next.as_ref().map(|n| n.id), Some(ZHONYAS), "{:?}", p.why);
    assert!(
        position(&p, STORMSURGE) < position(&p, ZHONYAS),
        "{:?}",
        p.path
    );
}

#[test]
fn an_old_death_fades_and_the_core_line_resumes() {
    // Six minutes after the only death the evidence has faded: the build goes back to the core.
    let p = plan(&snapshot(
        1210.0,
        13,
        500.0,
        &AFTER_LUDENS,
        &lineup(&[1055, 3009], 1, 3),
        &[(828.0, 0, &[2])],
    ));
    assert_ne!(p.next.as_ref().map(|n| n.id), Some(ZHONYAS), "{:?}", p.why);
}

#[test]
fn small_level_and_gold_ticks_keep_the_same_path_and_tags() {
    // (d) Two consecutive polls that differ only by a few gold and one enemy level-up give the
    // same plan: first at 13:10, then at 13:12 with 40 more gold and Graves a level higher.
    let shape = |p: &Plan| -> Vec<(u32, Option<String>)> {
        p.path.iter().map(|i| (i.id, i.tag.clone())).collect()
    };
    let first = plan(&snapshot(
        790.0,
        11,
        300.0,
        &AFTER_LUDENS,
        &lineup(&DARIUS_FED, 5, 0),
        &[],
    ));
    let mut ticked = lineup(&DARIUS_FED, 5, 0);
    ticked[1].2 += 1;
    let second = plan_with(
        &snapshot(792.0, 11, 340.0, &AFTER_LUDENS, &ticked, &[]),
        &first.preferences,
    );
    assert_eq!(shape(&first), shape(&second));
    // The early game that flickered: Darius hits level 3 while Lux is level 2, then she levels.
    let early = |my_level: u32, seconds: f64, prefs: &PlannerPreferences| {
        let mut enemies = lineup(&[1055], 1, 0);
        for (i, enemy) in enemies.iter_mut().enumerate() {
            enemy.2 = if i == 0 { 3 } else { 2 };
            enemy.3 = if i == 0 { &[1055] } else { &[] };
        }
        plan_with(
            &snapshot(
                seconds,
                my_level,
                150.0,
                &[DORANS_RING, WARD],
                &enemies,
                &[],
            ),
            prefs,
        )
    };
    let behind = early(2, 84.0, &PlannerPreferences::default());
    let level = early(3, 90.0, &behind.preferences);
    assert_eq!(shape(&behind), shape(&level));
}
