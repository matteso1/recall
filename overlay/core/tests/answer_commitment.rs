//! From the draft game of 2026-09-26 (Xayah ADC 5/3/7, a low-farm game). At 13:26 Lillia (3/0)
//! killed Xayah with Yasuo assisting and the panel promoted Mercurial Scimitar, "Lillia (3/0)
//! killed you; its magic resist cuts that damage"; the player bought Null-Magic Mantle toward it.
//! At 17:18 Yasuo (8/3) killed her alone, the answer switched to Guardian Angel, Mercurial left the
//! path, and the player spent until 26:48 on Guardian Angel and sold the Mantle. An answer the
//! player has started buying now stays in front until finished, and the new answer follows it.
//! The states below are rebuilt by hand from the scoreboard and the kill feed; no recorded payload
//! or Riot ID is included.
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
    let raw = serde_json::from_str(include_str!(
        "../../../m0/tests/fixtures/opgg_xayah_adc.json"
    ))
    .unwrap();
    aggregate::decode(&raw, 498, Position::Adc, "global", "emerald_plus").unwrap()
}

const DORANS_BOW: u32 = 1086;
const YUN_TAL: u32 = 3032;
const DAGGER: u32 = 1042;
const FOOTWEAR: u32 = 2422;
const MANTLE: u32 = 1033;
const LONG_SWORD: u32 = 1036;
const MERCURIAL: u32 = 3139;
const GUARDIAN_ANGEL: u32 = 3026;
const LILLIA: usize = 1;
const YASUO: usize = 2;
/// Lillia's kill at 13:26 (Yasuo assisting) and Yasuo's solo kill at 17:18.
const FIRST_DEATH: (f64, usize, &[usize]) = (806.0, LILLIA, &[YASUO]);
const SECOND_DEATH: (f64, usize, &[usize]) = (1038.0, YASUO, &[]);

