//! Swiftplay's pre-queue champion choices and isolated loadout preparation.
use crate::aggregate::{Position, RunePageIds};
use crate::runes::{order_spells, spell_name};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

#[derive(Clone, Debug)]
pub struct SwiftplaySlot {
    pub index: usize,
    pub champion_id: u32,
    pub position: Position,
    pub spell_ids: [u32; 2],
    pub raw: Value,
}

#[derive(Clone, Debug)]
pub struct SwiftplayLobby {
    pub slots: Vec<SwiftplaySlot>,
    pub raw_slots: Value,
    pub party_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SlotUpdate {
    pub index: usize,
    pub rune_page: Option<RunePageIds>,
    pub spells: Option<[u32; 2]>,
}

/// Swiftplay binds a page to a champion, not to the globally current editor page.
/// A shared/ambiguous association must not let one choice overwrite another.
pub fn linked_rune_page(pages: &Value, champion: u32) -> Result<&Value> {
    let pages = pages
        .as_array()
        .context("Client rune pages are unavailable")?;
    let mut linked = pages.iter().filter(|p| {
        p["quickPlayChampionIds"]
            .as_array()
            .is_some_and(|ids| ids.contains(&json!(champion)))
    });
    let page = linked
        .next()
        .context("Client has not linked a rune page to this Swiftplay choice yet")?;
    if linked.next().is_some() || page["quickPlayChampionIds"] != json!([champion]) {
        bail!("Swiftplay rune page is shared or ambiguous; your pages were kept");
    }
    page["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .context("Swiftplay rune page has no valid id")?;
    Ok(page)
}

/// Rename only an app page or the client's temporary page for this choice.
/// Do not consume a custom-page slot or select a single global page for both picks.
pub fn swiftplay_page_update(page: &Value, name: &str) -> Result<Value> {
    let old_name = page["name"].as_str().unwrap_or_default();
    if page["isEditable"] != true || !(page["isTemporary"] == true || crate::brand::owns(old_name))
    {
        bail!(
            "Personal rune page preserved; select a recommended page for automatic Swiftplay runes"
        );
    }
    let mut target = page.clone();
    target["name"] = json!(name);
    Ok(target)
}

impl SwiftplayLobby {
    /// Only queue 480's player-slot contract has been verified with the client.
    /// An incomplete choice pauses preparation for the whole array.
    pub fn from_lobby(lobby: &Value) -> Result<Option<Self>> {
        if lobby.pointer("/gameConfig/queueId").and_then(Value::as_u64) != Some(480) {
            return Ok(None);
        }
        let raw_slots = lobby
            .pointer("/localMember/playerSlots")
            .context("Swiftplay player choices are unavailable")?;
        Ok(Some(Self {
            slots: parse_slots(raw_slots)?,
            raw_slots: raw_slots.clone(),
            party_id: lobby
                .get("partyId")
                .and_then(Value::as_str)
                .map(str::to_owned),
        }))
    }
}

fn positive_id(raw: &Value, field: &str) -> Result<u32> {
    raw.get(field)
        .and_then(Value::as_u64)
        .and_then(|id| u32::try_from(id).ok())
        .filter(|id| *id > 0)
        .with_context(|| format!("Swiftplay choice has no valid {field}"))
}

fn slot_perks(raw: &Value) -> Result<Value> {
    let perks: Value = serde_json::from_str(
        raw.get("perks")
            .and_then(Value::as_str)
            .context("Swiftplay choice perks must be a JSON string")?,
    )
    .context("Swiftplay choice perks are not valid JSON")?;
    if !perks.is_object() {
        bail!("Swiftplay choice perks must describe an object");
    }
    Ok(perks)
}

fn parse_slots(raw: &Value) -> Result<Vec<SwiftplaySlot>> {
    let slots = raw
        .as_array()
        .context("Swiftplay player choices must be an array")?;
    if !(1..=2).contains(&slots.len()) {
        bail!("Swiftplay needs one or two ready player choices");
    }
    slots
        .iter()
        .enumerate()
        .map(|(index, raw)| {
            let champion_id = positive_id(raw, "championId")?;
            let position = raw
                .get("positionPreference")
                .and_then(Value::as_str)
                .and_then(Position::parse)
                .context("Swiftplay choice role is not ready")?;
            let spell_ids = [positive_id(raw, "spell1")?, positive_id(raw, "spell2")?];
            slot_perks(raw)?;
            Ok(SwiftplaySlot {
                index,
                champion_id,
                position,
                spell_ids,
                raw: raw.clone(),
            })
        })
        .collect()
}

/// Produce the complete player-slot array, preserving every unrequested field.
/// The caller validates rune membership against the current patch catalog and
/// rechecks lobby identity/gameflow immediately before submitting the result.
pub fn prepare_slots(current: &Value, expected: &Value, updates: &[SlotUpdate]) -> Result<Value> {
    if !slots_match(current, expected) {
        bail!("Swiftplay choices changed during preparation; refresh before importing");
    }
    let slots = parse_slots(current)?;
    let mut touched = std::collections::HashSet::new();
    for update in updates {
        if update.index >= slots.len() || !touched.insert(update.index) {
            bail!("Swiftplay choice update index is unavailable or repeated");
        }
        if let Some(page) = &update.rune_page {
            const STYLES: [u32; 5] = [8000, 8100, 8200, 8300, 8400];
            if page.perks.len() != 9
                || page.perks.contains(&0)
                || !STYLES.contains(&page.primary_style)
                || !STYLES.contains(&page.sub_style)
                || page.primary_style == page.sub_style
            {
                bail!("Swiftplay rune page requires nine perks and two different valid styles");
            }
        }
        if let Some([a, b]) = update.spells {
            if a == b || spell_name(a).is_none() || spell_name(b).is_none() {
                bail!("Swiftplay choice requires two different known summoner spells");
            }
        }
    }
    let mut output = current.clone();
    for update in updates {
        let slot = slots
            .get(update.index)
            .context("Swiftplay choice index is unavailable")?;
        let raw = &mut output[update.index];
        if let Some(page) = &update.rune_page {
            let original_perks = slot_perks(raw)?;
            let mut perks = original_perks.clone();
            perks["perkIds"] = json!(page.perks);
            perks["perkStyle"] = json!(page.primary_style);
            perks["perkSubStyle"] = json!(page.sub_style);
            if perks != original_perks {
                raw["perks"] = Value::String(serde_json::to_string(&perks)?);
            }
        }
        if let Some(picked) = update.spells {
            let (spell1, spell2) =
                order_spells(&picked, Some((slot.spell_ids[0], slot.spell_ids[1])))
                    .context("Swiftplay choice requires two spells")?;
            raw["spell1"] = json!(spell1);
            raw["spell2"] = json!(spell2);
        }
    }
    Ok(output)
}

/// Compare complete arrays while allowing harmless formatting of embedded perks.
/// Malformed responses never confirm a successful import.
pub fn slots_match(left: &Value, right: &Value) -> bool {
    fn normalized(raw: &Value) -> Result<Value> {
        parse_slots(raw)?;
        let mut normalized = raw.clone();
        for slot in normalized
            .as_array_mut()
            .expect("parse_slots checked array")
        {
            slot["perks"] = slot_perks(slot)?;
        }
        Ok(normalized)
    }
    match (normalized(left), normalized(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linked_page_fixture() -> Value {
        json!({"id":7,"name":"Irelia - Conqueror","isTemporary":true,
            "isEditable":true,"current":false,"quickPlayChampionIds":[39],
            "primaryStyleId":8000,"subStyleId":8400,
            "selectedPerkIds":[8010,9111,9104,8299,8242,8473,5005,5008,5001],
            "isValid":true})
    }

    #[test]
    fn linked_runes_ignore_global_current_and_duplicate_recommendation_names() {
        let linked = linked_page_fixture();
        let mut recommended = linked.clone();
        recommended["id"] = json!(8);
        recommended["current"] = json!(true);
        recommended["quickPlayChampionIds"] = json!([]);
        recommended["subStyleId"] = json!(8300);
        let pages = json!([recommended, linked]);
        assert_eq!(linked_rune_page(&pages, 39).unwrap()["id"], 7);
        assert!(linked_rune_page(&pages, 498).is_err());
        assert!(linked_rune_page(&Value::Null, 39).is_err());
    }

    #[test]
    fn linked_runes_reject_duplicate_and_shared_associations() {
        let page = linked_page_fixture();
        assert!(linked_rune_page(&json!([page, page]), 39).is_err());
        let mut shared = page.clone();
        shared["quickPlayChampionIds"] = json!([39, 498]);
        assert!(linked_rune_page(&json!([shared]), 39).is_err());
    }

    #[test]
    fn named_swiftplay_page_keeps_temporary_and_global_selection_without_creating_pages() {
        let original = linked_page_fixture();
        let target = swiftplay_page_update(&original, "Recall Irelia Mid").unwrap();
        assert_eq!(target["name"], "Recall Irelia Mid");
        for key in [
            "id",
            "current",
            "isTemporary",
            "quickPlayChampionIds",
            "primaryStyleId",
            "subStyleId",
            "selectedPerkIds",
        ] {
            assert_eq!(target[key], original[key], "{key}");
        }
        assert_eq!(
            swiftplay_page_update(&target, "Recall Irelia Mid").unwrap(),
            target
        );
    }

    #[test]
    fn personal_and_noneditable_pages_are_never_renamed() {
        let mut page = linked_page_fixture();
        page["isTemporary"] = json!(false);
        page["name"] = json!("My Irelia");
        assert!(swiftplay_page_update(&page, "Recall Irelia Mid").is_err());
        page["name"] = json!("Recalling");
        assert!(swiftplay_page_update(&page, "Recall Irelia Mid").is_err());
        page["name"] = json!("Recall Irelia Mid");
        assert!(swiftplay_page_update(&page, "Recall Irelia Mid").is_ok());
        page["isEditable"] = json!(false);
        assert!(swiftplay_page_update(&page, "Recall Irelia Mid").is_err());
    }
    use serde_json::json;

    fn slots() -> Value {
        json!([
            {
                "championId": 498, "positionPreference": "BOTTOM", "skinId": 498007,
                "spell1": 4, "spell2": 21, "futureField": {"keep": [1, 2]},
                "perks": "{\"perkIds\":[8008,8009,9103,8014,8304,8345,5005,5008,5001],\"perkStyle\":8000,\"perkSubStyle\":8300,\"unknown\":true}"
            },
            {
                "championId": 39, "positionPreference": "MIDDLE", "skinId": 39000,
                "spell1": 14, "spell2": 4,
                "perks": "{\"perkIds\":[8010,9111,9104,8299,8304,8345,5005,5008,5001],\"perkStyle\":8000,\"perkSubStyle\":8300}"
            }
        ])
    }

    fn lobby(slots: Value) -> Value {
        json!({"gameConfig": {"queueId": 480}, "partyId": "test-party", "localMember": {"playerSlots": slots}})
    }

    fn page() -> RunePageIds {
        RunePageIds {
            primary_style: 8000,
            sub_style: 8400,
            perks: vec![8010, 9111, 9104, 8299, 8444, 8451, 5005, 5008, 5001],
            ..Default::default()
        }
    }

    #[test]
    fn reads_both_champion_and_role_choices_without_collapsing_them() {
        let result = SwiftplayLobby::from_lobby(&lobby(slots()))
            .unwrap()
            .unwrap();
        assert_eq!(result.slots.len(), 2);
        assert_eq!(
            (
                result.slots[0].index,
                result.slots[0].champion_id,
                result.slots[0].position
            ),
            (0, 498, Position::Adc)
        );
        assert_eq!(
            (
                result.slots[1].index,
                result.slots[1].champion_id,
                result.slots[1].position
            ),
            (1, 39, Position::Mid)
        );
        assert_eq!(result.slots[1].spell_ids, [14, 4]);
        assert_eq!(result.slots[0].raw["skinId"], 498007);
        assert_eq!(result.raw_slots, slots());
        assert_eq!(result.party_id.as_deref(), Some("test-party"));
    }

    #[test]
    fn one_ready_choice_and_same_champion_in_different_roles_are_supported() {
        let one = json!([slots()[0]]);
        assert_eq!(
            SwiftplayLobby::from_lobby(&lobby(one))
                .unwrap()
                .unwrap()
                .slots
                .len(),
            1
        );
        let mut same_champion = slots();
        same_champion[1]["championId"] = json!(498);
        let result = SwiftplayLobby::from_lobby(&lobby(same_champion))
            .unwrap()
            .unwrap();
        assert_eq!(result.slots.len(), 2);
        assert_eq!(result.slots[1].champion_id, 498);
        assert_eq!(result.slots[1].position, Position::Mid);
    }

    #[test]
    fn other_modes_are_ignored_and_unready_swiftplay_choices_are_rejected() {
        for queue in [0, 400, 420, 440, 490, 900] {
            let mut other = lobby(slots());
            other["gameConfig"]["queueId"] = json!(queue);
            assert!(SwiftplayLobby::from_lobby(&other).unwrap().is_none());
        }
        assert!(SwiftplayLobby::from_lobby(&Value::Null).unwrap().is_none());
        for malformed in [
            Value::Null,
            json!([]),
            json!([slots()[0], slots()[1], slots()[0]]),
        ] {
            assert!(SwiftplayLobby::from_lobby(&lobby(malformed)).is_err());
        }
        for (field, value) in [
            ("championId", json!(0)),
            ("championId", json!(4294967296_u64)),
            ("positionPreference", json!("FILL")),
            ("spell1", json!(0)),
            ("spell2", Value::Null),
            ("perks", json!("not-json")),
            ("perks", json!("[]")),
        ] {
            let mut malformed = slots();
            malformed[1][field] = value;
            assert!(
                SwiftplayLobby::from_lobby(&lobby(malformed)).is_err(),
                "accepted malformed {field}"
            );
        }
    }

    #[test]
    fn preparing_one_choice_preserves_skin_identity_unknown_fields_and_the_other_choice() {
        let current = slots();
        let updated = prepare_slots(
            &current,
            &current,
            &[SlotUpdate {
                index: 0,
                rune_page: Some(page()),
                spells: Some([7, 4]),
            }],
        )
        .unwrap();
        assert_eq!(updated[1], current[1]);
        assert_eq!(updated[0]["championId"], 498);
        assert_eq!(updated[0]["positionPreference"], "BOTTOM");
        assert_eq!(updated[0]["skinId"], 498007);
        assert_eq!(updated[0]["futureField"], json!({"keep": [1, 2]}));
        assert_eq!(
            (updated[0]["spell1"].as_u64(), updated[0]["spell2"].as_u64()),
            (Some(4), Some(7))
        );
        let perks: Value = serde_json::from_str(updated[0]["perks"].as_str().unwrap()).unwrap();
        assert_eq!(
            perks,
            json!({"perkIds": [8010,9111,9104,8299,8444,8451,5005,5008,5001], "perkStyle":8000,"perkSubStyle":8400,"unknown":true})
        );
    }

    #[test]
    fn both_choices_keep_flash_on_their_individual_keys() {
        let current = slots();
        let updated = prepare_slots(
            &current,
            &current,
            &[
                SlotUpdate {
                    index: 0,
                    rune_page: None,
                    spells: Some([7, 4]),
                },
                SlotUpdate {
                    index: 1,
                    rune_page: None,
                    spells: Some([4, 12]),
                },
            ],
        )
        .unwrap();
        assert_eq!(updated[0]["spell1"], 4);
        assert_eq!(updated[0]["spell2"], 7);
        assert_eq!(updated[1]["spell1"], 12);
        assert_eq!(updated[1]["spell2"], 4);
        assert_eq!(updated[0]["perks"], current[0]["perks"]);
        assert_eq!(updated[1]["perks"], current[1]["perks"]);
    }

    #[test]
    fn stale_snapshot_cannot_overwrite_manual_edits_to_either_choice() {
        let expected = slots();
        for (index, field, value) in [
            (0, "championId", json!(22)),
            (0, "positionPreference", json!("UTILITY")),
            (0, "spell2", json!(7)),
            (1, "skinId", json!(39001)),
            (1, "futureField", json!(true)),
            (
                1,
                "perks",
                json!("{\"perkIds\":[],\"perkStyle\":8000,\"perkSubStyle\":8300}"),
            ),
        ] {
            let mut current = expected.clone();
            current[index][field] = value;
            assert!(
                prepare_slots(
                    &current,
                    &expected,
                    &[SlotUpdate {
                        index: 0,
                        rune_page: Some(page()),
                        spells: None
                    }]
                )
                .is_err(),
                "overwrote {index}.{field}"
            );
        }
    }

    #[test]
    fn semantic_confirmation_avoids_reimports_from_perk_json_formatting() {
        let current = slots();
        let updates = [SlotUpdate {
            index: 0,
            rune_page: Some(page()),
            spells: Some([7, 4]),
        }];
        let prepared = prepare_slots(&current, &current, &updates).unwrap();
        let mut confirmed = prepared.clone();
        let perks: Value = serde_json::from_str(confirmed[0]["perks"].as_str().unwrap()).unwrap();
        confirmed[0]["perks"] = json!(serde_json::to_string_pretty(&perks).unwrap());
        assert!(slots_match(&prepared, &confirmed));
        assert_eq!(
            prepare_slots(&confirmed, &prepared, &updates).unwrap(),
            confirmed
        );
        confirmed[1]["spell1"] = json!(12);
        assert!(!slots_match(&prepared, &confirmed));
        assert!(!slots_match(&Value::Null, &Value::Null));
    }

    #[test]
    fn invalid_updates_cannot_produce_a_partial_or_incomplete_loadout() {
        let current = slots();
        let mut short = page();
        short.perks.pop();
        let mut same_style = page();
        same_style.sub_style = same_style.primary_style;
        let mut unknown_style = page();
        unknown_style.primary_style = 9999;
        let mut zero_perk = page();
        zero_perk.perks[0] = 0;
        for update in [
            SlotUpdate {
                index: 2,
                rune_page: Some(page()),
                spells: None,
            },
            SlotUpdate {
                index: 0,
                rune_page: Some(short),
                spells: None,
            },
            SlotUpdate {
                index: 0,
                rune_page: Some(same_style),
                spells: None,
            },
            SlotUpdate {
                index: 0,
                rune_page: Some(unknown_style),
                spells: None,
            },
            SlotUpdate {
                index: 0,
                rune_page: Some(zero_perk),
                spells: None,
            },
            SlotUpdate {
                index: 0,
                rune_page: None,
                spells: Some([4, 4]),
            },
            SlotUpdate {
                index: 0,
                rune_page: None,
                spells: Some([4, 0]),
            },
            SlotUpdate {
                index: 0,
                rune_page: None,
                spells: Some([4, 9999]),
            },
        ] {
            assert!(prepare_slots(&current, &current, &[update]).is_err());
        }
        let duplicate = SlotUpdate {
            index: 0,
            rune_page: Some(page()),
            spells: None,
        };
        assert!(prepare_slots(&current, &current, &[duplicate.clone(), duplicate]).is_err());
    }
}
