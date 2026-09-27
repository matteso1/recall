//! From the draft game of 2026-09-26 (Xayah ADC 28/6/16 over 38 minutes). Orianna mid went 7/0 by
//! 16:54 and 19/8 by the end and took part in all six of Xayah's deaths, but the engine blamed
//! whoever landed the kill: "BT: Rengar (2/13) killed you" at 18:36, "BT: Miss Fortune (2/13)
//! killed you" at 24:30, and always promoted Bloodthirster's generic shield. Mercurial Scimitar,
//! the magic-resist item in Xayah's candidate pool, became the target only at 30:54 and 36:24, and
//! from 23:40 to 28:00 the target traded Bloodthirster and Mortal Reminder six times as the death
//! evidence faded and renewed. The states below are rebuilt by hand from the scoreboard and the
//! kill feed; no recorded payload or Riot ID is included.
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

const YUN_TAL: u32 = 3032;
const NAVORI: u32 = 6675;
const CLOAK: u32 = 1018;
const MERCURIAL: u32 = 3139;
const BLOODTHIRSTER: u32 = 3072;
const WARD: u32 = 3340;
const ORIANNA: usize = 2;
const MISS_FORTUNE: usize = 3;
const RENGAR: usize = 4;
const SETT: usize = 0;

/// One enemy on the scoreboard: champion, position, level, items, kills, deaths.
type Enemy = (&'static str, &'static str, u32, Vec<u32>, u32, u32);

/// The enemy team at 18:36, when Rengar (2/13) landed the kill with Orianna (9/1) and Miss Fortune
/// assisting. `fed_sett` swaps in a Sett who is 9/1 at level 14 with Stridebreaker and Sterak's
/// Gage, for the physical-killer control.
fn lineup(fed_sett: bool) -> Vec<Enemy> {
    let sett = if fed_sett {
        ("Sett", "TOP", 14, vec![6631, 3009, 3053], 9, 1)
    } else {
        ("Sett", "TOP", 12, vec![1055, 1042, 3009, 3044], 1, 4)
    };
    vec![
        sett,
        ("Maokai", "JUNGLE", 10, vec![1026, 3158, 6653, 1052], 3, 2),
        (
            "Orianna",
            "MIDDLE",
            13,
            vec![1056, 6655, 3020, 3145, 1058],
            9,
            1,
        ),
        ("Miss Fortune", "BOTTOM", 10, vec![1086, 2010, 6676], 1, 10),
        ("Rengar", "UTILITY", 9, vec![3867, 3179, 3134, 1028], 2, 13),
    ]
}

