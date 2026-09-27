//! Rune pages and summoner spells: pack names or aggregate ids -> LCU perk page / spell selection.
use crate::aggregate::RunePageIds;
use crate::ddragon::{normalize, Catalog};
use crate::lcu::Lcu;
use crate::pack::RunePage;
use anyhow::{bail, Result};
use serde_json::{json, Value};

/// Stat shards are not in Data Dragon's runesReforged.json.
pub fn shard_id(name: &str) -> Option<u32> {
    Some(match normalize(name).as_str() {
        "adaptiveforce" | "adaptive" => 5008,
        "attackspeed" => 5005,
        "abilityhaste" | "haste" => 5007,
        "movespeed" | "movementspeed" => 5010,
        "healthscaling" | "scalinghealth" => 5001,
        "health" | "flathealth" => 5011,
        "tenacity" | "tenacityandslowresist" => 5013,
        _ => return None,
    })
}

pub fn shard_name(id: u32) -> Option<&'static str> {
    Some(match id {
        5008 => "Adaptive Force",
        5005 => "Attack Speed",
        5007 => "Ability Haste",
        5010 => "Move Speed",
        5001 => "Health Scaling",
        5011 => "Health",
        5013 => "Tenacity and Slow Resist",
        _ => return None,
    })
}

pub const SUMMONER_SPELL_IDS: &[(&str, u64)] = &[
    ("Cleanse", 1),
    ("Exhaust", 3),
    ("Flash", 4),
    ("Ghost", 6),
    ("Heal", 7),
    ("Smite", 11),
    ("Teleport", 12),
    ("Clarity", 13),
    ("Ignite", 14),
    ("Barrier", 21),
    ("To the King!", 30),
    ("Poro Toss", 31),
    ("Mark", 32),
];
pub const FLASH: u32 = 4;

pub fn spell_id(name: &str) -> Option<u64> {
    let key = normalize(name);
    SUMMONER_SPELL_IDS
        .iter()
        .find(|(n, _)| normalize(n) == key)
        .map(|(_, id)| *id)
}

pub fn spell_name(id: u32) -> Option<&'static str> {
    SUMMONER_SPELL_IDS
        .iter()
        .find(|(_, i)| *i == id as u64)
        .map(|(n, _)| *n)
}

/// The two spells to set, keeping Flash on the key the player has it on now (D or F).
pub fn order_spells(picked: &[u32], current: Option<(u32, u32)>) -> Option<(u32, u32)> {
    if picked.len() != 2 {
        return None;
    }
    let (a, b) = (picked[0], picked[1]);
    match current {
        Some((_, f)) if f == FLASH && a == FLASH => Some((b, a)),
        Some((d, _)) if d == FLASH && b == FLASH => Some((b, a)),
        _ => Some((a, b)),
    }
}

/// `{name, primaryStyleId, subStyleId, selectedPerkIds, current}` from resolved ids.
pub fn page_value(page: &RunePageIds, name: &str) -> Value {
    json!({
        "name": name,
        "primaryStyleId": page.primary_style,
        "subStyleId": page.sub_style,
        "selectedPerkIds": page.perks,
        "current": true
    })
}

/// Resolve a pack page (names) to ids: keystone, 3 primary, 2 secondary, 3 shards.
pub fn page_ids(page: &RunePage, cat: &Catalog) -> Result<RunePageIds> {
    let mut missing: Vec<String> = Vec::new();
    let mut perks: Vec<u32> = Vec::new();
    let mut rune = |n: &str| match cat.rune_id(n) {
        Some(id) => perks.push(id),
        None => missing.push(n.to_string()),
    };
    rune(&page.keystone);
    for p in &page.primary_perks {
        rune(p);
    }
    for p in &page.secondary_perks {
        rune(p);
    }
    for s in &page.shards {
        match shard_id(s) {
            Some(id) => perks.push(id),
            None => missing.push(s.to_string()),
        }
    }
    let primary = cat.style_id(&page.primary);
    let secondary = cat.style_id(&page.secondary);
    if primary.is_none() {
        missing.push(page.primary.clone());
    }
    if secondary.is_none() {
        missing.push(page.secondary.clone());
    }
    if !missing.is_empty() {
        bail!("unknown rune names for this patch: {}", missing.join(", "));
    }
    if perks.len() != 9 {
        bail!(
            "a rune page needs 9 perks (keystone + 3 + 2 + 3 shards), got {}",
            perks.len()
        );
    }
    Ok(RunePageIds {
        primary_style: primary.unwrap(),
        sub_style: secondary.unwrap(),
        perks,
        ..Default::default()
    })
}

