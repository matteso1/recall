//! Pre-queue preparation is independent of the actual, assigned match champion.
use crate::{controller::now_ms, settings::Settings, App};
use anyhow::{bail, Context, Result};
use featherstorm_core::{
    aggregate::{self, Position},
    ddragon::Catalog,
    engine::{self, Inputs},
    itemset,
    lcu::Lcu,
    session,
    state::{SwiftplaySlotView, SwiftplayView},
    swiftplay::{
        linked_rune_page, prepare_slots, slots_match, swiftplay_page_update, SlotUpdate,
        SwiftplayLobby,
    },
};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::JoinHandle;

const RETRY_MS: u64 = 30_000;

/// A slot is a champion AND role; never infer the assignment from list order.
pub fn selected_role(lobby: &SwiftplayLobby, champion: u32) -> Option<Position> {
    let mut matches = lobby.slots.iter().filter(|s| s.champion_id == champion);
    let role = matches.next()?.position;
    matches.all(|s| s.position == role).then_some(role)
}

fn settled(status: &str) -> bool {
    matches!(status, "done" | "kept" | "off")
}

fn is_ready(view: &SwiftplayView) -> bool {
    view.slots.len() == 2
        && view.slots.iter().all(|s| {
            s.plan.is_some()
                && settled(&s.imports.runes)
                && settled(&s.imports.spells)
                && settled(&s.imports.itemset)
        })
}

fn preserve_edits(view: &mut SwiftplayView, before: &Value, after: &Value) {
    for slot in &mut view.slots {
        let old = &before[slot.index];
        let new = &after[slot.index];
        let perks = |s: &Value| {
            s["perks"]
                .as_str()
                .and_then(|p| serde_json::from_str::<Value>(p).ok())
        };
        if perks(old) != perks(new) {
            slot.imports.runes = "kept".into();
        }
        if old["spell1"] != new["spell1"] || old["spell2"] != new["spell2"] {
            slot.imports.spells = "kept".into();
        }
    }
}

fn same_choices(a: &SwiftplayLobby, b: &SwiftplayLobby) -> bool {
    a.party_id == b.party_id
        && a.slots.len() == b.slots.len()
        && a.slots.iter().zip(&b.slots).all(|(a, b)| {
            a.index == b.index && a.champion_id == b.champion_id && a.position == b.position
        })
}

#[derive(Clone, Default)]
struct Progress {
    view: SwiftplayView,
    /// Last confirmed write, or the original read when no write was confirmed.
    raw: Value,
    completed_at_ms: u64,
}

fn retain_unchanged(
    view: &mut SwiftplayView,
    old: &SwiftplayLobby,
    new: &SwiftplayLobby,
    previous: &Progress,
) {
    if old.party_id != new.party_id {
        return;
    }
    for choice in &new.slots {
        if old
            .slots
            .get(choice.index)
            .is_some_and(|s| s.champion_id == choice.champion_id && s.position == choice.position)
        {
            if let Some(saved) = previous.view.slots.get(choice.index) {
                let mut retained = SwiftplayView {
                    slots: vec![saved.clone()],
                    ..Default::default()
                };
                preserve_edits(&mut retained, &previous.raw, &new.raw_slots);
                view.slots[choice.index] = retained.slots.remove(0);
            }
        }
    }
}

/// One bounded worker prepares both choices; it never touches the active-match planner.
#[derive(Default)]
pub struct Runtime {
    choices: Option<SwiftplayLobby>,
    progress: Arc<Mutex<Progress>>,
    task: Option<JoinHandle<()>>,
    retry_at_ms: u64,
}

impl Runtime {
    pub fn role_for(&self, champion: u32) -> Option<Position> {
        self.choices
            .as_ref()
            .and_then(|c| selected_role(c, champion))
    }

