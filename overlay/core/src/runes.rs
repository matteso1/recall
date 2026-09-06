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

/// Reuse exactly one editable app page, preferring this loadout, then the current
/// app page, then the lowest id. Never delete pages to free space for an import.
pub fn replacement_page_id(pages: &Value, name: &str) -> Option<u64> {
    pages
        .as_array()?
        .iter()
        .filter_map(|page| {
            let page_name = page.get("name")?.as_str()?;
            if page.get("isDeletable").and_then(Value::as_bool) != Some(true)
                || !(page_name == "Featherstorm" || page_name.starts_with("Featherstorm "))
            {
                return None;
            }
            Some((
                page_name != name,
                page.get("current").and_then(Value::as_bool) != Some(true),
                page.get("id")?.as_u64()?,
            ))
        })
        .min()
        .map(|(_, _, id)| id)
}

/// Update an existing app page in place, or create a page without deleting any.
pub async fn import(lcu: &Lcu, page: Value) -> Result<String> {
    let name = page
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Featherstorm")
        .to_string();
    let pages = lcu.perk_pages().await?;
    let Some(all_pages) = pages.as_array() else {
        bail!("client rune-page list is unavailable")
    };
    if let Some(id) = replacement_page_id(&pages, &name) {
        lcu.update_perk_page(id, &page).await?;
        return Ok(name);
    }
    let deletable = all_pages
        .iter()
        .filter(|page| page.get("isDeletable").and_then(Value::as_bool) == Some(true))
        .count();
    let owned = lcu
        .perk_inventory()
        .await
        .ok()
        .and_then(|v| v.get("ownedPageCount").and_then(Value::as_u64))
        .unwrap_or(2) as usize;
    if deletable >= owned {
        bail!("all {owned} rune pages are in use; delete one in the client and retry");
    }
    lcu.create_perk_page(&page).await?;
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_reuses_one_owned_page_without_deleting_unrelated_pages() {
        let pages = json!([
            {"id": 10, "name": "Featherstorming", "isDeletable": true, "current": true},
            {"id": 7, "name": "Featherstorm Ahri MID", "isDeletable": true},
            {"id": 3, "name": "Featherstorm Lulu SUPPORT", "isDeletable": true, "current": true},
            {"id": 1, "name": "Featherstorm Xayah ADC", "isDeletable": false}
        ]);
        assert_eq!(
            replacement_page_id(&pages, "Featherstorm Ahri MID"),
            Some(7)
        );
        assert_eq!(
            replacement_page_id(&pages, "Featherstorm Ornn TOP"),
            Some(3)
        );
        assert_eq!(
            replacement_page_id(&pages, "Featherstorm Xayah ADC"),
            Some(3)
        );
        assert_eq!(
            replacement_page_id(
                &json!([
                    {"id": 10, "name": "Featherstorming", "isDeletable": true},
                    {"id": 11, "name": "My personal page", "isDeletable": true}
                ]),
                "Featherstorm Ahri MID"
            ),
            None
        );
        assert_eq!(
            replacement_page_id(&json!([]), "Featherstorm Ahri MID"),
            None
        );
        assert_eq!(
            replacement_page_id(&Value::Null, "Featherstorm Ahri MID"),
            None
        );
    }

    #[test]
    fn replacement_is_independent_of_page_response_order() {
        let pages = json!([
            {"id": 6, "name": "Featherstorm Xayah", "isDeletable": true},
            {"id": 2, "name": "Featherstorm Ahri", "isDeletable": true},
            {"name": "Featherstorm Lulu", "isDeletable": true}
        ]);
        let mut reversed = pages.clone();
        reversed.as_array_mut().unwrap().reverse();
        assert_eq!(replacement_page_id(&pages, "Featherstorm Ornn"), Some(2));
        assert_eq!(replacement_page_id(&reversed, "Featherstorm Ornn"), Some(2));
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
            {"id": 7, "name": "Featherstorm Xayah ADC", "isDeletable": true},
            {"id": 8, "name": "My personal page", "isDeletable": true}
        ]);
        let (client, server) = mock_client(vec![(200, pages), (200, json!({"id": 7}))]);
        let page = page_value(&RunePageIds::default(), "Featherstorm Ahri MID");
        assert_eq!(
            import(&client, page).await.unwrap(),
            "Featherstorm Ahri MID"
        );
        let requests = server.join().unwrap();
        assert_eq!(requests[0].0, "GET /lol-perks/v1/pages HTTP/1.1");
        assert_eq!(requests[1].0, "PUT /lol-perks/v1/pages/7 HTTP/1.1");
        assert_eq!(requests[1].1["id"], 7);
        assert_eq!(requests[1].1["name"], "Featherstorm Ahri MID");
        assert_eq!(requests[1].1["current"], true);
    }

    #[tokio::test]
    async fn failed_refresh_does_not_delete_or_create_any_page() {
        let (client, server) = mock_client(vec![
            (
                200,
                json!([{"id": 7, "name": "Featherstorm Xayah ADC", "isDeletable": true}]),
            ),
            (500, json!({"message": "test failure"})),
        ]);
        assert!(import(
            &client,
            page_value(&RunePageIds::default(), "Featherstorm Ahri MID")
        )
        .await
        .is_err());
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1].0, "PUT /lol-perks/v1/pages/7 HTTP/1.1");
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
            "Featherstorm Xayah",
        );
        assert_eq!(page["primaryStyleId"], 8000);
        assert_eq!(page["selectedPerkIds"].as_array().unwrap().len(), 9);
        assert_eq!(spell_id("heal"), Some(7));
    }
}