/// A live Xayah state (measured armor 66, magic resist 44, health 1978 as at 18:36) with her
/// deaths as (game time, killer index, assister indices) into `enemies`.
fn snapshot(
    seconds: f64,
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
        "riotId": "Player#TEST", "riotIdGameName": "Player", "championName": "Xayah",
        "team": "CHAOS", "position": "BOTTOM", "level": 12, "items": items(mine),
        "scores": {"kills": 16, "deaths": deaths.len(), "assists": 5, "creepScore": 120}
    })];
    for (i, (champion, position, level, owned, kills, died)) in enemies.iter().enumerate() {
        players.push(json!({
            "riotId": format!("Enemy{i}#TEST"), "riotIdGameName": format!("Enemy{i}"),
            "championName": champion, "team": "ORDER", "position": position, "level": level,
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
            "riotId": "Player#TEST", "riotIdGameName": "Player", "level": 12, "currentGold": gold,
            "championStats": {"armor": 66.0, "magicResist": 44.0, "maxHealth": 1978.0,
                               "currentHealth": 1978.0}
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

const AT_THE_DEATH: [u32; 4] = [YUN_TAL, NAVORI, CLOAK, WARD];

fn at_the_death(fed_sett: bool, deaths: &[(f64, usize, &[usize])], gold: f64) -> LiveSnapshot {
    snapshot(1120.0, gold, &AT_THE_DEATH, &lineup(fed_sett), deaths)
}

#[test]
fn the_fed_enemy_behind_a_death_is_blamed_not_the_one_who_landed_it() {
    // 18:40, four seconds after Rengar (2/13) landed the kill with Orianna (9/1) and Miss Fortune
    // (1/10) assisting: Orianna is named, honestly as helping, and the magic-resist answer from
    // Xayah's pool moves to the front of what is left to buy (the path was full of core items and
    // boots, so it takes the place of the weakest flexible item). With 1370 gold an affordable
    // Executioner's Calling (anti-heal for Maokai) may still be the purchase of the moment.
    let p = plan_with(
        &at_the_death(false, &[(1116.0, RENGAR, &[ORIANNA, MISS_FORTUNE])], 1370.0),
        &PlannerPreferences::default(),
    );
    assert_eq!(p.preferences.promoted, Some(MERCURIAL), "{:?}", p.path);
    let first = p
        .path
        .iter()
        .find(|i| !i.owned)
        .expect("something left to buy");
    assert_eq!(first.id, MERCURIAL, "{:?}", p.path);
    let why = first.why.as_deref().unwrap_or_default();
    assert!(
        why.contains("Orianna (9/1)") && why.contains("helped kill you") && !why.contains("Rengar"),
        "{why}"
    );
    let total = |id: u32| p.score_trace.iter().find(|s| s.id == id).map(|s| s.total);
    assert!(
        total(MERCURIAL) > total(BLOODTHIRSTER),
        "{:?}",
        p.score_trace
    );
    assert!(p
        .path
        .iter()
        .all(|i| !i.why.as_deref().unwrap_or_default().contains("Rengar")));
}

#[test]
fn a_death_to_a_fed_physical_enemy_still_gets_an_armor_or_buffer_answer() {
    // Control: Sett, fed (9/1, level 14, Stridebreaker and Sterak's Gage), lands the kill alone.
    // Magic resist does not answer him; the promoted answer resists physical damage or is a
    // buffer, and names Sett.
    let p = plan_with(
        &at_the_death(true, &[(1116.0, SETT, &[])], 1370.0),
        &PlannerPreferences::default(),
    );
    let promoted = p.preferences.promoted.expect("an answer is promoted");
    assert_ne!(promoted, MERCURIAL, "{:?}", p.why);
    let item = catalog().item(promoted).cloned().unwrap();
    assert!(
        item.effects.armor.is_some_and(|a| a > 0.0)
            || item.effects.shield.is_some()
            || item.effects.stasis,
        "{}",
        item.name
    );
    let entry = p.path.iter().find(|i| i.id == promoted).unwrap();
    let why = entry.why.as_deref().unwrap_or_default();
    assert!(why.contains("Sett (9/1) killed you"), "{why}");
}

#[test]
fn a_near_tie_keeps_the_target_across_consecutive_polls() {
    // Two polls two seconds apart with 20 more gold: the same target. And when the target shown
    // last poll is the runner-up within `TARGET_MARGIN`, it stays rather than flipping back (the
    // Bloodthirster / Mortal Reminder trade from 23:40 to 28:00).
    let deaths: &[(f64, usize, &[usize])] = &[(1116.0, RENGAR, &[ORIANNA, MISS_FORTUNE])];
    let first = plan_with(
        &at_the_death(false, deaths, 1370.0),
        &PlannerPreferences::default(),
    );
    let mut later = at_the_death(false, deaths, 1390.0);
    later.game_time = 1122.0;
    let second = plan_with(&later, &first.preferences);
    let target = |p: &Plan| p.next.as_ref().map(|n| n.id);
    assert_eq!(target(&first), target(&second));

    let leader = first.score_trace[0].clone();
    let runner_up = first
        .score_trace
        .iter()
        .skip(1)
        .find(|s| leader.total - s.total < 0.75)
        .expect("this state has a near-tie")
        .clone();
    let shown = PlannerPreferences {
        last_target: Some(runner_up.id),
        ..first.preferences.clone()
    };
    let kept = plan_with(&at_the_death(false, deaths, 1370.0), &shown);
    assert_eq!(target(&kept), Some(runner_up.id), "{:?}", kept.score_trace);
}