    pub fn stop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
            let mut p = self.progress.lock().unwrap();
            p.view.preparing = false;
            for slot in &mut p.view.slots {
                for status in [
                    &mut slot.imports.runes,
                    &mut slot.imports.spells,
                    &mut slot.imports.itemset,
                ] {
                    if status == "working" {
                        *status = "idle".into();
                    }
                }
            }
            p.view.ready = is_ready(&p.view);
            p.view.message = Some("Preparation paused outside the lobby".into());
            self.retry_at_ms = 0;
        }
    }

    pub fn clear(&mut self) {
        self.stop();
        self.choices = None;
        self.progress = Arc::new(Mutex::new(Progress::default()));
        self.retry_at_ms = 0;
    }

    pub async fn observe(
        &mut self,
        lobby: &SwiftplayLobby,
        observed_at: u64,
        phase: &str,
        st: &Arc<App>,
        catalog: &Arc<Catalog>,
        lcu: Option<Lcu>,
    ) -> SwiftplayView {
        if self
            .choices
            .as_ref()
            .is_none_or(|old| !same_choices(old, lobby))
        {
            self.stop();
            let previous = self.progress.lock().unwrap().clone();
            let mut view = SwiftplayView {
                slots: lobby
                    .slots
                    .iter()
                    .map(|slot| SwiftplaySlotView {
                        index: slot.index,
                        champion: catalog.champion_name(slot.champion_id),
                        position: slot.position.label().into(),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            };
            if let Some(old) = self.choices.as_ref() {
                retain_unchanged(&mut view, old, lobby, &previous);
            }
            self.progress = Arc::new(Mutex::new(Progress {
                view,
                raw: lobby.raw_slots.clone(),
                completed_at_ms: 0,
            }));
            self.choices = Some(lobby.clone());
            self.retry_at_ms = 0;
        }
        if self.task.as_ref().is_some_and(JoinHandle::is_finished) {
            if let Some(task) = self.task.take() {
                if let Err(error) = task.await {
                    let mut p = self.progress.lock().unwrap();
                    p.view.preparing = false;
                    p.view.ready = false;
                    p.view.message = Some(format!("Preparation interrupted: {error}"));
                }
            }
            self.retry_at_ms = now_ms().saturating_add(RETRY_MS);
        }
        if phase != "Lobby" {
            self.stop();
        }
        {
            let mut p = self.progress.lock().unwrap();
            // A poll started before the write response is not a subsequent manual edit.
            if self.task.is_none()
                && observed_at > p.completed_at_ms
                && !slots_match(&p.raw, &lobby.raw_slots)
            {
                let before = p.raw.clone();
                preserve_edits(&mut p.view, &before, &lobby.raw_slots);
                p.raw = lobby.raw_slots.clone();
            }
            p.view.observed_at_ms = Some(observed_at);
            p.view.ready = is_ready(&p.view);
        }
        let should_prepare = {
            let p = self.progress.lock().unwrap();
            !is_ready(&p.view)
                && self.task.is_none()
                && phase == "Lobby"
                && now_ms() >= self.retry_at_ms
        };
        if should_prepare {
            if let Some(lcu) = lcu {
                let st = st.clone();
                let catalog = catalog.clone();
                let expected = lobby.clone();
                let progress = self.progress.clone();
                let settings = st.settings.lock().unwrap().clone();
                progress.lock().unwrap().view.preparing = true;
                self.task = Some(tokio::spawn(async move {
                    prepare(&st, &catalog, &lcu, &expected, &settings, &progress).await;
                }));
            }
        }
        self.progress.lock().unwrap().view.clone()
    }
}

/// Fresh reads immediately precede every mutating request. The LCU has no CAS operation,
/// so an edit exactly concurrent with the HTTP write cannot be made transactional.
async fn guard(lcu: &Lcu, expected: &SwiftplayLobby, raw: &Value) -> Result<()> {
    let current = lcu.lobby().await?.context("Lobby is no longer available")?;
    let current = SwiftplayLobby::from_lobby(&current)?.context("No longer in Swiftplay")?;
    if !same_choices(expected, &current) {
        bail!("Swiftplay choices changed; preparing the new choices");
    }
    let current = lcu.player_slots().await?;
    if !slots_match(&current, raw) {
        bail!("Loadout edited during preparation; your changes were kept");
    }
    if lcu.gameflow_phase().await? != "Lobby" {
        bail!("Queue started; loadout writes are paused");
    }
    Ok(())
}

fn error_working(view: &mut SwiftplayView, error: &str) {
    for slot in &mut view.slots {
        for status in [
            &mut slot.imports.runes,
            &mut slot.imports.spells,
            &mut slot.imports.itemset,
        ] {
            if status == "working" {
                *status = format!("error: {error}");
            }
        }
    }
}

async fn prepare(
    st: &App,
    catalog: &Catalog,
    lcu: &Lcu,
    expected: &SwiftplayLobby,
    settings: &Settings,
    progress: &Mutex<Progress>,
) {
    let mut updates = Vec::new();
    let mut sets = Vec::new();
    for choice in &expected.slots {
        {
            let mut p = progress.lock().unwrap();
            let slot = &mut p.view.slots[choice.index];
            for (status, enabled) in [
                (&mut slot.imports.runes, settings.auto_runes),
                (&mut slot.imports.spells, settings.auto_spells),
                (&mut slot.imports.itemset, settings.auto_itemset),
            ] {
                if !enabled {
                    *status = "off".into();
                } else if !settled(status) {
                    *status = "working".into();
                }
            }
            slot.message = Some("Loading role-specific build data".into());
        }
        let dir = crate::settings::data_dir().join("aggregate");
        let result = tokio::time::timeout(
            Duration::from_secs(28),
            aggregate::load(
                &dir,
                &settings.region,
                &settings.tier,
                choice.champion_id,
                Some(choice.position),
            ),
        )
        .await;
        let result = match result {
            Ok(r) => r,
            Err(_) => Err(anyhow::anyhow!("Build source timed out")),
        };
        let prepared = result.and_then(|aggregate| {
            if !aggregate.provenance.is_fresh_at(now_ms()) {
                bail!("Build source is stale; imports paused");
            }
            let champion = catalog.champion_name(choice.champion_id);
            // Queue 480 is Swiftplay: its shop rules apply to the prepared item set and start.
            let plan = engine::plan_in_mode(
                &Inputs {
                    champion: &champion,
                    pack: st.pack_for(&champion),
                    aggregate: Some(&aggregate),
                    traits: &st.traits,
                    catalog,
                    enemies: &[],
                    live: None,
                },
                &engine::PlannerPreferences::default(),
                engine::GameMode::Swiftplay,
            );
            if plan.path.is_empty() {
                bail!("No compatible build for this champion and role");
            }
            Ok(plan)
        });
        let mut p = progress.lock().unwrap();
        let slot = &mut p.view.slots[choice.index];
        match prepared {
            Ok(plan) => {
                let mut update = SlotUpdate {
                    index: choice.index,
                    rune_page: None,
                    spells: None,
                };
                if slot.imports.runes == "working" {
                    match plan
                        .runes
                        .as_ref()
                        .context("No rune page available")
                        .and_then(|page| {
                            session::validate_rune_page(page, catalog)
                                .map_err(anyhow::Error::msg)?;
                            Ok(page.clone())
                        }) {
                        Ok(page) => update.rune_page = Some(page),
                        Err(error) => slot.imports.runes = format!("error: {error}"),
                    }
                }
                let mut notes: Vec<String> = plan
                    .source_position
                    .as_ref()
                    .and_then(|_| plan.note.clone())
                    .into_iter()
                    .collect();
                if slot.imports.spells == "working" {
                    match plan.spell_ids.as_slice() {
                        [a, b] => update.spells = Some([*a, *b]),
                        _ => {
                            // No pair can be proposed for this role from the data: the saved
                            // choice stays, which is a settled state, not a failure.
                            slot.imports.spells = "kept".into();
                            notes.push(format!(
                                "Your summoner spells were kept; the data has no {} pair",
                                slot.position
                            ));
                        }
                    }
                }
                if slot.imports.itemset == "working" {
                    match itemset::build_for_role(
                        &plan,
                        st.pack_for(&plan.champion),
                        catalog,
                        choice.champion_id,
                    ) {
                        Ok(set) => sets.push((choice.index, set)),
                        Err(error) => slot.imports.itemset = format!("error: {error}"),
                    }
                }
                updates.push(update);
                slot.plan = Some(plan);
                slot.message = (!notes.is_empty()).then(|| notes.join(". "));
            }
            Err(error) => {
                for status in [
                    &mut slot.imports.runes,
                    &mut slot.imports.spells,
                    &mut slot.imports.itemset,
                ] {
                    if status == "working" {
                        *status = format!("error: {error}");
                    }
                }
                slot.message = Some(error.to_string());
            }
        }
    }
    let result = import_prepared(lcu, expected, &updates, &sets, progress).await;
    let mut p = progress.lock().unwrap();
    if let Err(error) = result {
        log::warn!("Swiftplay preparation: {error}");
        error_working(&mut p.view, &error.to_string());
        p.view.message = Some(error.to_string());
    } else {
        // Ready is still honest: a same-champion fallback is prepared, but said so.
        let fallbacks: Vec<String> = p
            .view
            .slots
            .iter()
            .filter_map(|slot| {
                slot.plan
                    .as_ref()
                    .and_then(|plan| plan.source_position.as_ref())
                    .map(|source| {
                        format!(
                            "{} {}: {source} build (no {} data)",
                            slot.champion, slot.position, slot.position
                        )
                    })
            })
            .collect();
        p.view.message = (!fallbacks.is_empty()).then(|| format!("Prepared. {}", fallbacks.join("; ")));
    }
    p.completed_at_ms = now_ms();
    p.view.preparing = false;
    p.view.ready = is_ready(&p.view);
}

async fn import_prepared(
    lcu: &Lcu,
    expected: &SwiftplayLobby,
    updates: &[SlotUpdate],
    sets: &[(usize, Value)],
    progress: &Mutex<Progress>,
) -> Result<()> {
    let mut raw = expected.raw_slots.clone();
    let mut updates = updates.to_vec();
    let mut original_pages = std::collections::HashMap::new();
    // Preflight ownership before writing perks through either API: a slot write
    // may also affect the linked page on some client versions.
    if updates.iter().any(|u| u.rune_page.is_some()) {
        let pages = lcu.perk_pages().await?;
        for update in &mut updates {
            if update.rune_page.is_none() {
                continue;
            }
            let check = rune_page_target(&pages, expected, update, progress);
            if let Err(error) = check {
                progress.lock().unwrap().view.slots[update.index]
                    .imports
                    .runes = format!("error: {error}");
                update.rune_page = None;
            } else {
                original_pages.insert(
                    update.index,
                    linked_rune_page(&pages, expected.slots[update.index].champion_id)?.clone(),
                );
            }
        }
    }
    if updates
        .iter()
        .any(|u| u.rune_page.is_some() || u.spells.is_some())
    {
        guard(lcu, expected, &raw).await?;
        let target = prepare_slots(&raw, &raw, &updates)?;
        if !slots_match(&target, &raw) {
            if !original_pages.is_empty() {
                let latest = lcu.perk_pages().await?;
                for (index, original) in &original_pages {
                    let current = linked_rune_page(&latest, expected.slots[*index].champion_id)?;
                    if page_snapshot(current) != page_snapshot(original) {
                        progress.lock().unwrap().view.slots[*index].imports.runes = "kept".into();
                        return Err(PageEdited.into());
                    }
                }
                // The pages read above can overlap the user starting their queue.
                if lcu.gameflow_phase().await? != "Lobby" {
                    bail!("Queue started; loadout writes are paused");
                }
            }
            lcu.put_player_slots(&target).await?;
        }
        let confirmed = lcu.player_slots().await?;
        if !slots_match(&confirmed, &target) {
            bail!("Client has not confirmed the saved loadouts");
        }
        raw = confirmed;
        let mut p = progress.lock().unwrap();
        p.raw = raw.clone();
        for update in &updates {
            let slot = &mut p.view.slots[update.index];
            if update.spells.is_some() {
                slot.imports.spells = "done".into();
            }
        }
    }
    for update in &updates {
        if update.rune_page.is_none() {
            continue;
        }
        let result = sync_rune_page(
            lcu,
            expected,
            &raw,
            update,
            progress,
            &original_pages[&update.index],
        )
        .await;
        progress.lock().unwrap().view.slots[update.index]
            .imports
            .runes = match result {
            Ok(()) => "done".into(),
            Err(error) if error.is::<PageEdited>() => "kept".into(),
            Err(error) => format!("error: {error}"),
        };
    }
    if !sets.is_empty() {
        let me = lcu.current_summoner().await?;
        let id = me["summonerId"]
            .as_u64()
            .filter(|id| *id > 0)
            .context("Current summoner is unavailable")?;
        let original = lcu.item_sets(id).await?;
        let mut payload = original.clone();
        if !payload["itemSets"].is_array() {
            bail!("Existing item sets could not be read safely");
        }
        for (_, set) in sets {
            payload = itemset::upsert(&payload, set.clone());
        }
        guard(lcu, expected, &raw).await?;
        let latest = lcu.item_sets(id).await?;
        if latest["itemSets"] != original["itemSets"]
            || latest["accountId"] != original["accountId"]
        {
            bail!("Item sets changed during preparation; your changes were kept");
        }
        if lcu.gameflow_phase().await? != "Lobby" {
            bail!("Queue started; item-set writes are paused");
        }
        lcu.put_item_sets(id, &payload).await?;
        let confirmed = lcu.item_sets(id).await?;
        for (index, set) in sets {
            let found = confirmed["itemSets"].as_array().is_some_and(|list| {
                list.iter().any(|s| {
                    s["uid"] == set["uid"]
                        && s["title"] == set["title"]
                        && s["blocks"] == set["blocks"]
                })
            });
            let mut p = progress.lock().unwrap();
            p.view.slots[*index].imports.itemset = if found {
                "done".into()
            } else {
                "error: Client has not confirmed the item set".into()
            };
        }
    }
    Ok(())
}

fn rune_page_target(
    pages: &Value,
    expected: &SwiftplayLobby,
    update: &SlotUpdate,
    progress: &Mutex<Progress>,
) -> Result<Value> {
    let choice = expected
        .slots
        .get(update.index)
        .context("Swiftplay choice is unavailable")?;
    if expected
        .slots
        .iter()
        .filter(|s| s.champion_id == choice.champion_id)
        .count()
        != 1
    {
        bail!(
            "The client shares rune pages for duplicate champion choices; automatic runes paused"
        );
    }
    let page = linked_rune_page(pages, choice.champion_id)?;
    let title = {
        let p = progress.lock().unwrap();
        let slot = &p.view.slots[update.index];
        format!("Featherstorm {} {}", slot.champion, slot.position)
    };
    let mut target = swiftplay_page_update(page, &title)?;
    let runes = update
        .rune_page
        .as_ref()
        .context("No rune update requested")?;
    target["primaryStyleId"] = serde_json::json!(runes.primary_style);
    target["subStyleId"] = serde_json::json!(runes.sub_style);
    target["selectedPerkIds"] = serde_json::json!(runes.perks);
    Ok(target)
}

fn page_payload(page: &Value) -> Value {
    [
        "id",
        "name",
        "current",
        "isTemporary",
        "quickPlayChampionIds",
        "primaryStyleId",
        "subStyleId",
        "selectedPerkIds",
    ]
    .into_iter()
    .map(|key| (key.to_owned(), page[key].clone()))
    .collect()
}

#[derive(Debug)]
struct PageEdited;

impl std::fmt::Display for PageEdited {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Rune page edited during preparation; your changes were kept")
    }
}
impl std::error::Error for PageEdited {}

