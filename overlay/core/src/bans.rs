//! Who to ban for the champion you intend to play. `data/pack/bans.json` holds, per champion and role, the
//! enemy champions whose presence cost the most expected win rate in ~107k ranked Master+ player-games
//! (Kaggle ranked-timeline, 16.13-16.18): share of games they appear in x (base win rate - win rate with
//! them in the game, shrunk toward the base by 30 games). This ranks associations, not causal gains.
//! Built by `tools/priors/export_bans.py`. Sparse champion-roles use matching provider matchups
//! when available, then the role's table.
use crate::aggregate::{Aggregate, Position};
use crate::champselect::Lobby;
use crate::ddragon::Catalog;
use crate::state::{BanAdvice, BanView};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

const JSON: &str = include_str!("../../../data/pack/bans.json");
const SHOWN: usize = 3;

#[derive(Deserialize)]
struct Ban {
    id: u32,
    seen: f64,
    win_vs: f64,
}

#[derive(Deserialize)]
struct Table {
    games: u32,
    base: f64,
    bans: Vec<Ban>,
}

#[derive(Deserialize)]
struct Doc {
    champions: HashMap<String, Table>,
    roles: HashMap<String, Table>,
}

fn doc() -> &'static Doc {
    static DOC: OnceLock<Doc> = OnceLock::new();
    DOC.get_or_init(|| serde_json::from_str(JSON).expect("data/pack/bans.json"))
}

/// The role the champion is played in most, for lobbies without assigned positions.
fn main_role(champion_key: u32) -> Option<Position> {
    Position::ALL
        .into_iter()
        .filter_map(|role| {
            doc()
                .champions
                .get(&format!("{champion_key}|{}", role.label()))
                .map(|t| (role, t.games))
        })
        .max_by_key(|(_, games)| *games)
        .map(|(role, _)| role)
}

/// Bans for `champion_key` (0 = not chosen yet) in `role`, skipping `exclude` (picked, hovered or banned).
pub fn suggest(
    cat: &Catalog,
    champion_key: u32,
    role: Option<Position>,
    exclude: &[u32],
) -> Option<BanAdvice> {
    suggest_with_aggregate(cat, champion_key, role, exclude, None)
}

/// Prefer the corpus champion table, then matching provider lane-opponent game outcomes,
/// then generic role bans. Never borrow another champion's or another role's matchups.
pub fn suggest_with_aggregate(
    cat: &Catalog,
    champion_key: u32,
    role: Option<Position>,
    exclude: &[u32],
    aggregate: Option<&Aggregate>,
) -> Option<BanAdvice> {
    let role = role.or_else(|| main_role(champion_key)).or_else(|| {
        aggregate
            .filter(|a| a.champion_key == champion_key && a.requested_position.is_none())
            .map(|a| a.position)
    })?;
    let d = doc();
    let own = (champion_key > 0)
        .then(|| d.champions.get(&format!("{champion_key}|{}", role.label())))
        .flatten();
    if own.is_none() {
        if let Some(advice) =
            aggregate.and_then(|a| provider_bans(cat, champion_key, role, exclude, a))
        {
            return Some(advice);
        }
    }
    let (table, who, label) = match own {
        Some(t) => {
            let name = cat.champion_name(champion_key);
            (t, name.clone(), format!("{name} {}", role.label()))
        }
        None => (
            d.roles.get(role.label())?,
            format!("{} players", role.label()),
            role.label().to_string(),
        ),
    };
    let picks: Vec<BanView> = table
        .bans
        .iter()
        .filter(|b| b.id != champion_key && !exclude.contains(&b.id))
        .take(SHOWN)
        .map(|b| {
            let name = cat.champion_name(b.id);
            BanView {
                reason: format!(
                    "{who} win {:.0}% with {name} in the game ({:.0}% overall); {name} is in {:.0}% of their games.",
                    100.0 * b.win_vs,
                    100.0 * table.base,
                    100.0 * b.seen
                ),
                name,
            }
        })
        .collect();
    (!picks.is_empty()).then(|| BanAdvice {
        matchup: own.is_some(),
        label,
        picks,
    })
}