/// Pack page (names) -> LCU perk page value.
pub fn build_page(page: &RunePage, cat: &Catalog, name: &str) -> Result<Value> {
    Ok(page_value(&page_ids(page, cat)?, name))
}

/// One of our pages that champion select may overwrite: named by us, editable, and not the
/// client's temporary Swiftplay page. `linked` means the client tied it to a Swiftplay pick: it
/// then refuses to delete it but still counts it against the player's page limit.
struct OwnPage<'a> {
    page: &'a Value,
    id: u64,
    exact: bool,
    linked: bool,
    current: bool,
}

fn flag(page: &Value, key: &str) -> Option<bool> {
    page.get(key).and_then(Value::as_bool)
}

fn own_pages<'a>(pages: &'a Value, name: &str) -> Vec<OwnPage<'a>> {
    let legacy = crate::brand::legacy_name(name);
    pages
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|page| {
            let page_name = page.get("name")?.as_str()?;
            let editable =
                flag(page, "isEditable").unwrap_or(flag(page, "isDeletable") == Some(true));
            if !editable
                || flag(page, "isTemporary") == Some(true)
                || !crate::brand::owns(page_name)
            {
                return None;
            }
            let quick_play = page
                .get("quickPlayChampionIds")
                .and_then(Value::as_array)
                .is_some_and(|ids| !ids.is_empty());
            Some(OwnPage {
                page,
                id: page.get("id")?.as_u64().filter(|id| *id > 0)?,
                exact: page_name == name || legacy.as_deref() == Some(page_name),
                linked: quick_play || flag(page, "isDeletable") != Some(true),
                current: flag(page, "current") == Some(true),
            })
        })
        .collect()
}

/// Our page for this loadout (a free one before one tied to Swiftplay), else our free page:
/// the current one, then the lowest id. Pages tied to a Swiftplay pick are only borrowed for
/// another loadout when no custom-page slot is left (see [`import`]).
fn reusable_page<'a>(pages: &'a Value, name: &str) -> Option<OwnPage<'a>> {
    own_pages(pages, name)
        .into_iter()
        .filter(|own| own.exact || !own.linked)
        .min_by_key(|own| (!own.exact, own.linked, !own.current, own.id))
}

fn linked_page<'a>(pages: &'a Value, name: &str) -> Option<OwnPage<'a>> {
    own_pages(pages, name)
        .into_iter()
        .filter(|own| own.linked)
        .min_by_key(|own| (!own.current, own.id))
}

/// The page champion select would update in place before considering a new page.
pub fn replacement_page_id(pages: &Value, name: &str) -> Option<u64> {
    reusable_page(pages, name).map(|own| own.id)
}

/// Pages the client counts against the custom-page limit: every non-temporary editable page.
fn counts_against_limit(page: &Value) -> bool {
    flag(page, "isTemporary") != Some(true)
        && (flag(page, "isDeletable") == Some(true) || flag(page, "isEditable") == Some(true))
}

/// Whether the client accepts one more custom page. `canAddCustomPage` is its own answer; the
/// counts are a fallback for clients without it. Counting deletable pages alone is wrong: a page
/// tied to a Swiftplay pick is not deletable yet still takes a slot ("Max pages reached").
pub fn can_create_page(pages: &Value, inventory: Option<&Value>) -> bool {
    let field = |key: &str| inventory.and_then(|inventory| inventory.get(key));
    if let Some(allowed) = field("canAddCustomPage").and_then(Value::as_bool) {
        return allowed;
    }
    let owned = field("ownedPageCount").and_then(Value::as_u64).unwrap_or(2);
    let used = field("customPageCount")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| {
            pages.as_array().map_or(0, |all| {
                all.iter().filter(|page| counts_against_limit(page)).count() as u64
            })
        });
    used < owned
}

/// The PUT body for reusing `existing`: the new loadout, with the Swiftplay link and temporary flag
/// kept exactly as the client reported them, so borrowing a page never detaches a pick from it.
fn update_payload(existing: &Value, page: &Value) -> Value {
    let mut payload = page.clone();
    if let Some(object) = payload.as_object_mut() {
        for key in ["isTemporary", "quickPlayChampionIds"] {
            if let Some(value) = existing.get(key) {
                object.insert(key.into(), value.clone());
            }
        }
    }
    payload
}

