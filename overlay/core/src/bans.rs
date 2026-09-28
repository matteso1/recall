//! Who to ban for the champion you intend to play. `data/pack/bans.json` holds, per champion and role, the
//! enemy champions whose presence cost the most expected win rate in ~107k ranked Master+ player-games
//! (Kaggle ranked-timeline, 16.13-16.18): share of games they appear in x (base win rate - win rate with
//! them in the game, shrunk toward the base by 30 games). Banning removes them, so that product is the
//! expected gain of the ban. Associational, like every statistic here. Built by
//! `tools/priors/export_bans.py`; champion-roles under 200 games use the role's table.
use crate::aggregate::Position;
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
    let role = role.or_else(|| main_role(champion_key))?;
    let d = doc();
    let own = (champion_key > 0)
        .then(|| d.champions.get(&format!("{champion_key}|{}", role.label())))
        .flatten();
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

/// Ban advice while the player's own ban is still to come; None otherwise.
pub fn for_lobby(lobby: &Lobby, cat: &Catalog) -> Option<BanAdvice> {
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
    suggest(
        cat,
        lobby.my_champion,
        Position::parse(&lobby.my_position),
        &exclude,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ddragon::test_support::catalog;

    const XAYAH: u32 = 498;
    const TRISTANA: u32 = 18;
    const TEEMO: u32 = 17;

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