/// One enemy on the scoreboard: champion, position, level, items, kills, deaths.
type Enemy = (&'static str, &'static str, u32, &'static [u32], u32, u32);

/// The enemy team as it was at `seconds` (13:42, 16:12 or 17:30 in the recorded game).
fn lineup(seconds: f64) -> [Enemy; 5] {
    if seconds < 900.0 {
        [
            ("Garen", "TOP", 10, &[1054, 3077, 3044, 1042], 1, 1),
            ("Lillia", "JUNGLE", 9, &[1102, 3147, 6653, 2422], 3, 0),
            ("Yasuo", "MIDDLE", 10, &[1086, 1043, 1053, 1001, 1042], 6, 2),
            ("Jinx", "BOTTOM", 8, &[1055, 1037, 6670, 1036], 0, 4),
            ("Zyra", "UTILITY", 8, &[3147, 1052, 3866, 2031, 2422], 0, 1),
        ]
    } else if seconds < 1000.0 {
        [
            ("Garen", "TOP", 12, &[1054, 6631], 1, 1),
            ("Lillia", "JUNGLE", 11, &[4633, 6653, 2422], 5, 0),
            ("Yasuo", "MIDDLE", 11, &[1086, 3153, 1037, 3172, 1018], 7, 3),
            ("Jinx", "BOTTOM", 8, &[1055, 2523, 1018, 1042, 1042], 0, 5),
            ("Zyra", "UTILITY", 9, &[3147, 1052, 3871, 2031, 3047], 0, 1),
        ]
    } else {
        [
            ("Garen", "TOP", 12, &[1054, 6631], 1, 1),
            ("Lillia", "JUNGLE", 12, &[4633, 6653, 2422], 5, 0),
            ("Yasuo", "MIDDLE", 12, &[1086, 3153, 1037, 3172, 1018], 8, 3),
            ("Jinx", "BOTTOM", 9, &[1055, 2523, 3086, 3144], 0, 6),
            ("Zyra", "UTILITY", 9, &[3147, 1052, 3871, 2031, 3047], 1, 1),
        ]
    }
}

/// A live Xayah state with the client's measured stats (armor, magic resist, health) and her
/// deaths as (game time, killer index, assister indices).
fn snapshot(
    seconds: f64,
    level: u32,
    gold: f64,
    mine: &[u32],
    stats: (f64, f64, f64),
    deaths: &[(f64, usize, &[usize])],
) -> LiveSnapshot {
    let items = |ids: &[u32]| -> Vec<Value> {
        ids.iter()
            .enumerate()
            .map(|(slot, id)| json!({"itemID": id, "count": 1, "slot": slot}))
            .collect()
    };
    let mut players = vec![json!({
        "riotId": "Player#TEST", "riotIdGameName": "Player", "championName": "Xayah",
        "team": "ORDER", "position": "BOTTOM", "level": level, "items": items(mine),
        "scores": {"kills": 3, "deaths": deaths.len(), "assists": 2, "creepScore": 60}
    })];
    for (i, (champion, position, level, owned, kills, died)) in lineup(seconds).iter().enumerate() {
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
    let (armor, magic_resist, health) = stats;
    live::summarize(&json!({
        "activePlayer": {
            "riotId": "Player#TEST", "riotIdGameName": "Player", "level": level,
            "currentGold": gold,
            "championStats": {"armor": armor, "magicResist": magic_resist,
                               "maxHealth": health, "currentHealth": health}
        },
        "allPlayers": players,
        "events": {"Events": events},
        "gameData": {"gameTime": seconds, "gameMode": "CLASSIC"}
    }))
}

fn plan(live: &LiveSnapshot, preferences: &PlannerPreferences) -> Plan {
    let (cat, traits, a) = (catalog(), pack::load_traits().unwrap(), aggregate());
    let enemies: Vec<String> = live.enemies.iter().map(|p| p.champion.clone()).collect();
    engine::plan_with_preferences(
        &Inputs {
            champion: "Xayah",
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

/// 13:42 (just after Lillia's kill), then 16:12 with or without the Null-Magic Mantle bought,
/// each poll carrying the previous plan's preferences as the overlay does.
fn after_the_first_death(with_mantle: bool) -> Plan {
    let first = plan(
        &snapshot(
            822.0,
            7,
            83.0,
            &[DORANS_BOW, YUN_TAL, DAGGER, FOOTWEAR],
            (45.3, 38.3, 1308.0),
            &[FIRST_DEATH],
        ),
        &PlannerPreferences::default(),
    );
    let mine: &[u32] = if with_mantle {
        &[DORANS_BOW, YUN_TAL, MANTLE]
    } else {
        &[DORANS_BOW, YUN_TAL]
    };
    let mr = if with_mantle { 62.3 } else { 42.3 };
    plan(
        &snapshot(972.0, 8, 46.0, mine, (51.7, mr, 1418.0), &[FIRST_DEATH]),
        &first.preferences,
    )
}

/// 17:30, twelve seconds after Yasuo's solo kill.
fn after_the_second_death(with_mantle: bool) -> Plan {
    let before = after_the_first_death(with_mantle);
    let mine: &[u32] = if with_mantle {
        &[DORANS_BOW, YUN_TAL, MANTLE, LONG_SWORD]
    } else {
        &[DORANS_BOW, YUN_TAL, LONG_SWORD]
    };
    let mr = if with_mantle { 62.3 } else { 42.3 };
    plan(
        &snapshot(
            1050.0,
            8,
            96.0,
            mine,
            (51.7, mr, 1418.0),
            &[FIRST_DEATH, SECOND_DEATH],
        ),
        &before.preferences,
    )
}

fn first_to_buy(p: &Plan) -> u32 {
    p.path.iter().find(|i| !i.owned).map(|i| i.id).unwrap()
}

#[test]
fn lillias_kill_promotes_mercurial_scimitar() {
    let p = plan(
        &snapshot(
            822.0,
            7,
            83.0,
            &[DORANS_BOW, YUN_TAL, DAGGER, FOOTWEAR],
            (45.3, 38.3, 1308.0),
            &[FIRST_DEATH],
        ),
        &PlannerPreferences::default(),
    );
    assert_eq!(p.preferences.promoted, Some(MERCURIAL), "{:?}", p.path);
    let why = p
        .path
        .iter()
        .find(|i| i.id == MERCURIAL)
        .unwrap()
        .why
        .clone();
    assert!(
        why.as_deref().unwrap_or_default().contains("Lillia (3/0)"),
        "{why:?}"
    );
}

#[test]
fn a_started_answer_stays_in_front_and_the_new_one_follows_it() {
    // Null-Magic Mantle owned: Mercurial stays the promoted answer and the first thing to buy
    // after Yasuo's kill, saying what is already owned; Guardian Angel, the answer to Yasuo, may
    // follow it but does not replace it, and the Mantle's item never leaves the path.
    let p = after_the_second_death(true);
    assert_eq!(p.preferences.promoted, Some(MERCURIAL), "{:?}", p.path);
    assert_eq!(first_to_buy(&p), MERCURIAL, "{:?}", p.path);
    assert_eq!(
        p.next.as_ref().map(|n| n.id),
        Some(MERCURIAL),
        "{:?}",
        p.score_trace
    );
    let entry = p.path.iter().find(|i| i.id == MERCURIAL).unwrap();
    let why = entry.why.as_deref().unwrap_or_default();
    assert!(why.contains("you already own Null-Magic Mantle"), "{why}");
    let merc = p.path.iter().position(|i| i.id == MERCURIAL).unwrap();
    let angel = p
        .path
        .iter()
        .position(|i| i.id == GUARDIAN_ANGEL)
        .expect("Guardian Angel follows as the answer to Yasuo");
    assert_eq!(angel, merc + 1, "{:?}", p.path);
    let why = p.path[angel].why.as_deref().unwrap_or_default();
    assert!(why.contains("Yasuo (8/3) killed you"), "{why}");
}

#[test]
fn without_anything_started_the_answer_may_switch_to_the_new_threat() {
    // Control: the same 17:30 state without the Mantle. Nothing was bought toward Mercurial, so
    // the answer to Yasuo's damage (armor) may take over, as it did in the recorded game.
    let p = after_the_second_death(false);
    let promoted = p.preferences.promoted.expect("an answer is promoted");
    assert_ne!(promoted, MERCURIAL, "{:?}", p.path);
    let item = catalog().item(promoted).cloned().unwrap();
    assert!(item.effects.armor.is_some_and(|a| a > 0.0), "{}", item.name);
}

#[test]
fn a_started_answer_outlasts_the_kill_feed_evidence() {
    // 24:00, more than six minutes after Yasuo's kill: no death is remembered, so no enemy is
    // blamed. The Mantle is still owned and Lillia (5/0, magic damage) is still ahead, so Mercurial
    // stays in front instead of dropping off the path (the recorded game lost it at 23:24).
    let before = after_the_second_death(true);
    let p = plan(
        &snapshot(
            1440.0,
            9,
            300.0,
            &[DORANS_BOW, YUN_TAL, MANTLE, LONG_SWORD],
            (55.0, 62.3, 1500.0),
            &[FIRST_DEATH, SECOND_DEATH],
        ),
        &before.preferences,
    );
    assert_eq!(p.preferences.promoted, Some(MERCURIAL), "{:?}", p.path);
    assert_eq!(first_to_buy(&p), MERCURIAL, "{:?}", p.path);
    let why = p
        .path
        .iter()
        .find(|i| i.id == MERCURIAL)
        .and_then(|i| i.why.clone())
        .unwrap_or_default();
    assert!(why.contains("you already own Null-Magic Mantle"), "{why}");
}