fn no_free_page(pages: &Value, inventory: Option<&Value>) -> String {
    let personal: Vec<&str> = pages
        .as_array()
        .into_iter()
        .flatten()
        .filter(|page| counts_against_limit(page))
        .filter_map(|page| page.get("name")?.as_str())
        .filter(|name| !crate::brand::owns(name))
        .collect();
    let owned = inventory
        .and_then(|inventory| inventory.get("ownedPageCount"))
        .and_then(Value::as_u64)
        .map_or_else(|| "rune page".to_string(), |n| format!("{n} rune page"));
    if personal.is_empty() {
        return format!("No free rune page and none of Recall's pages can be reused; free one of your {owned} slots in the client");
    }
    format!(
        "Your {owned} slots hold your own pages ({}); delete one in the client and Recall will use that slot (it never deletes your pages)",
        personal.join(", ")
    )
}

/// Put `page` (from [`page_value`]) into the client for champion select, in this order: update our
/// page for this loadout or our free page in place; create a page while a custom-page slot is free;
/// otherwise borrow our page that the client tied to a Swiftplay pick (the next Swiftplay
/// preparation rewrites it for that pick). Personal pages are never written and no page is ever
/// deleted. `check` runs right before each write, so a caller can stop an import whose match or
/// loadout changed while the pages were being read.
pub async fn import(
    lcu: &Lcu,
    page: Value,
    mut check: impl FnMut() -> std::result::Result<(), String>,
) -> Result<String> {
    let name = page
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(crate::brand::NAME)
        .to_string();
    let pages = lcu.perk_pages().await?;
    if !pages.is_array() {
        bail!("client rune-page list is unavailable")
    }
    if let Some(own) = reusable_page(&pages, &name) {
        check().map_err(anyhow::Error::msg)?;
        lcu.update_perk_page(own.id, &update_payload(own.page, &page))
            .await?;
        return Ok(name);
    }
    let inventory = lcu.perk_inventory().await.ok();
    if can_create_page(&pages, inventory.as_ref()) {
        check().map_err(anyhow::Error::msg)?;
        match lcu.create_perk_page(&page).await {
            Ok(_) => return Ok(name),
            // The inventory can lag behind the page list: with no slot after all, borrow below.
            Err(error)
                if error.to_string().contains("Max pages")
                    && linked_page(&pages, &name).is_some() => {}
            Err(error) => return Err(error),
        }
    }
    let Some(own) = linked_page(&pages, &name) else {
        bail!("{}", no_free_page(&pages, inventory.as_ref()));
    };
    check().map_err(anyhow::Error::msg)?;
    lcu.update_perk_page(own.id, &update_payload(own.page, &page))
        .await?;
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_from_the_previous_name_is_reused_and_a_personal_page_never_is() {
        let pages = json!([
            {"id": 4, "name": "Featherstorm Ahri MID", "isDeletable": true},
            {"id": 9, "name": "Featherstorming", "isDeletable": true},
            {"id": 2, "name": "My main page", "isDeletable": true}
        ]);
        assert_eq!(replacement_page_id(&pages, "Recall Ahri MID"), Some(4));
        let personal = json!([{"id": 2, "name": "My main page", "isDeletable": true}]);
        assert_eq!(replacement_page_id(&personal, "Recall Ahri MID"), None);
    }

    #[test]
    fn replacement_reuses_one_owned_page_without_deleting_unrelated_pages() {
        let pages = json!([
            {"id": 10, "name": "Recalling", "isDeletable": true, "current": true},
            {"id": 7, "name": "Recall Ahri MID", "isDeletable": true},
            {"id": 3, "name": "Recall Lulu SUPPORT", "isDeletable": true, "current": true},
            {"id": 1, "name": "Recall Xayah ADC", "isDeletable": false}
        ]);
        assert_eq!(replacement_page_id(&pages, "Recall Ahri MID"), Some(7));
        assert_eq!(replacement_page_id(&pages, "Recall Ornn TOP"), Some(3));
        assert_eq!(replacement_page_id(&pages, "Recall Xayah ADC"), Some(3));
        assert_eq!(
            replacement_page_id(
                &json!([
                    {"id": 10, "name": "Recalling", "isDeletable": true},
                    {"id": 11, "name": "My personal page", "isDeletable": true}
                ]),
                "Recall Ahri MID"
            ),
            None
        );
        assert_eq!(replacement_page_id(&json!([]), "Recall Ahri MID"), None);
        assert_eq!(replacement_page_id(&Value::Null, "Recall Ahri MID"), None);
    }

    #[test]
    fn replacement_is_independent_of_page_response_order() {
        let pages = json!([
            {"id": 6, "name": "Recall Xayah", "isDeletable": true},
            {"id": 2, "name": "Recall Ahri", "isDeletable": true},
            {"name": "Recall Lulu", "isDeletable": true}
        ]);
        let mut reversed = pages.clone();
        reversed.as_array_mut().unwrap().reverse();
        assert_eq!(replacement_page_id(&pages, "Recall Ornn"), Some(2));
        assert_eq!(replacement_page_id(&reversed, "Recall Ornn"), Some(2));
    }

    // A local disposable HTTP server: these tests never read a lockfile or contact League.
    fn mock_client(
        responses: Vec<(u16, Value)>,
    ) -> (Lcu, std::thread::JoinHandle<Vec<(String, Value)>>) {
        use std::io::{BufRead, Read, Write};
        use std::net::TcpListener;
        use std::time::{Duration, Instant};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let thread = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, response) in responses {
                let until = Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error)
                            if error.kind() == std::io::ErrorKind::WouldBlock
                                && Instant::now() < until =>
                        {
                            std::thread::sleep(Duration::from_millis(5))
                        }
                        Err(error) => panic!("mock request missing: {error}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut reader = std::io::BufReader::new(&stream);
                let mut first = String::new();
                reader.read_line(&mut first).unwrap();
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line.trim().is_empty() {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse::<usize>().unwrap();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                requests.push((
                    first.trim().to_string(),
                    serde_json::from_slice(&body).unwrap_or(Value::Null),
                ));
                let body = response.to_string();
                write!(stream, "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            requests
        });
        let client = Lcu::from_lockfile(crate::lcu::Lockfile {
            process: "test".into(),
            pid: 0,
            port,
            password: "test-only".into(),
            protocol: "http".into(),
        })
        .unwrap();
        (client, thread)
    }

    #[tokio::test]
    async fn refresh_updates_in_place_and_never_deletes_the_old_page() {
        let pages = json!([
            {"id": 7, "name": "Recall Xayah ADC", "isDeletable": true},
            {"id": 8, "name": "My personal page", "isDeletable": true}
        ]);
        let (client, server) = mock_client(vec![(200, pages), (200, json!({"id": 7}))]);
        let page = page_value(&RunePageIds::default(), "Recall Ahri MID");
        assert_eq!(
            import(&client, page, || Ok(())).await.unwrap(),
            "Recall Ahri MID"
        );
        let requests = server.join().unwrap();
        assert_eq!(requests[0].0, "GET /lol-perks/v1/pages HTTP/1.1");
        assert_eq!(requests[1].0, "PUT /lol-perks/v1/pages/7 HTTP/1.1");
        assert_eq!(requests[1].1["id"], 7);
        assert_eq!(requests[1].1["name"], "Recall Ahri MID");
        assert_eq!(requests[1].1["current"], true);
    }

    #[tokio::test]
    async fn failed_refresh_does_not_delete_or_create_any_page() {
        let (client, server) = mock_client(vec![
            (
                200,
                json!([{"id": 7, "name": "Recall Xayah ADC", "isDeletable": true}]),
            ),
            (500, json!({"message": "test failure"})),
        ]);
        assert!(import(
            &client,
            page_value(&RunePageIds::default(), "Recall Ahri MID"),
            || Ok(())
        )
        .await
        .is_err());
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1].0, "PUT /lol-perks/v1/pages/7 HTTP/1.1");
    }

    /// The account as the client reported it during the failed draft (2026-09-26): two custom-page
    /// slots, both taken, one by a leftover personal page and one by our page that the client tied
    /// to Xayah's Swiftplay pick, plus the client's temporary page for the other Swiftplay pick.
    fn full_account() -> (Value, Value) {
        (
            json!([
                {"id": 1119371772u64, "name": "Recall Yasuo Top", "isDeletable": false,
                 "isEditable": true, "isTemporary": true, "current": false,
                 "quickPlayChampionIds": [157]},
                {"id": 732678925u64, "name": "OP.GG adc Xayah", "isDeletable": true,
                 "isEditable": true, "isTemporary": false, "current": false,
                 "quickPlayChampionIds": []},
                {"id": 324074296u64, "name": "Recall Xayah ADC", "isDeletable": false,
                 "isEditable": true, "isTemporary": false, "current": true,
                 "quickPlayChampionIds": [498]}
            ]),
            json!({"canAddCustomPage": false, "customPageCount": 2,
                   "isCustomPageCreationUnlocked": true, "ownedPageCount": 2}),
        )
    }

    #[tokio::test]
    async fn draft_with_every_slot_taken_borrows_our_swiftplay_page_and_keeps_its_link() {
        let (pages, inventory) = full_account();
        let (client, server) = mock_client(vec![(200, pages), (200, inventory), (201, json!({}))]);
        let page = page_value(&RunePageIds::default(), "Recall Malphite Top");
        assert_eq!(
            import(&client, page, || Ok(())).await.unwrap(),
            "Recall Malphite Top"
        );
        let requests = server.join().unwrap();
        assert_eq!(requests[0].0, "GET /lol-perks/v1/pages HTTP/1.1");
        assert_eq!(requests[1].0, "GET /lol-perks/v1/inventory HTTP/1.1");
        assert_eq!(requests[2].0, "PUT /lol-perks/v1/pages/324074296 HTTP/1.1");
        let body = &requests[2].1;
        assert_eq!(body["id"], 324074296u64);
        assert_eq!(body["name"], "Recall Malphite Top");
        assert_eq!(body["current"], true);
        assert_eq!(body["quickPlayChampionIds"], json!([498]));
        assert_eq!(body["isTemporary"], false);
    }

    #[tokio::test]
    async fn a_free_slot_gets_a_new_page_and_the_swiftplay_page_is_left_alone() {
        let (client, server) = mock_client(vec![
            (
                200,
                json!([{"id": 5, "name": "Recall Xayah ADC", "isDeletable": false,
                        "isEditable": true, "isTemporary": false,
                        "quickPlayChampionIds": [498]}]),
            ),
            (
                200,
                json!({"canAddCustomPage": true, "customPageCount": 1, "ownedPageCount": 2}),
            ),
            (200, json!({"id": 6})),
        ]);
        let page = page_value(&RunePageIds::default(), "Recall Malphite Top");
        import(&client, page, || Ok(())).await.unwrap();
        let requests = server.join().unwrap();
        assert_eq!(requests[2].0, "POST /lol-perks/v1/pages HTTP/1.1");
        assert_eq!(requests[2].1["name"], "Recall Malphite Top");
    }

    #[tokio::test]
    async fn the_same_loadout_updates_its_swiftplay_page_instead_of_adding_a_duplicate() {
        let (client, server) = mock_client(vec![
            (
                200,
                json!([{"id": 5, "name": "Recall Xayah ADC", "isDeletable": false,
                        "isEditable": true, "isTemporary": false,
                        "quickPlayChampionIds": [498]}]),
            ),
            (201, json!({})),
        ]);
        let page = page_value(&RunePageIds::default(), "Recall Xayah ADC");
        import(&client, page, || Ok(())).await.unwrap();
        let requests = server.join().unwrap();
        assert_eq!(requests[1].0, "PUT /lol-perks/v1/pages/5 HTTP/1.1");
        assert_eq!(requests[1].1["quickPlayChampionIds"], json!([498]));
    }

    #[tokio::test]
    async fn a_stale_inventory_that_hits_max_pages_falls_back_to_our_swiftplay_page() {
        let (pages, _) = full_account();
        let (client, server) = mock_client(vec![
            (200, pages),
            (200, json!({"canAddCustomPage": true})),
            (
                400,
                json!({"errorCode": "RPC_ERROR", "httpStatus": 400, "message": "Max pages reached"}),
            ),
            (201, json!({})),
        ]);
        let page = page_value(&RunePageIds::default(), "Recall Malphite Top");
        import(&client, page, || Ok(())).await.unwrap();
        let requests = server.join().unwrap();
        assert_eq!(requests[2].0, "POST /lol-perks/v1/pages HTTP/1.1");
        assert_eq!(requests[3].0, "PUT /lol-perks/v1/pages/324074296 HTTP/1.1");
    }

    #[tokio::test]
    async fn only_personal_pages_is_an_error_naming_them_and_nothing_is_written() {
        let (client, server) = mock_client(vec![
            (
                200,
                json!([
                    {"id": 1, "name": "OP.GG adc Xayah", "isDeletable": true, "isEditable": true},
                    {"id": 2, "name": "My page", "isDeletable": true, "isEditable": true},
                    {"id": 3, "name": "Recall Yasuo Top", "isDeletable": false,
                     "isEditable": true, "isTemporary": true, "quickPlayChampionIds": [157]}
                ]),
            ),
            (200, json!({"canAddCustomPage": false, "ownedPageCount": 2})),
        ]);
        let page = page_value(&RunePageIds::default(), "Recall Malphite Top");
        let error = import(&client, page, || Ok(()))
            .await
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("OP.GG adc Xayah") && error.contains("My page"),
            "{error}"
        );
        assert!(error.contains("2 rune page slots"), "{error}");
        assert_eq!(server.join().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_failed_check_stops_the_write() {
        let (client, server) = mock_client(vec![(
            200,
            json!([{"id": 7, "name": "Recall Xayah ADC", "isDeletable": true}]),
        )]);
        let page = page_value(&RunePageIds::default(), "Recall Ahri MID");
        let error = import(&client, page, || Err("loadout changed".into()))
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "loadout changed");
        assert_eq!(server.join().unwrap().len(), 1);
    }

    #[test]
    fn swiftplay_pages_are_reused_for_their_own_loadout_only_until_slots_run_out() {
        let pages = json!([
            {"id": 7, "name": "Recall Ahri MID", "isDeletable": true},
            {"id": 5, "name": "Recall Xayah ADC", "isDeletable": false, "isEditable": true,
             "quickPlayChampionIds": [498]},
            {"id": 3, "name": "Recall Irelia Mid", "isDeletable": false, "isEditable": true,
             "isTemporary": true, "quickPlayChampionIds": [39]}
        ]);
        assert_eq!(replacement_page_id(&pages, "Recall Xayah ADC"), Some(5));
        assert_eq!(replacement_page_id(&pages, "Recall Ornn TOP"), Some(7));
        // The client's temporary Swiftplay page is never taken, even for its own name.
        assert_eq!(replacement_page_id(&pages, "Recall Irelia Mid"), Some(7));
        assert_eq!(
            linked_page(&pages, "Recall Ornn TOP").map(|own| own.id),
            Some(5)
        );
    }

    #[test]
    fn free_slots_come_from_the_client_and_count_swiftplay_pages() {
        let (pages, inventory) = full_account();
        assert!(!can_create_page(&pages, Some(&inventory)));
        // Without canAddCustomPage the counts decide; without counts, the page list does, and the
        // non-deletable Swiftplay page still takes a slot.
        assert!(!can_create_page(
            &pages,
            Some(&json!({"ownedPageCount": 2}))
        ));
        assert!(can_create_page(&pages, Some(&json!({"ownedPageCount": 3}))));
        assert!(!can_create_page(&pages, None));
        assert!(can_create_page(
            &json!([{"id": 1, "name": "Mine", "isDeletable": true, "isEditable": true}]),
            None
        ));
        assert!(can_create_page(
            &pages,
            Some(&json!({"canAddCustomPage": true, "customPageCount": 2, "ownedPageCount": 2}))
        ));
    }

    #[test]
    fn shards_and_spells() {
        assert_eq!(shard_id("Attack Speed"), Some(5005));
        assert_eq!(shard_id("Adaptive Force"), Some(5008));
        assert_eq!(shard_id("Health"), Some(5011));
        assert_eq!(shard_id("nope"), None);
        assert_eq!(spell_id("Flash"), Some(4));
        assert_eq!(spell_name(21), Some("Barrier"));
        assert_eq!(spell_name(99), None);
        // Flash stays on the player's key.
        assert_eq!(order_spells(&[4, 21], Some((7, 4))), Some((21, 4)));
        assert_eq!(order_spells(&[21, 4], Some((4, 7))), Some((4, 21)));
        assert_eq!(order_spells(&[4, 21], Some((4, 7))), Some((4, 21)));
        assert_eq!(order_spells(&[4, 21], None), Some((4, 21)));
        assert_eq!(order_spells(&[4], None), None);
        let page = page_value(
            &RunePageIds {
                primary_style: 8000,
                sub_style: 8300,
                perks: vec![8008, 8009, 9103, 8014, 8304, 8345, 5005, 5008, 5001],
                ..Default::default()
            },
            "Recall Xayah",
        );
        assert_eq!(page["primaryStyleId"], 8000);
        assert_eq!(page["selectedPerkIds"].as_array().unwrap().len(), 9);
        assert_eq!(spell_id("heal"), Some(7));
    }
}