fn provider_bans(
    cat: &Catalog,
    champion_key: u32,
    role: Position,
    exclude: &[u32],
    agg: &Aggregate,
) -> Option<BanAdvice> {
    if champion_key == 0
        || agg.champion_key != champion_key
        || agg.position != role
        || agg.games < 200
        || !agg.win_rate.is_finite()
        || !(0.0..=1.0).contains(&agg.win_rate)
    {
        return None;
    }
    // Same shrinkage as the corpus exporter; require 50 observed matchup games,
    // matching the existing matchup panel's minimum. Frequency breaks tiny-sample hype.
    let mut rows: Vec<_> = agg
        .counters
        .iter()
        .filter_map(|&(id, games, wins)| {
            if games < 50
                || games > agg.games
                || wins > games
                || id == champion_key
                || exclude.contains(&id)
            {
                return None;
            }
            let shrunk = (f64::from(wins) + 30.0 * agg.win_rate) / (f64::from(games) + 30.0);
            let value = f64::from(games) / f64::from(agg.games) * (agg.win_rate - shrunk);
            (value > 0.0).then_some((id, games, wins, value))
        })
        .collect();
    rows.sort_by(|a, b| b.3.total_cmp(&a.3).then_with(|| a.0.cmp(&b.0)));
    let who = cat.champion_name(champion_key);
    let picks: Vec<_> = rows
        .into_iter()
        .take(SHOWN)
        .map(|(id, games, wins, _)| {
            let name = cat.champion_name(id);
            BanView {
                reason: format!(
                    "op.gg: {who} won {:.0}% of {games} games against {name} ({:.0}% overall).",
                    100.0 * f64::from(wins) / f64::from(games),
                    100.0 * agg.win_rate
                ),
                name,
            }
        })
        .collect();
    (!picks.is_empty()).then(|| BanAdvice {
        matchup: true,
        label: format!("{who} {} · op.gg", role.label()),
        picks,
    })
}

/// Ban advice while the player's own ban is still to come; None otherwise.
pub fn for_lobby(lobby: &Lobby, cat: &Catalog) -> Option<BanAdvice> {
    for_lobby_with_aggregate(lobby, cat, None)
}

