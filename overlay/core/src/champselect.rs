//! Champ select session -> who is on each side (public information only).
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct Lobby {
    /// Timer phase: PLANNING | BAN_PICK | FINALIZATION | GAME_STARTING
    pub phase: String,
    pub my_cell: i64,
    /// Locked or hovered champion (0 = none yet)
    pub my_champion: u32,
    pub my_locked: bool,
    /// top | jungle | middle | bottom | utility | "" (custom games, blind pick)
    pub my_position: String,
    /// My current summoner spells (D, F); 0 when unknown
    pub my_spells: (u32, u32),
    /// Ally champions (locked or hovered), including me
    pub allies: Vec<u32>,
    /// Enemy champions - only visible once locked
    pub enemies: Vec<u32>,
    pub ally_bans: Vec<u32>,
    pub enemy_bans: Vec<u32>,
    pub is_custom: bool,
}

impl Lobby {
    pub fn all_enemies_locked(&self, expected: usize) -> bool {
        self.enemies.len() >= expected
    }
}

fn u32_of(v: &Value, key: &str) -> u32 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0) as u32
}

fn ids(v: Option<&Value>) -> Vec<u32> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_u64).filter(|&x| x > 0).map(|x| x as u32).collect())
        .unwrap_or_default()
}

pub fn extract(session: &Value) -> Lobby {
    let my_cell = session.get("localPlayerCellId").and_then(Value::as_i64).unwrap_or(-1);
    let phase = session
        .get("timer")
        .and_then(|t| t.get("phase"))
        .and_then(Value::as_str)
        .unwrap_or("?")
        .to_string();

    let mut completed: HashSet<i64> = HashSet::new();
    for group in session.get("actions").and_then(Value::as_array).into_iter().flatten() {
        for a in group.as_array().into_iter().flatten() {
            if a.get("type").and_then(Value::as_str) == Some("pick")
                && a.get("completed").and_then(Value::as_bool).unwrap_or(false)
            {
                completed.insert(a.get("actorCellId").and_then(Value::as_i64).unwrap_or(-1));
            }
        }
    }

    let mut lobby = Lobby { phase, my_cell, ..Default::default() };
    for p in session.get("myTeam").and_then(Value::as_array).into_iter().flatten() {
        let cell = p.get("cellId").and_then(Value::as_i64).unwrap_or(-1);
        let champ = u32_of(p, "championId");
        let intent = u32_of(p, "championPickIntent");
        let shown = if champ > 0 { champ } else { intent };
        if cell == my_cell {
            lobby.my_champion = shown;
            lobby.my_locked = completed.contains(&cell)
                || (champ > 0 && matches!(lobby.phase.as_str(), "FINALIZATION" | "GAME_STARTING"));
            lobby.my_position = p
                .get("assignedPosition")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_ascii_lowercase();
            lobby.my_spells = (u32_of(p, "spell1Id"), u32_of(p, "spell2Id"));
        }
        if shown > 0 {
            lobby.allies.push(shown);
        }
    }
    for p in session.get("theirTeam").and_then(Value::as_array).into_iter().flatten() {
        let champ = u32_of(p, "championId");
        if champ > 0 {
            lobby.enemies.push(champ);
        }
    }
    let bans = session.get("bans").cloned().unwrap_or(Value::Null);
    lobby.ally_bans = ids(bans.get("myTeamBans"));
    lobby.enemy_bans = ids(bans.get("theirTeamBans"));
    lobby.is_custom = session.get("isCustomGame").and_then(Value::as_bool).unwrap_or(false);
    lobby
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOVER: &str = include_str!("../../../m0/tests/fixtures/champselect_practicetool_hover.json");
    const LOCK: &str = include_str!("../../../m0/tests/fixtures/champselect_practicetool_lock.json");
    const DRAFT: &str = include_str!("../../../m0/tests/fixtures/champselect_session.json");

    #[test]
    fn real_practice_tool_hover_then_lock() {
        let hover = extract(&serde_json::from_str(HOVER).unwrap());
        assert_eq!((hover.my_cell, hover.my_champion, hover.my_locked), (0, 498, false));
        assert!(hover.is_custom);
        let lock = extract(&serde_json::from_str(LOCK).unwrap());
        assert_eq!((lock.my_champion, lock.my_locked, lock.phase.as_str()), (498, true, "FINALIZATION"));
        assert!(lock.my_spells.0 > 0 && lock.my_spells.1 > 0, "{:?}", lock.my_spells);
        assert!(lock.enemies.is_empty());
    }

    #[test]
    fn draft_lobby_sides_and_bans() {
        let lobby = extract(&serde_json::from_str(DRAFT).unwrap());
        assert_eq!(lobby.my_cell, 3);
        assert_eq!(lobby.my_position, "bottom");
        assert_eq!(lobby.my_champion, 498);
        assert!(lobby.my_locked);
        assert_eq!(lobby.allies, vec![75, 62, 238, 498, 497]);
        assert_eq!(lobby.enemies, vec![18, 16, 54, 516]);
        assert_eq!(lobby.ally_bans, vec![350, 81]);
        assert_eq!(lobby.enemy_bans, vec![117]);
    }
}