fn page_snapshot(page: &Value) -> Value {
    let mut value = page_payload(page);
    value.as_object_mut().unwrap().remove("current");
    value["isEditable"] = page["isEditable"].clone();
    value
}

async fn sync_rune_page(
    lcu: &Lcu,
    expected: &SwiftplayLobby,
    raw: &Value,
    update: &SlotUpdate,
    progress: &Mutex<Progress>,
    observed_page: &Value,
) -> Result<()> {
    let champion = expected.slots[update.index].champion_id;
    let pages = lcu.perk_pages().await?;
    let original = linked_rune_page(&pages, champion)?;
    // Only an exact rune-content update may have come from our own raw-slot
    // write. Edits/reassociation since preflight must not become a new baseline.
    let mut after_slot_write = observed_page.clone();
    let runes = update
        .rune_page
        .as_ref()
        .context("No rune update requested")?;
    after_slot_write["primaryStyleId"] = serde_json::json!(runes.primary_style);
    after_slot_write["subStyleId"] = serde_json::json!(runes.sub_style);
    after_slot_write["selectedPerkIds"] = serde_json::json!(runes.perks);
    if page_snapshot(original) != page_snapshot(observed_page)
        && page_snapshot(original) != page_snapshot(&after_slot_write)
    {
        return Err(PageEdited.into());
    }
    let mut target = rune_page_target(&pages, expected, update, progress)?;
    if page_payload(original) != page_payload(&target) {
        guard(lcu, expected, raw).await?;
        let latest = lcu.perk_pages().await?;
        let latest = linked_rune_page(&latest, champion)?;
        if page_snapshot(latest) != page_snapshot(original) {
            return Err(PageEdited.into());
        }
        target["current"] = latest["current"].clone();
        if lcu.gameflow_phase().await? != "Lobby" {
            bail!("Queue started; rune-page writes are paused");
        }
        lcu.update_perk_page(
            target["id"].as_u64().context("Invalid rune page id")?,
            &page_payload(&target),
        )
        .await?;
    }
    let confirmed = lcu.perk_pages().await?;
    let confirmed = linked_rune_page(&confirmed, champion)?;
    // Global `current` belongs to the editor, not either choice. It may change
    // independently; confirm the page id, association, name and exact nine perks.
    for key in [
        "id",
        "name",
        "quickPlayChampionIds",
        "primaryStyleId",
        "subStyleId",
        "selectedPerkIds",
    ] {
        if confirmed[key] != target[key] {
            bail!("Client has not confirmed the linked Swiftplay rune page");
        }
    }
    if confirmed["isValid"] != true || !slots_match(&lcu.player_slots().await?, raw) {
        bail!("Client has not confirmed valid runes and spells for this choice");
    }
    log::info!(
        "Swiftplay verified {}: page {}, spells {}/{}",
        target["name"],
        target["id"],
        raw[update.index]["spell1"],
        raw[update.index]["spell2"]
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use featherstorm_core::{engine::Plan, lcu::Lockfile, state::Imports};
    use serde_json::json;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };

    /// Actual HTTP adapter and mutation sequence, with no account/client connection.
    fn scripted_client(
        script: Vec<(&'static str, Value)>,
    ) -> (Lcu, std::thread::JoinHandle<Vec<Value>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let mut bodies = Vec::new();
            for (expected, response) in script {
                let deadline = std::time::Instant::now() + Duration::from_secs(4);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                std::time::Instant::now() < deadline,
                                "Missing request: {expected}"
                            );
                            std::thread::sleep(Duration::from_millis(2));
                        }
                        Err(error) => panic!("{error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0_u8; 1];
                while !request.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                let header = String::from_utf8(request).unwrap();
                assert!(
                    header.starts_with(&format!("{expected} HTTP/1.1\r\n")),
                    "{header}"
                );
                let len = header
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|n| n.parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                let mut body = vec![0; len];
                stream.read_exact(&mut body).unwrap();
                bodies.push(serde_json::from_slice(&body).unwrap_or(Value::Null));
                let response = response.to_string();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
            }
            bodies
        });
        let lcu = Lcu::from_lockfile(Lockfile {
            process: "test".into(),
            pid: 1,
            port,
            password: "test-only".into(),
            protocol: "http".into(),
        })
        .unwrap();
        (lcu, server)
    }

    fn raw_lobby(choices: &SwiftplayLobby) -> Value {
        json!({"gameConfig":{"queueId":480},"partyId":choices.party_id,
            "localMember":{"playerSlots":choices.raw_slots}})
    }

    fn progress(choices: &SwiftplayLobby) -> Mutex<Progress> {
        Mutex::new(Progress {
            raw: choices.raw_slots.clone(),
            view: SwiftplayView {
                slots: choices
                    .slots
                    .iter()
                    .map(|s| SwiftplaySlotView {
                        index: s.index,
                        imports: Imports {
                            runes: "off".into(),
                            spells: "working".into(),
                            itemset: "working".into(),
                        },
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    fn rune_update() -> SlotUpdate {
        SlotUpdate {
            index: 1,
            spells: Some([4, 14]),
            rune_page: Some(aggregate::RunePageIds {
                primary_style: 8000,
                sub_style: 8400,
                perks: vec![8010, 9111, 9104, 8299, 8242, 8473, 5005, 5008, 5001],
                ..Default::default()
            }),
        }
    }

    fn rune_progress(choices: &SwiftplayLobby) -> Mutex<Progress> {
        let p = progress(choices);
        {
            let mut p = p.lock().unwrap();
            p.view.slots[1].champion = "Irelia".into();
            p.view.slots[1].position = "Mid".into();
            p.view.slots[1].imports.runes = "working".into();
        }
        p
    }

    fn page_fixture(name: &str) -> Value {
        json!({"id":7,"name":name,"isTemporary":true,"isEditable":true,
            "current":false,"quickPlayChampionIds":[39],"isValid":true,
            "primaryStyleId":8000,"subStyleId":8400,
            "selectedPerkIds":[8010,9111,9104,8299,8242,8473,5005,5008,5001]})
    }

    #[tokio::test]
    async fn raw_slot_echo_without_a_linked_page_is_not_runes_ready() {
        let choices = lobby();
        let update = rune_update();
        let target = prepare_slots(
            &choices.raw_slots,
            &choices.raw_slots,
            std::slice::from_ref(&update),
        )
        .unwrap();
        let (lcu, server) = scripted_client(vec![
            (
                "GET /lol-perks/v1/pages",
                json!([page_fixture("Irelia - Conqueror")]),
            ),
            ("GET /lol-lobby/v2/lobby", raw_lobby(&choices)),
            (
                "GET /lol-lobby/v1/lobby/members/localMember/player-slots",
                choices.raw_slots.clone(),
            ),
            ("GET /lol-gameflow/v1/gameflow-phase", json!("Lobby")),
            (
                "GET /lol-perks/v1/pages",
                json!([page_fixture("Irelia - Conqueror")]),
            ),
            ("GET /lol-gameflow/v1/gameflow-phase", json!("Lobby")),
            (
                "PUT /lol-lobby/v1/lobby/members/localMember/player-slots",
                Value::Null,
            ),
            (
                "GET /lol-lobby/v1/lobby/members/localMember/player-slots",
                target,
            ),
            ("GET /lol-perks/v1/pages", json!([])),
        ]);
        let p = rune_progress(&choices);
        import_prepared(&lcu, &choices, &[update], &[], &p)
            .await
            .unwrap();
        assert_ne!(p.lock().unwrap().view.slots[1].imports.runes, "done");
        assert_eq!(p.lock().unwrap().view.slots[1].imports.spells, "done");
        assert_eq!(server.join().unwrap().len(), 9);
    }

    #[tokio::test]
    async fn linked_temporary_page_is_updated_in_place_and_read_back() {
        let mut choices = lobby();
        let update = rune_update();
        choices.raw_slots = prepare_slots(
            &choices.raw_slots,
            &choices.raw_slots,
            std::slice::from_ref(&update),
        )
        .unwrap();
        let mut original = page_fixture("Irelia - Conqueror");
        original["subStyleId"] = json!(8300);
        original["selectedPerkIds"] = json!([8010, 9111, 9104, 8299, 8345, 8347, 5005, 5008, 5001]);
        let named = page_fixture("Featherstorm Irelia Mid");
        let (lcu, server) = scripted_client(vec![
            ("GET /lol-perks/v1/pages", json!([original])),
            ("GET /lol-lobby/v2/lobby", raw_lobby(&choices)),
            (
                "GET /lol-lobby/v1/lobby/members/localMember/player-slots",
                choices.raw_slots.clone(),
            ),
            ("GET /lol-gameflow/v1/gameflow-phase", json!("Lobby")),
            (
                "GET /lol-lobby/v1/lobby/members/localMember/player-slots",
                choices.raw_slots.clone(),
            ),
            ("GET /lol-perks/v1/pages", json!([original])),
            ("GET /lol-lobby/v2/lobby", raw_lobby(&choices)),
            (
                "GET /lol-lobby/v1/lobby/members/localMember/player-slots",
                choices.raw_slots.clone(),
            ),
            ("GET /lol-gameflow/v1/gameflow-phase", json!("Lobby")),
            ("GET /lol-perks/v1/pages", json!([original])),
            ("GET /lol-gameflow/v1/gameflow-phase", json!("Lobby")),
            ("PUT /lol-perks/v1/pages/7", named.clone()),
            ("GET /lol-perks/v1/pages", json!([named])),
            (
                "GET /lol-lobby/v1/lobby/members/localMember/player-slots",
                choices.raw_slots.clone(),
            ),
        ]);
        let p = rune_progress(&choices);
        import_prepared(&lcu, &choices, &[update], &[], &p)
            .await
            .unwrap();
        assert_eq!(p.lock().unwrap().view.slots[1].imports.runes, "done");
        let requests = server.join().unwrap();
        assert_eq!(requests[11]["name"], "Featherstorm Irelia Mid");
        assert_eq!(requests[11]["current"], false);
        assert_eq!(requests[11]["isTemporary"], true);
        assert_eq!(requests[11]["quickPlayChampionIds"], json!([39]));
        assert_eq!(requests[11]["subStyleId"], 8400);
        assert_eq!(requests[11]["selectedPerkIds"], named["selectedPerkIds"]);
    }

    #[tokio::test]
    async fn verified_named_page_is_idempotent_and_never_sets_global_current() {
        let mut choices = lobby();
        let update = rune_update();
        choices.raw_slots = prepare_slots(
            &choices.raw_slots,
            &choices.raw_slots,
            std::slice::from_ref(&update),
        )
        .unwrap();
        let page = page_fixture("Featherstorm Irelia Mid");
        let (lcu, server) = scripted_client(vec![
            ("GET /lol-perks/v1/pages", json!([page])),
            ("GET /lol-perks/v1/pages", json!([page])),
            (
                "GET /lol-lobby/v1/lobby/members/localMember/player-slots",
                choices.raw_slots.clone(),
            ),
        ]);
        sync_rune_page(
            &lcu,
            &choices,
            &choices.raw_slots,
            &update,
            &rune_progress(&choices),
            &page,
        )
        .await
        .unwrap();
        assert_eq!(server.join().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn unconfirmed_or_invalid_linked_page_never_reports_success() {
        for invalid in [false, true] {
            let choices = lobby();
            let page = page_fixture("Featherstorm Irelia Mid");
            let mut unconfirmed = page.clone();
            if invalid {
                unconfirmed["isValid"] = json!(false);
            } else {
                unconfirmed["subStyleId"] = json!(8300);
            }
            let (lcu, server) = scripted_client(vec![
                ("GET /lol-perks/v1/pages", json!([page])),
                ("GET /lol-perks/v1/pages", json!([unconfirmed])),
            ]);
            assert!(sync_rune_page(
                &lcu,
                &choices,
                &choices.raw_slots,
                &rune_update(),
                &rune_progress(&choices),
                &page,
            )
            .await
            .is_err());
            assert_eq!(server.join().unwrap().len(), 2);
        }
    }

    #[tokio::test]
    async fn concurrent_page_edit_or_queue_start_prevents_page_put() {
        for queue_started in [false, true] {
            let choices = lobby();
            let original = page_fixture("Irelia - Conqueror");
            let mut edited = original.clone();
            edited["name"] = json!("My edit");
            let mut script = vec![
                ("GET /lol-perks/v1/pages", json!([original])),
                ("GET /lol-lobby/v2/lobby", raw_lobby(&choices)),
                (
                    "GET /lol-lobby/v1/lobby/members/localMember/player-slots",
                    choices.raw_slots.clone(),
                ),
                (
                    "GET /lol-gameflow/v1/gameflow-phase",
                    json!(if queue_started {
                        "Matchmaking"
                    } else {
                        "Lobby"
                    }),
                ),
            ];
            if !queue_started {
                script.push(("GET /lol-perks/v1/pages", json!([edited])));
            }
            let (lcu, server) = scripted_client(script);
            let error = sync_rune_page(
                &lcu,
                &choices,
                &choices.raw_slots,
                &rune_update(),
                &rune_progress(&choices),
                &original,
            )
            .await
            .unwrap_err();
            assert!(error.to_string().contains(if queue_started {
                "Queue started"
            } else {
                "edited during"
            }));
            assert_eq!(
                server.join().unwrap().len(),
                if queue_started { 4 } else { 5 }
            );
        }
    }

    #[tokio::test]
    async fn changing_global_editor_selection_does_not_cancel_or_reselect_choice_runes() {
        let choices = lobby();
        let page = page_fixture("Irelia - Conqueror");
        let mut selected = page.clone();
        selected["current"] = json!(true);
        let mut named = page_fixture("Featherstorm Irelia Mid");
        named["current"] = json!(true);
        let (lcu, server) = scripted_client(vec![
            ("GET /lol-perks/v1/pages", json!([page])),
            ("GET /lol-lobby/v2/lobby", raw_lobby(&choices)),
            (
                "GET /lol-lobby/v1/lobby/members/localMember/player-slots",
                choices.raw_slots.clone(),
            ),
            ("GET /lol-gameflow/v1/gameflow-phase", json!("Lobby")),
            ("GET /lol-perks/v1/pages", json!([selected])),
            ("GET /lol-gameflow/v1/gameflow-phase", json!("Lobby")),
            ("PUT /lol-perks/v1/pages/7", named.clone()),
            ("GET /lol-perks/v1/pages", json!([named])),
            (
                "GET /lol-lobby/v1/lobby/members/localMember/player-slots",
                choices.raw_slots.clone(),
            ),
        ]);
        let result = sync_rune_page(
            &lcu,
            &choices,
            &choices.raw_slots,
            &rune_update(),
            &rune_progress(&choices),
            &page,
        )
        .await;
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(server.join().unwrap()[6]["current"], true);
    }

    #[tokio::test]
    async fn queue_start_during_last_page_read_blocks_page_write() {
        let choices = lobby();
        let page = page_fixture("Irelia - Conqueror");
        let (lcu, server) = scripted_client(vec![
            ("GET /lol-perks/v1/pages", json!([page])),
            ("GET /lol-lobby/v2/lobby", raw_lobby(&choices)),
            (
                "GET /lol-lobby/v1/lobby/members/localMember/player-slots",
                choices.raw_slots.clone(),
            ),
            ("GET /lol-gameflow/v1/gameflow-phase", json!("Lobby")),
            ("GET /lol-perks/v1/pages", json!([page])),
            ("GET /lol-gameflow/v1/gameflow-phase", json!("Matchmaking")),
        ]);
        let error = sync_rune_page(
            &lcu,
            &choices,
            &choices.raw_slots,
            &rune_update(),
            &rune_progress(&choices),
            &page,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("Queue started"));
        assert_eq!(server.join().unwrap().len(), 6);
    }

    #[tokio::test]
    async fn page_only_edit_since_preflight_is_kept_and_not_retried() {
        let mut choices = lobby();
        let update = rune_update();
        choices.raw_slots = prepare_slots(
            &choices.raw_slots,
            &choices.raw_slots,
            std::slice::from_ref(&update),
        )
        .unwrap();
        let original = page_fixture("Irelia - Conqueror");
        let mut edited = original.clone();
        edited["name"] = json!("Irelia - my edit");
        let (lcu, server) = scripted_client(vec![
            ("GET /lol-perks/v1/pages", json!([original])),
            ("GET /lol-lobby/v2/lobby", raw_lobby(&choices)),
            (
                "GET /lol-lobby/v1/lobby/members/localMember/player-slots",
                choices.raw_slots.clone(),
            ),
            ("GET /lol-gameflow/v1/gameflow-phase", json!("Lobby")),
            (
                "GET /lol-lobby/v1/lobby/members/localMember/player-slots",
                choices.raw_slots.clone(),
            ),
            ("GET /lol-perks/v1/pages", json!([edited])),
        ]);
        let p = rune_progress(&choices);
        import_prepared(&lcu, &choices, &[update], &[], &p)
            .await
            .unwrap();
        assert_eq!(p.lock().unwrap().view.slots[1].imports.runes, "kept");
        assert!(settled(&p.lock().unwrap().view.slots[1].imports.runes));
        assert_eq!(server.join().unwrap().len(), 6);
    }

    #[tokio::test]
    async fn personal_page_is_protected_before_either_perks_endpoint_is_written() {
        let choices = lobby();
        let mut personal = page_fixture("My personal page");
        personal["isTemporary"] = json!(false);
        let mut update = rune_update();
        update.spells = None;
        let (lcu, server) = scripted_client(vec![("GET /lol-perks/v1/pages", json!([personal]))]);
        let p = rune_progress(&choices);
        import_prepared(&lcu, &choices, &[update], &[], &p)
            .await
            .unwrap();
        assert!(p.lock().unwrap().view.slots[1]
            .imports
            .runes
            .contains("Personal rune page preserved"));
        assert_eq!(server.join().unwrap().len(), 1);
    }

    #[test]
    fn changing_secondary_choice_preserves_primary_manual_edits_and_build() {
        let old = lobby();
        let mut new = old.clone();
        new.slots[1].champion_id = 103;
        new.raw_slots[1]["championId"] = json!(103);
        new.raw_slots[0]["spell2"] = json!(7);
        let mut previous = progress(&old).into_inner().unwrap();
        previous.view.slots[0].plan = Some(Plan {
            champion: "Xayah".into(),
            position: Some("ADC".into()),
            ..Default::default()
        });
        previous.view.slots[0].imports.itemset = "done".into();
        let mut next = SwiftplayView {
            slots: (0..2)
                .map(|index| SwiftplaySlotView {
                    index,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        retain_unchanged(&mut next, &old, &new, &previous);
        assert_eq!(next.slots[0].imports.spells, "kept");
        assert_eq!(next.slots[0].imports.runes, "off");
        assert_eq!(next.slots[0].imports.itemset, "done");
        assert_eq!(next.slots[0].plan, previous.view.slots[0].plan);
        assert_eq!(next.slots[1].imports.runes, "");
        assert_eq!(next.slots[1].imports.spells, "");
    }

    #[tokio::test]
    async fn queueing_or_manual_edit_blocks_the_actual_http_write() {
        let choices = lobby();
        let updates = [SlotUpdate {
            index: 0,
            rune_page: None,
            spells: Some([4, 7]),
        }];
        let (lcu, server) = scripted_client(vec![
            ("GET /lol-lobby/v2/lobby", raw_lobby(&choices)),
            (
                "GET /lol-lobby/v1/lobby/members/localMember/player-slots",
                choices.raw_slots.clone(),
            ),
            ("GET /lol-gameflow/v1/gameflow-phase", json!("Matchmaking")),
        ]);
        let error = import_prepared(&lcu, &choices, &updates, &[], &progress(&choices))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("Queue started"));
        assert_eq!(server.join().unwrap().len(), 3);

        let mut edited = choices.raw_slots.clone();
        edited[1]["spell2"] = json!(7);
        let (lcu, server) = scripted_client(vec![
            ("GET /lol-lobby/v2/lobby", raw_lobby(&choices)),
            (
                "GET /lol-lobby/v1/lobby/members/localMember/player-slots",
                edited,
            ),
        ]);
        let error = import_prepared(&lcu, &choices, &updates, &[], &progress(&choices))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("edited during"));
        assert_eq!(server.join().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn both_loadouts_and_itemsets_are_confirmed_without_losing_foreign_sets() {
        let choices = lobby();
        let updates = [
            SlotUpdate {
                index: 0,
                rune_page: None,
                spells: Some([4, 7]),
            },
            SlotUpdate {
                index: 1,
                rune_page: None,
                spells: Some([4, 12]),
            },
        ];
        let target = prepare_slots(&choices.raw_slots, &choices.raw_slots, &updates).unwrap();
        let sets = vec![
            (
                0,
                json!({"uid":"xayah-adc","title":"Featherstorm Xayah ADC","blocks":[]}),
            ),
            (
                1,
                json!({"uid":"irelia-mid","title":"Featherstorm Irelia Mid","blocks":[]}),
            ),
        ];
        let foreign = json!({"uid":"user","title":"My set","blocks":[{"type":"keep"}]});
        let confirmed_sets = json!({"itemSets":[foreign,sets[0].1,sets[1].1]});
        let (lcu, server) = scripted_client(vec![
            ("GET /lol-lobby/v2/lobby", raw_lobby(&choices)),
            (
                "GET /lol-lobby/v1/lobby/members/localMember/player-slots",
                choices.raw_slots.clone(),
            ),
            ("GET /lol-gameflow/v1/gameflow-phase", json!("Lobby")),
            (
                "PUT /lol-lobby/v1/lobby/members/localMember/player-slots",
                Value::Null,
            ),
            (
                "GET /lol-lobby/v1/lobby/members/localMember/player-slots",
                target.clone(),
            ),
            (
                "GET /lol-summoner/v1/current-summoner",
                json!({"summonerId":123}),
            ),
            (
                "GET /lol-item-sets/v1/item-sets/123/sets",
                json!({"accountId":456,"itemSets":[foreign]}),
            ),
            ("GET /lol-lobby/v2/lobby", raw_lobby(&choices)),
            (
                "GET /lol-lobby/v1/lobby/members/localMember/player-slots",
                target.clone(),
            ),
            ("GET /lol-gameflow/v1/gameflow-phase", json!("Lobby")),
            (
                "GET /lol-item-sets/v1/item-sets/123/sets",
                json!({"accountId":456,"itemSets":[foreign]}),
            ),
            ("GET /lol-gameflow/v1/gameflow-phase", json!("Lobby")),
            ("PUT /lol-item-sets/v1/item-sets/123/sets", Value::Null),
            ("GET /lol-item-sets/v1/item-sets/123/sets", confirmed_sets),
        ]);
        let progress = progress(&choices);
        import_prepared(&lcu, &choices, &updates, &sets, &progress)
            .await
            .unwrap();
        let requests = server.join().unwrap();
        assert_eq!(requests[3], target);
        assert_eq!(requests[12]["itemSets"][0], foreign);
        assert_eq!(requests[12]["accountId"], 456);
        let p = progress.lock().unwrap();
        assert!(slots_match(&p.raw, &target));
        assert!(p
            .view
            .slots
            .iter()
            .all(|s| s.imports.spells == "done" && s.imports.itemset == "done"));
    }

    #[tokio::test]
    async fn unconfirmed_slot_write_never_claims_done_or_writes_itemsets() {
        let choices = lobby();
        let updates = [SlotUpdate {
            index: 0,
            rune_page: None,
            spells: Some([4, 7]),
        }];
        let (lcu, server) = scripted_client(vec![
            ("GET /lol-lobby/v2/lobby", raw_lobby(&choices)),
            (
                "GET /lol-lobby/v1/lobby/members/localMember/player-slots",
                choices.raw_slots.clone(),
            ),
            ("GET /lol-gameflow/v1/gameflow-phase", json!("Lobby")),
            (
                "PUT /lol-lobby/v1/lobby/members/localMember/player-slots",
                Value::Null,
            ),
            (
                "GET /lol-lobby/v1/lobby/members/localMember/player-slots",
                choices.raw_slots.clone(),
            ),
        ]);
        let p = progress(&choices);
        let error = import_prepared(&lcu, &choices, &updates, &[(0, json!({}))], &p)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("not confirmed"));
        assert_ne!(p.lock().unwrap().view.slots[0].imports.spells, "done");
        assert_eq!(server.join().unwrap().len(), 5);
    }

    #[tokio::test]
    async fn concurrent_itemset_edit_is_not_overwritten_by_a_stale_collection() {
        let choices = lobby();
        let (lcu, server) = scripted_client(vec![
            (
                "GET /lol-summoner/v1/current-summoner",
                json!({"summonerId":123}),
            ),
            (
                "GET /lol-item-sets/v1/item-sets/123/sets",
                json!({"accountId":456,"itemSets":[]}),
            ),
            ("GET /lol-lobby/v2/lobby", raw_lobby(&choices)),
            (
                "GET /lol-lobby/v1/lobby/members/localMember/player-slots",
                choices.raw_slots.clone(),
            ),
            ("GET /lol-gameflow/v1/gameflow-phase", json!("Lobby")),
            (
                "GET /lol-item-sets/v1/item-sets/123/sets",
                json!({"accountId":456,"itemSets":[{"title":"User just added this"}]}),
            ),
        ]);
        let error = import_prepared(
            &lcu,
            &choices,
            &[],
            &[(0, json!({"title":"Featherstorm Xayah ADC"}))],
            &progress(&choices),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("Item sets changed"));
        assert_eq!(server.join().unwrap().len(), 6);
    }

    fn lobby() -> SwiftplayLobby {
        SwiftplayLobby::from_lobby(&json!({"gameConfig":{"queueId":480},
            "partyId":"test", "localMember":{"playerSlots":[
                {"championId":498,"positionPreference":"BOTTOM","skinId":498000,
                 "spell1":4,"spell2":21,"perks":"{}"},
                {"championId":39,"positionPreference":"MIDDLE","skinId":39000,
                 "spell1":4,"spell2":14,"perks":"{}"}]}}))
        .unwrap()
        .unwrap()
    }

    #[test]
    fn selected_role_never_guesses_between_two_choices_for_same_champion() {
        let mut choices = lobby();
        assert_eq!(selected_role(&choices, 498), Some(Position::Adc));
        assert_eq!(selected_role(&choices, 39), Some(Position::Mid));
        assert_eq!(selected_role(&choices, 103), None);
        choices.slots[1].champion_id = 498;
        assert_eq!(selected_role(&choices, 498), None);
    }

    #[test]
    fn readiness_requires_two_verified_choices_not_just_successful_fetches() {
        let mut view = SwiftplayView::default();
        assert!(!is_ready(&view));
        let row = SwiftplaySlotView {
            plan: Some(Plan::default()),
            imports: Imports {
                runes: "done".into(),
                spells: "done".into(),
                itemset: "done".into(),
            },
            ..Default::default()
        };
        view.slots.push(row.clone());
        assert!(!is_ready(&view));
        view.slots.push(row);
        assert!(is_ready(&view));
        view.slots[1].imports.runes = "working".into();
        assert!(!is_ready(&view));
        view.slots[1].imports.runes = "kept".into();
        assert!(is_ready(&view));
    }

    #[test]
    fn a_manual_rune_edit_only_preserves_that_choices_runes() {
        let old = lobby().raw_slots;
        let mut new = old.clone();
        new[1]["perks"] = json!("{\"perkIds\":[123]}");
        let mut view = SwiftplayView {
            slots: (0..2)
                .map(|index| SwiftplaySlotView {
                    index,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        preserve_edits(&mut view, &old, &new);
        assert_eq!(view.slots[1].imports.runes, "kept");
        assert_ne!(view.slots[0].imports.runes, "kept");
        assert_ne!(view.slots[1].imports.spells, "kept");
    }
}