pub fn for_lobby_with_aggregate(
    lobby: &Lobby,
    cat: &Catalog,
    aggregate: Option<&Aggregate>,
) -> Option<BanAdvice> {
    if !lobby.my_ban_pending {
        return None;
    }
    let exclude: Vec<u32> = [
        &lobby.allies,
        &lobby.enemies,
        &lobby.ally_bans,
        &lobby.enemy_bans,
    ]
    .into_iter()
    .flatten()
    .copied()
    .collect();
    suggest_with_aggregate(
        cat,
        lobby.my_champion,
        Position::parse(&lobby.my_position),
        &exclude,
        aggregate,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ddragon::test_support::catalog;

    const XAYAH: u32 = 498;
    const TRISTANA: u32 = 18;
    const TEEMO: u32 = 17;

    fn sparse_champion_aggregate() -> Aggregate {
        let raw = serde_json::from_str(include_str!(
            "../../../m0/tests/fixtures/opgg_xayah_adc.json"
        ))
        .unwrap();
        let mut agg =
            crate::aggregate::decode(&raw, XAYAH, Position::Adc, "global", "emerald_plus").unwrap();
        agg.champion_key = 895;
        agg.games = 5000;
        agg.win_rate = 0.51;
        // An adverse common matchup, a favourable one, and an unreliable tiny sample.
        agg.counters = vec![(222, 600, 276), (51, 450, 240), (18, 4, 0)];
        agg
    }

    #[test]
    fn sparse_champion_uses_its_matching_provider_matchups_before_generic_role_bans() {
        let agg = sparse_champion_aggregate();
        let advice =
            suggest_with_aggregate(&catalog(), 895, Some(Position::Adc), &[], Some(&agg)).unwrap();
        assert!(advice.matchup);
        assert!(advice.label.contains("op.gg"));
        assert_eq!(advice.picks.len(), 1);
        assert_eq!(advice.picks[0].name, catalog().champion_name(222));
        assert!(advice.picks[0].reason.contains("600 games"));
        assert!(!advice.picks[0].reason.contains("lane win"));
        let excluded =
            suggest_with_aggregate(&catalog(), 895, Some(Position::Adc), &[222], Some(&agg))
                .unwrap();
        assert!(!excluded.matchup);
    }

    #[test]
    fn provider_fallback_rejects_wrong_champion_wrong_role_and_malformed_counts() {
        let mut agg = sparse_champion_aggregate();
        for (champion, role) in [(17, Position::Adc), (895, Position::Top)] {
            assert!(
                !suggest_with_aggregate(&catalog(), champion, Some(role), &[], Some(&agg))
                    .unwrap()
                    .matchup
            );
        }
        agg.counters = vec![(222, 600, 601), (51, 6000, 1), (18, 4, 0)];
        assert!(
            !suggest_with_aggregate(&catalog(), 895, Some(Position::Adc), &[], Some(&agg))
                .unwrap()
                .matchup
        );
    }

    #[test]
    fn provider_fallback_does_not_replace_a_master_champion_table() {
        let mut agg = sparse_champion_aggregate();
        agg.champion_key = XAYAH;
        let advice =
            suggest_with_aggregate(&catalog(), XAYAH, Some(Position::Adc), &[], Some(&agg))
                .unwrap();
        assert_eq!(advice.picks[0].name, "Tristana");
        assert_eq!(advice.label, "Xayah ADC");
    }

    #[test]
    fn xayah_bans_tristana_first() {
        let advice = suggest(&catalog(), XAYAH, Some(Position::Adc), &[]).unwrap();
        assert!(advice.matchup);
        assert_eq!(advice.label, "Xayah ADC");
        assert_eq!(advice.picks.len(), 3);
        assert_eq!(advice.picks[0].name, "Tristana");
        assert!(
            advice.picks[0]
                .reason
                .starts_with("Xayah win 49% with Tristana"),
            "{}",
            advice.picks[0].reason
        );
    }

    #[test]
    fn taken_and_banned_champions_are_skipped() {
        let advice = suggest(&catalog(), XAYAH, Some(Position::Adc), &[TRISTANA]).unwrap();
        assert!(advice.picks.iter().all(|b| b.name != "Tristana"));
    }

    #[test]
    fn uncovered_champions_and_unknown_positions_fall_back() {
        let cat = catalog();
        // Teemo top has too few games: the top-lane table, said as such.
        let advice = suggest(&cat, TEEMO, Some(Position::Top), &[]).unwrap();
        assert!(!advice.matchup);
        assert_eq!(advice.label, "Top");
        // No assigned position: the champion's main role.
        assert_eq!(suggest(&cat, XAYAH, None, &[]).unwrap().label, "Xayah ADC");
        assert!(suggest(&cat, 0, None, &[]).is_none());
    }

    #[test]
    fn advice_only_until_the_players_ban_is_done() {
        let cat = catalog();
        let mut lobby = Lobby {
            my_champion: XAYAH,
            my_position: "bottom".into(),
            allies: vec![XAYAH, TRISTANA],
            my_ban_pending: true,
            ..Default::default()
        };
        let advice = for_lobby(&lobby, &cat).unwrap();
        assert!(
            advice.picks.iter().all(|b| b.name != "Tristana"),
            "an ally's pick"
        );
        lobby.my_ban_pending = false;
        assert!(for_lobby(&lobby, &cat).is_none());
    }
}
