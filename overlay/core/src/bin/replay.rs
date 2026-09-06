//! Offline replay of visible-state captures. This command never fetches or writes data.

use anyhow::{anyhow, bail, Context, Result};
use recall_core::aggregate::{self, Aggregate, Position};
use recall_core::ddragon::{normalize, Catalog};
use recall_core::engine::{self, Inputs, Plan};
use recall_core::live::{self, InvItem, LiveSnapshot, Me, Player};
use recall_core::{pack, shop};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Instant;

const MAX_DEPTH: usize = 8;
const MAX_ENTRIES: usize = 8192;
const MAX_FILES: usize = 4096;
const MAX_CAPTURE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_TOTAL_CAPTURE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_DATA_BYTES: u64 = 32 * 1024 * 1024;
const HELP: &str = "Recall offline replay (stdout only, no network)

replay --session PATH --items PATH --champions PATH --aggregate PATH
       [--runes PATH] [--champion NAME] [--role ROLE] [--aggregate-role ROLE] [--json]
replay --fixtures [--items PATH] [--champions PATH] [--runes PATH]
       [--champion NAME] [--role ROLE] [--json]

--session accepts one raw allGameData JSON file or a capture directory.
Captures are ordered by gameTime within each parent directory. Directory groups
are not verified match boundaries; repeated snapshots are not independent games.
--champion/--role filter known identities/roles; they never replace observed ones.
--aggregate-role names the role a raw --aggregate file was fetched for (a cache
record carries it in its request URL). When --role differs from it, the plans use
the labelled same-champion fallback exactly as the overlay does.
--fixtures uses eight real aggregate fixtures with one pregame plan and two
clearly labelled synthetic visible-state scenarios per champion. No outcomes
are simulated. Item/champion defaults are repository fixture files.

Limits: depth 8, 4096 JSON files, 8192 directory entries, 2 MiB per capture,
256 MiB total capture bytes; symlinks are skipped. Skips/limits are reported.
Latency includes only engine::plan, not loading, decoding, or validation.
JSON records include the complete production plan and snapshot inventory/level/
gold context, but never the live player's summoner name or identifiers.
Exit 0: no observed violations (missing data/paused plans are allowed).
Exit 1: invalid recommendation or price. Exit 2: usage/input error.";

#[derive(Debug, Default)]
struct Options {
    fixtures: bool,
    session: Option<PathBuf>,
    items: Option<PathBuf>,
    champions: Option<PathBuf>,
    aggregate: Option<PathBuf>,
    runes: Option<PathBuf>,
    champion: Option<String>,
    role: Option<Position>,
    /// The role a raw --aggregate file was fetched for, when the file does not carry its request URL.
    aggregate_role: Option<Position>,
    json: bool,
    help: bool,
}

fn parse_args(args: &[String]) -> Result<Options> {
    let mut options = Options::default();
    let mut seen = HashSet::new();
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        if !seen.insert(flag) {
            bail!("duplicate option {flag}");
        }
        match flag {
            "--fixtures" => options.fixtures = true,
            "--json" => options.json = true,
            "--help" | "-h" => options.help = true,
            "--session" | "--items" | "--champions" | "--aggregate" | "--runes" | "--champion"
            | "--role" | "--aggregate-role" => {
                index += 1;
                let value = args
                    .get(index)
                    .filter(|v| !v.starts_with("--"))
                    .ok_or_else(|| anyhow!("{flag} requires a value"))?;
                match flag {
                    "--session" => options.session = Some(value.into()),
                    "--items" => options.items = Some(value.into()),
                    "--champions" => options.champions = Some(value.into()),
                    "--aggregate" => options.aggregate = Some(value.into()),
                    "--runes" => options.runes = Some(value.into()),
                    "--champion" => options.champion = Some(value.clone()),
                    "--role" => {
                        options.role = Some(
                            Position::parse(value)
                                .ok_or_else(|| anyhow!("unknown role {value}"))?,
                        )
                    }
                    "--aggregate-role" => {
                        options.aggregate_role = Some(
                            Position::parse(value)
                                .ok_or_else(|| anyhow!("unknown aggregate role {value}"))?,
                        )
                    }
                    _ => unreachable!(),
                }
            }
            _ => bail!("unknown option {flag}"),
        }
        index += 1;
    }
    if options.help {
        return Ok(options);
    }
    if options.fixtures {
        if options.session.is_some() || options.aggregate.is_some() {
            bail!("--fixtures cannot be combined with --session or --aggregate");
        }
    } else if options.session.is_none()
        || options.items.is_none()
        || options.champions.is_none()
        || options.aggregate.is_none()
    {
        bail!("provide --fixtures, or --session --items --champions --aggregate");
    }
    Ok(options)
}

fn quantile(values: &[f64], fraction: f64) -> Option<f64> {
    if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
        return None;
    }
    let mut sorted: Vec<_> = values
        .iter()
        .copied()
        .filter(|v| v.is_finite() && *v >= 0.0)
        .collect();
    sorted.sort_by(f64::total_cmp);
    if sorted.is_empty() {
        return None;
    }
    let rank = (fraction * sorted.len() as f64).ceil().max(1.0) as usize;
    sorted.get(rank - 1).copied()
}

fn parse_snapshot(value: &Value) -> Result<Option<LiveSnapshot>> {
    if !value.get("gameData").is_some_and(Value::is_object)
        || !value.get("allPlayers").is_some_and(Value::is_array)
    {
        return Ok(None);
    }
    if !value["gameData"]["gameTime"]
        .as_f64()
        .is_some_and(|time| time.is_finite() && time >= 0.0)
    {
        bail!("capture has no valid nonnegative gameTime");
    }
    Ok(Some(live::summarize(value)))
}

#[derive(Debug, Default, Serialize)]
struct LoadCounts {
    directory_entries: usize,
    files_read: usize,
    bytes_read: u64,
    invalid_json: usize,
    invalid_capture: usize,
    irrelevant_json: usize,
    non_json_files: usize,
    oversized_files: usize,
    unreadable_paths: usize,
    symlinks_skipped: usize,
    depth_limited_directories: usize,
    champion_filtered: usize,
    role_filtered: usize,
    entry_or_file_limit_reached: bool,
    byte_limit_reached: bool,
}

fn collect_files(
    path: &Path,
    depth: usize,
    explicit: bool,
    paths: &mut Vec<PathBuf>,
    counts: &mut LoadCounts,
) {
    if counts.directory_entries >= MAX_ENTRIES || paths.len() >= MAX_FILES {
        counts.entry_or_file_limit_reached = true;
        return;
    }
    counts.directory_entries += 1;
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        counts.unreadable_paths += 1;
        return;
    };
    if metadata.file_type().is_symlink() {
        counts.symlinks_skipped += 1;
        return;
    }
    if metadata.is_file() {
        if explicit
            || path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        {
            paths.push(path.to_path_buf());
        } else {
            counts.non_json_files += 1;
        }
    } else if metadata.is_dir() {
        if depth > MAX_DEPTH {
            counts.depth_limited_directories += 1;
            return;
        }
        let Ok(entries) = std::fs::read_dir(path) else {
            counts.unreadable_paths += 1;
            return;
        };
        let remaining = MAX_ENTRIES.saturating_sub(counts.directory_entries);
        let mut entries: Vec<_> = entries.take(remaining.saturating_add(1)).collect();
        if entries.len() > remaining {
            counts.entry_or_file_limit_reached = true;
            entries.truncate(remaining);
        }
        entries.sort_by_key(|entry| entry.as_ref().ok().map(|entry| entry.path()));
        for entry in entries {
            match entry {
                Ok(entry) => collect_files(&entry.path(), depth + 1, false, paths, counts),
                Err(_) => counts.unreadable_paths += 1,
            }
            if counts.directory_entries >= MAX_ENTRIES || paths.len() >= MAX_FILES {
                counts.entry_or_file_limit_reached = true;
                break;
            }
        }
    }
}

fn read_bytes(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .with_context(|| format!("cannot read {}", path.display()))?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        bail!("{} exceeds its input size limit", path.display());
    }
    Ok(bytes)
}

fn read_json(path: &Path) -> Result<Value> {
    serde_json::from_slice(&read_bytes(path, MAX_DATA_BYTES)?)
        .with_context(|| format!("invalid JSON in {}", path.display()))
}

fn load_aggregate(
    path: &Path,
    requested_role: Option<Position>,
    declared_role: Option<Position>,
) -> Result<Aggregate> {
    let raw = read_json(path)?;
    let value = raw.get("payload").unwrap_or(&raw);
    if value.get("champion_key").is_some() {
        let decoded: Aggregate = serde_json::from_value(value.clone())?;
        if requested_role.is_some_and(|role| role != engine::actual_position(&decoded)) {
            bail!("aggregate does not cover the requested role");
        }
        return Ok(decoded);
    }
    let data = value.get("data").unwrap_or(value);
    let key = data["summary"]["id"]
        .as_u64()
        .and_then(|key| u32::try_from(key).ok())
        .ok_or_else(|| anyhow!("aggregate has no champion identity"))?;
    let positions = aggregate::decode_positions(&data["summary"]);
    // A raw response never says which role it was fetched for; only a cache record's request
    // URL does. Without that, the requested role must be one the file can stand for.
    let file_role = raw
        .get("source_url")
        .and_then(Value::as_str)
        .and_then(|url| {
            let path = url.split('?').next().unwrap_or(url);
            path.rsplit('/').next().and_then(Position::parse)
        })
        .or(declared_role)
        .filter(|role| positions.iter().any(|p| p.position == *role && p.games > 0));
    let exact = aggregate::choose_position(requested_role, &positions);
    let (role, fallback) = match (file_role, exact, requested_role) {
        (Some(file), _, Some(requested)) if file != requested => {
            if exact.is_some() {
                bail!(
                    "this file holds {} data, not the requested {} role",
                    file.label(),
                    requested.label()
                );
            }
            // Same-champion fallback: the assigned role is kept separately from the data's role.
            (file, true)
        }
        (Some(file), _, _) => (file, false),
        (None, Some(role), _) => (role, false),
        (None, None, _) => bail!("aggregate has no games for the requested role"),
    };
    // Raw responses omit the request population. Do not relabel an arbitrary file global/emerald+.
    let mut decoded = aggregate::decode(value, key, role, "unknown", "unknown")?;
    if fallback {
        decoded.requested_position = requested_role;
    }
    Ok(decoded)
}

struct ReplayCase {
    source: String,
    session: String,
    scenario: &'static str,
    champion: String,
    aggregate_index: Option<usize>,
    live: Option<LiveSnapshot>,
}

fn sort_cases(cases: &mut [ReplayCase]) {
    cases.sort_by(|a, b| {
        a.session
            .cmp(&b.session)
            .then_with(|| {
                a.live
                    .as_ref()
                    .map(|s| s.game_time)
                    .unwrap_or(-1.0)
                    .total_cmp(&b.live.as_ref().map(|s| s.game_time).unwrap_or(-1.0))
            })
            .then_with(|| a.source.cmp(&b.source))
    });
}

fn same_champion(cat: &Catalog, a: &str, b: &str) -> bool {
    match (cat.champion_key(a), cat.champion_key(b)) {
        (Some(a), Some(b)) => a == b,
        _ => normalize(a) == normalize(b),
    }
}

fn capture_cases(
    options: &Options,
    cat: &Catalog,
    aggregate: Option<&Aggregate>,
    counts: &mut LoadCounts,
) -> Result<Vec<ReplayCase>> {
    let root = options.session.as_ref().expect("validated session path");
    if !root.exists() {
        bail!("session path does not exist: {}", root.display());
    }
    let mut paths = Vec::new();
    collect_files(root, 0, true, &mut paths, counts);
    paths.sort();
    let mut cases = Vec::new();
    for path in paths {
        let Ok(metadata) = std::fs::metadata(&path) else {
            counts.unreadable_paths += 1;
            continue;
        };
        if metadata.len() > MAX_CAPTURE_BYTES {
            counts.oversized_files += 1;
            continue;
        }
        if counts.bytes_read.saturating_add(metadata.len()) > MAX_TOTAL_CAPTURE_BYTES {
            counts.byte_limit_reached = true;
            continue;
        }
        let limit = MAX_CAPTURE_BYTES.min(MAX_TOTAL_CAPTURE_BYTES - counts.bytes_read);
        let Ok(bytes) = read_bytes(&path, limit) else {
            counts.unreadable_paths += 1;
            continue;
        };
        counts.files_read += 1;
        counts.bytes_read += bytes.len() as u64;
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
            counts.invalid_json += 1;
            continue;
        };
        let snapshot = match parse_snapshot(&value) {
            Ok(Some(snapshot)) => snapshot,
            Ok(None) => {
                counts.irrelevant_json += 1;
                continue;
            }
            Err(_) => {
                counts.invalid_capture += 1;
                continue;
            }
        };
        if let Some(me) = &snapshot.me {
            if options
                .champion
                .as_ref()
                .is_some_and(|name| !same_champion(cat, name, &me.player.champion))
            {
                counts.champion_filtered += 1;
                continue;
            }
            if options
                .role
                .zip(Position::parse(&me.player.position))
                .is_some_and(|(requested, observed)| requested != observed)
            {
                counts.role_filtered += 1;
                continue;
            }
        }
        let champion = snapshot
            .me
            .as_ref()
            .map(|me| me.player.champion.clone())
            .or_else(|| options.champion.clone())
            .or_else(|| {
                aggregate
                    .and_then(|a| cat.champion(a.champion_key))
                    .map(|c| c.name.clone())
            })
            .unwrap_or_default();
        cases.push(ReplayCase {
            source: path.to_string_lossy().into_owned(),
            session: path.parent().unwrap_or(root).to_string_lossy().into_owned(),
            scenario: "recorded_visible_state",
            champion,
            aggregate_index: aggregate.map(|_| 0),
            live: Some(snapshot),
        });
    }
    sort_cases(&mut cases);
    Ok(cases)
}

const FIXTURES: &[(u32, &str, Position, &str)] = &[
    (498, "Xayah", Position::Adc, "opgg_xayah_adc.json"),
    (103, "Ahri", Position::Mid, "opgg_ahri_mid.json"),
    (516, "Ornn", Position::Top, "opgg_ornn_top.json"),
    (122, "Darius", Position::Top, "opgg_darius_top.json"),
    (117, "Lulu", Position::Support, "opgg_lulu_support.json"),
    (64, "Lee Sin", Position::Jungle, "opgg_leesin_jungle.json"),
    (523, "Aphelios", Position::Adc, "opgg_aphelios_adc.json"),
    (77, "Udyr", Position::Jungle, "opgg_udyr_jungle.json"),
];

fn fixture_cases(
    options: &Options,
    cat: &Catalog,
    directory: &Path,
    aggregates: &mut Vec<Aggregate>,
    warnings: &mut Vec<String>,
) -> Vec<ReplayCase> {
    let mut cases = Vec::new();
    for &(key, champion, role, filename) in FIXTURES {
        if options
            .champion
            .as_ref()
            .is_some_and(|name| !same_champion(cat, name, champion))
            || options.role.is_some_and(|wanted| wanted != role)
        {
            continue;
        }
        let path = directory.join(filename);
        let aggregate_index = match load_aggregate(&path, Some(role), None) {
            Ok(a) if a.champion_key == key => {
                aggregates.push(a);
                Some(aggregates.len() - 1)
            }
            Ok(_) => {
                warnings.push(format!("Wrong champion in {}", path.display()));
                None
            }
            Err(error) => {
                warnings.push(format!("Aggregate unavailable for {champion}: {error}"));
                None
            }
        };
        let source = path.to_string_lossy().into_owned();
        let session = format!("fixture:{champion}:{}", role.slug());
        cases.push(ReplayCase {
            source: source.clone(),
            session: session.clone(),
            scenario: "pregame_with_real_aggregate",
            champion: champion.into(),
            aggregate_index,
            live: None,
        });
        let aggregate = aggregate_index.and_then(|i| aggregates.get(i));
        for (scenario, time, level, gold) in [
            ("synthetic_opening", 30.0, 1, 500.0),
            ("synthetic_progression", 900.0, 9, 1400.0),
        ] {
            let mut items = Vec::new();
            if level > 1 {
                if let Some(a) = aggregate {
                    if let Some(id) = a
                        .core
                        .ids
                        .iter()
                        .copied()
                        .find(|id| cat.item(*id).is_some_and(|i| i.is_finished(cat)))
                    {
                        items.push(id);
                    }
                    if let Some(id) = a
                        .boots
                        .as_ref()
                        .and_then(|b| b.ids.first())
                        .copied()
                        .filter(|id| shop::compatible(cat, *id, &items))
                    {
                        items.push(id);
                    }
                }
            }
            let me = Me {
                player: Player {
                    name: "synthetic-replay-player".into(),
                    champion: champion.into(),
                    team: "ORDER".into(),
                    position: role.slug().into(),
                    level,
                    items: items
                        .into_iter()
                        .enumerate()
                        .map(|(slot, id)| InvItem {
                            id,
                            name: cat.item_name(id),
                            count: 1,
                            slot: slot as u32,
                        })
                        .collect(),
                    ..Default::default()
                },
                gold,
                spell_ids: aggregate.map(|a| a.spells.ids.clone()).unwrap_or_default(),
                ..Default::default()
            };
            cases.push(ReplayCase {
                source: source.clone(),
                session: session.clone(),
                scenario,
                champion: champion.into(),
                aggregate_index,
                live: Some(LiveSnapshot {
                    game_time: time,
                    mode: "CLASSIC".into(),
                    me: Some(me),
                    ..Default::default()
                }),
            });
        }
    }
    sort_cases(&mut cases);
    cases
}

fn owns_equivalent(cat: &Catalog, inventory: &[InvItem], target: u32) -> bool {
    inventory.iter().filter(|item| item.count > 0).any(|item| {
        let mut current = Some(item.id);
        for _ in 0..32 {
            let Some(id) = current else { break };
            if id == target || (id == 2422 && target == 1001) {
                return true;
            }
            current = cat.item(id).and_then(|item| item.special_recipe);
        }
        false
    })
}

#[derive(Debug, Serialize)]
struct Violation {
    kind: &'static str,
    message: String,
}

#[derive(Debug, Default, Serialize)]
struct Assessment {
    quote_checked: bool,
    target_legal: bool,
    target_owned: bool,
    target_blocked: bool,
    target_unpriced: bool,
    free_affordable_action: bool,
    unknown_inventory_items: usize,
    violations: Vec<Violation>,
}

impl Assessment {
    fn fail(&mut self, kind: &'static str, message: impl Into<String>) {
        self.violations.push(Violation {
            kind,
            message: message.into(),
        });
    }
}

fn component_key(component: &shop::ShopComponent) -> (u32, u32, bool) {
    (component.id, component.cost, component.owned)
}

fn validate_plan(plan: &Plan, cat: &Catalog, snapshot: Option<&LiveSnapshot>) -> Assessment {
    let mut assessment = Assessment::default();
    let me = snapshot.and_then(|snapshot| snapshot.me.as_ref());
    let inventory = me.map(|me| me.player.items.as_slice()).unwrap_or(&[]);
    let gold = me.map(|me| me.gold).unwrap_or(0.0);
    let boots_locked = me
        .is_some_and(|me| me.rune_ids.as_ref().is_some_and(|ids| ids.contains(&8304)))
        && !inventory
            .iter()
            .any(|item| item.count > 0 && cat.item(item.id).is_some_and(|item| item.effects.boots));
    let swiftplay = snapshot.is_some_and(|snapshot| {
        engine::GameMode::parse(&snapshot.mode) == engine::GameMode::Swiftplay
    });
    let context = shop::ShopContext {
        champion: me.map(|me| me.player.champion.as_str()),
        spell_ids: me
            .filter(|me| !me.spell_ids.is_empty())
            .map(|me| me.spell_ids.as_slice()),
        boots_locked,
        swiftplay,
    };
    // A future boot upgrade can be legal even before Magical Footwear arrives.
    // Only pregame planning may use the proposed spell page for compatibility.
    let path_context = shop::ShopContext {
        champion: context
            .champion
            .or_else(|| (!plan.champion.is_empty()).then_some(plan.champion.as_str())),
        spell_ids: context
            .spell_ids
            .or_else(|| snapshot.is_none().then_some(plan.spell_ids.as_slice())),
        boots_locked: false,
        swiftplay,
    };
    let immutable: Vec<_> = inventory
        .iter()
        .filter(|owned| owned.count > 0)
        .filter(|owned| {
            cat.item(owned.id)
                .is_some_and(|item| item.is_owned_commitment(cat) || item.effects.boots)
        })
        .map(|owned| owned.id)
        .collect();
    assessment.unknown_inventory_items = inventory
        .iter()
        .filter(|owned| cat.item(owned.id).is_none())
        .count();
    if snapshot.is_some() && me.is_none() && plan.next.is_some() {
        assessment.fail(
            "identity",
            "purchase advice was produced without the player's live identity",
        );
    }
    for score in &plan.score_trace {
        if [
            score.prior,
            score.completion,
            score.situation,
            score.delay,
            score.total,
        ]
        .iter()
        .any(|score| !score.is_finite())
        {
            assessment.fail("score", format!("nonfinite score for item {}", score.id));
        }
    }
    let mut future = Vec::new();
    for item in &plan.path {
        let Some(catalog_item) = cat.item(item.id) else {
            continue;
        };
        if item.owned {
            if me.is_some() && !owns_equivalent(cat, inventory, item.id) {
                assessment.fail(
                    "ownership",
                    format!("path marks unowned item {} as owned", item.id),
                );
            }
            continue;
        }
        if !shop::compatible_with_context(cat, item.id, &immutable, &path_context) {
            assessment.fail(
                "path",
                format!(
                    "future item {} conflicts with immutable owned equipment or a shop restriction",
                    item.id
                ),
            );
        }
        for &earlier in &future {
            if !shop::compatible_with_context(cat, item.id, &[earlier], &path_context)
                && !shop::compatible_with_context(cat, earlier, &[item.id], &path_context)
            {
                assessment.fail(
                    "path",
                    format!("future items {earlier} and {} cannot coexist", item.id),
                );
            }
        }
        if catalog_item.is_finished(cat) || catalog_item.effects.boots {
            future.push(item.id);
        }
    }
    let Some(next) = &plan.next else {
        return assessment;
    };
    let quote = shop::quote_with_context(cat, next.id, inventory, gold, &context);
    assessment.quote_checked = true;
    assessment.target_owned = cat.item(next.id).is_some_and(|item| item.max_stacks <= 1)
        && owns_equivalent(cat, inventory, next.id);
    assessment.target_blocked = quote.blocked.is_some();
    assessment.target_legal = quote.blocked.is_none();
    assessment.target_unpriced = quote.remaining_cost.is_none();
    let mut price_errors = Vec::new();
    if next.price_known != quote.remaining_cost.is_some()
        || (next.price_known && quote.remaining_cost != Some(next.remaining_cost))
    {
        price_errors.push(format!(
            "remaining price {} (known={}) differs from quote {:?}",
            next.remaining_cost, next.price_known, quote.remaining_cost
        ));
    }
    if cat
        .item(next.id)
        .is_some_and(|item| item.price_known && item.total != next.cost)
    {
        price_errors.push("displayed full price differs from catalog".into());
    }
    if next
        .components
        .iter()
        .map(component_key)
        .collect::<Vec<_>>()
        != quote
            .components
            .iter()
            .map(component_key)
            .collect::<Vec<_>>()
    {
        price_errors
            .push("displayed recipe components differ from the inventory-aware quote".into());
    }
    let expected_hint = if quote.blocked.is_some() {
        None
    } else {
        quote
            .buy_now
            .as_ref()
            .or(quote.save_for.as_ref())
            .map(component_key)
            .or_else(|| {
                quote
                    .components
                    .iter()
                    .filter(|component| !component.owned)
                    .min_by_key(|component| (component.cost, component.id))
                    .map(component_key)
            })
            .or_else(|| quote.remaining_cost.map(|cost| (next.id, cost, false)))
    };
    if next
        .buy_now
        .as_ref()
        .is_some_and(|action| expected_hint != Some(component_key(action)))
    {
        price_errors.push("purchase or saving hint differs from the legal quoted action".into());
    }
    let expected_gap = next
        .buy_now
        .as_ref()
        .filter(|_| gold.is_finite())
        .and_then(|action| {
            (f64::from(action.cost) > gold).then(|| (f64::from(action.cost) - gold).ceil() as u32)
        });
    if next.save_gap != expected_gap {
        price_errors.push("saving gap differs from current gold and the quoted action".into());
    }
    let basket_sum: u64 = next.basket.iter().map(|item| u64::from(item.cost)).sum();
    if basket_sum != u64::from(next.basket_cost) {
        price_errors.push("basket price differs from the sum of its actions".into());
    }
    if !price_errors.is_empty() {
        assessment.fail(
            "price",
            format!("target {}: {}", next.id, price_errors.join("; ")),
        );
    }
    if next.buy_now_affordable {
        let valid = next.buy_now.as_ref().is_some_and(|action| {
            !action.owned
                && f64::from(action.cost) <= gold
                && quote.blocked.is_none()
                && quote.buy_now.as_ref().map(component_key) == Some(component_key(action))
        });
        if !valid {
            assessment.fail(
                "affordable_action",
                format!(
                    "target {} claims an affordable action absent from its legal quote",
                    next.id
                ),
            );
        } else {
            assessment.free_affordable_action =
                next.buy_now.as_ref().is_some_and(|action| action.cost == 0);
        }
    }
    if !next.basket.is_empty()
        && (quote.blocked.is_some()
            || !gold.is_finite()
            || basket_sum as f64 > gold
            || next.basket.iter().map(component_key).collect::<Vec<_>>()
                != quote.basket.iter().map(component_key).collect::<Vec<_>>())
    {
        assessment.fail(
            "affordable_basket",
            format!(
                "target {} exposes a basket that differs from the legal affordable quote",
                next.id
            ),
        );
    }
    assessment
}

fn baseline_deviation(
    plan: &Plan,
    cat: &Catalog,
    aggregate: Option<&Aggregate>,
    snapshot: Option<&LiveSnapshot>,
) -> Option<bool> {
    let next = plan.next.as_ref()?;
    if cat.item(next.id)?.effects.boots || snapshot.is_some_and(|s| s.game_time < 90.0) {
        return None;
    }
    let a = aggregate?;
    let core = if a.core_most_picked.ids.is_empty() {
        &a.core.ids
    } else {
        &a.core_most_picked.ids
    };
    let inventory = snapshot
        .and_then(|s| s.me.as_ref())
        .map(|me| me.player.items.as_slice())
        .unwrap_or(&[]);
    let baseline = core.iter().copied().find(|id| {
        cat.item(*id).is_some_and(|item| item.is_finished(cat))
            && !owns_equivalent(cat, inventory, *id)
    })?;
    Some(next.id != baseline)
}

#[derive(Serialize)]
struct SnapshotContext {
    identity_resolved: bool,
    mode: String,
    level: Option<u32>,
    gold: Option<f64>,
    inventory: Vec<InventoryContext>,
}

#[derive(Serialize)]
struct InventoryContext {
    id: u32,
    count: u32,
    slot: u32,
}

fn snapshot_context(snapshot: &LiveSnapshot) -> SnapshotContext {
    SnapshotContext {
        identity_resolved: snapshot.me.is_some(),
        mode: snapshot.mode.clone(),
        level: snapshot.me.as_ref().map(|me| me.player.level),
        gold: snapshot.me.as_ref().map(|me| me.gold),
        inventory: snapshot
            .me
            .as_ref()
            .map(|me| {
                me.player
                    .items
                    .iter()
                    .map(|item| InventoryContext {
                        id: item.id,
                        count: item.count,
                        slot: item.slot,
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

#[derive(Serialize)]
struct CaseResult {
    source: String,
    session: String,
    scenario: &'static str,
    champion: String,
    role: Option<Position>,
    game_time: Option<f64>,
    source_patch: Option<String>,
    aggregate_available: bool,
    missing_identity: bool,
    unsupported_mode: bool,
    next_target: Option<u32>,
    buy_now: Option<shop::ShopComponent>,
    buy_now_affordable: bool,
    basket: Vec<shop::ShopComponent>,
    path: Vec<u32>,
    note: Option<String>,
    target_changed: bool,
    action_changed: bool,
    baseline_deviation: Option<bool>,
    plan_latency_us: f64,
    assessment: Assessment,
    snapshot: Option<SnapshotContext>,
    plan: Plan,
}

#[derive(PartialEq)]
struct ActionSignature {
    target: Option<u32>,
    buy: Option<(u32, u32, bool)>,
    affordable: bool,
    basket: Vec<(u32, u32, bool)>,
}

fn run(options: &Options) -> Result<Value> {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../m0/tests/fixtures");
    let items = read_json(
        options
            .items
            .as_deref()
            .unwrap_or(&directory.join("item_subset.json")),
    )?;
    let champions = read_json(
        options
            .champions
            .as_deref()
            .unwrap_or(&directory.join("champion_subset.json")),
    )?;
    let runes = options
        .runes
        .as_deref()
        .map(read_json)
        .transpose()?
        .unwrap_or_else(|| json!([]));
    let version = items["version"].as_str().unwrap_or("unknown");
    let cat = Catalog::from_json(version, &items, &champions, &runes);
    let traits = pack::load_traits()?;
    let mut aggregates = Vec::new();
    let mut warnings = Vec::new();
    let mut loading = LoadCounts::default();
    let cases = if options.fixtures {
        fixture_cases(options, &cat, &directory, &mut aggregates, &mut warnings)
    } else {
        match load_aggregate(
            options
                .aggregate
                .as_deref()
                .expect("validated aggregate path"),
            options.role,
            options.aggregate_role,
        ) {
            Ok(a) => aggregates.push(a),
            Err(error) => warnings.push(format!(
                "Aggregate unavailable; replay will check paused plans: {error}"
            )),
        }
        capture_cases(options, &cat, aggregates.first(), &mut loading)?
    };
    if cat.items.is_empty() || cat.champions.is_empty() {
        warnings.push(
            "The supplied catalog is incomplete; missing data may pause recommendations.".into(),
        );
    }
    let mut records = Vec::new();
    let mut prior: BTreeMap<String, ActionSignature> = BTreeMap::new();
    // The shell carries the plan's effective preferences (pin state, offered and declined
    // detours) from one poll to the next; replay does the same within a session.
    let mut carried: BTreeMap<String, engine::PlannerPreferences> = BTreeMap::new();
    let mut duplicate_times = 0;
    let mut seen_times = BTreeSet::new();
    for case in cases {
        let aggregate = case.aggregate_index.and_then(|i| aggregates.get(i));
        let enemies: Vec<_> = case
            .live
            .as_ref()
            .map(|s| s.enemies.iter().map(|p| p.champion.clone()).collect())
            .unwrap_or_default();
        let inputs = Inputs {
            champion: &case.champion,
            pack: None,
            aggregate,
            traits: &traits,
            catalog: &cat,
            enemies: &enemies,
            live: case.live.as_ref(),
        };
        let preferences = carried.remove(&case.session).unwrap_or_default();
        let start = Instant::now();
        let plan = engine::plan_with_preferences(&inputs, &preferences);
        carried.insert(case.session.clone(), plan.preferences.clone());
        let latency = start.elapsed().as_secs_f64() * 1_000_000.0;
        let assessment = validate_plan(&plan, &cat, case.live.as_ref());
        let signature = ActionSignature {
            target: plan.next.as_ref().map(|n| n.id),
            buy: plan
                .next
                .as_ref()
                .and_then(|n| n.buy_now.as_ref())
                .map(component_key),
            affordable: plan.next.as_ref().is_some_and(|n| n.buy_now_affordable),
            basket: plan
                .next
                .as_ref()
                .map(|n| n.basket.iter().map(component_key).collect())
                .unwrap_or_default(),
        };
        let target_changed = prior
            .get(&case.session)
            .is_some_and(|previous| previous.target != signature.target);
        let action_changed = prior
            .get(&case.session)
            .is_some_and(|previous| *previous != signature);
        prior.insert(case.session.clone(), signature);
        if let Some(live) = &case.live {
            if !seen_times.insert((case.session.clone(), live.game_time.to_bits())) {
                duplicate_times += 1;
            }
        }
        let missing_identity = case.live.as_ref().is_some_and(|s| s.me.is_none());
        let unsupported_mode = case.live.as_ref().is_some_and(|s| {
            !["classic", "swiftplay", "practicetool"].contains(&normalize(&s.mode).as_str())
        });
        let observed_role = case
            .live
            .as_ref()
            .and_then(|s| s.me.as_ref())
            .and_then(|m| Position::parse(&m.player.position));
        let aggregate_available = aggregate.is_some_and(|a| {
            cat.champion_key(&case.champion) == Some(a.champion_key)
                && observed_role.is_none_or(|role| role == engine::actual_position(a))
        });
        records.push(CaseResult {
            source: case.source,
            session: case.session,
            scenario: case.scenario,
            champion: case.champion,
            role: observed_role
                .or(options.role)
                .or_else(|| aggregate.map(|a| a.position)),
            game_time: case.live.as_ref().map(|s| s.game_time),
            source_patch: aggregate.map(|a| a.patch.clone()),
            aggregate_available,
            missing_identity,
            unsupported_mode,
            next_target: plan.next.as_ref().map(|n| n.id),
            buy_now: plan.next.as_ref().and_then(|n| n.buy_now.clone()),
            buy_now_affordable: plan.next.as_ref().is_some_and(|n| n.buy_now_affordable),
            basket: plan
                .next
                .as_ref()
                .map(|n| n.basket.clone())
                .unwrap_or_default(),
            path: plan.path.iter().map(|item| item.id).collect(),
            note: plan.note.clone(),
            target_changed,
            action_changed,
            baseline_deviation: baseline_deviation(
                &plan,
                &cat,
                aggregate.filter(|_| aggregate_available),
                case.live.as_ref(),
            ),
            plan_latency_us: latency,
            assessment,
            snapshot: case.live.as_ref().map(snapshot_context),
            plan,
        });
    }
    let latencies: Vec<_> = records.iter().map(|r| r.plan_latency_us).collect();
    let mut violation_counts = BTreeMap::new();
    for violation in records.iter().flat_map(|r| &r.assessment.violations) {
        *violation_counts.entry(violation.kind).or_insert(0_usize) += 1;
    }
    let invalid_cases = records
        .iter()
        .filter(|r| !r.assessment.violations.is_empty())
        .count();
    let source_patches: BTreeSet<_> = aggregates.iter().map(|a| a.patch.as_str()).collect();
    let sessions: BTreeSet<_> = records.iter().map(|r| r.session.as_str()).collect();
    Ok(json!({
        "schema_version": 1, "engine_version": engine::ENGINE_VERSION,
        "mode": if options.fixtures { "synthetic_states_with_real_aggregates" } else { "recorded_visible_state" },
        "session_grouping": if options.fixtures { "synthetic champion scenario groups; not matches" } else { "capture parent directories; unverified match boundaries" },
        "evidence_note": "Snapshots and aggregate observations are not independent experiments. No causal uplift or match outcomes are inferred.",
        "catalog_patch": cat.version, "source_patches": source_patches,
        "sessions": sessions.len(), "plans": records.len(), "snapshots": records.iter().filter(|r| r.game_time.is_some()).count(),
        "pregame_plans": records.iter().filter(|r| r.game_time.is_none()).count(), "duplicate_game_times": duplicate_times,
        "plans_with_next": records.iter().filter(|r| r.next_target.is_some()).count(),
        "missing_identity": records.iter().filter(|r| r.missing_identity).count(),
        "aggregate_unavailable": records.iter().filter(|r| !r.aggregate_available).count(),
        "unsupported_mode": records.iter().filter(|r| r.unsupported_mode).count(),
        "target_changes": records.iter().filter(|r| r.target_changed).count(), "action_changes": records.iter().filter(|r| r.action_changed).count(),
        "changes_include_pauses": true,
        "quote_checks": records.iter().filter(|r| r.assessment.quote_checked).count(),
        "legal_targets": records.iter().filter(|r| r.assessment.target_legal).count(),
        "owned_targets": records.iter().filter(|r| r.assessment.target_owned).count(),
        "blocked_targets": records.iter().filter(|r| r.assessment.target_blocked).count(),
        "unpriced_targets": records.iter().filter(|r| r.assessment.target_unpriced).count(),
        "free_affordable_actions": records.iter().filter(|r| r.assessment.free_affordable_action).count(),
        "invalid_recommendation_cases": invalid_cases, "violation_counts": violation_counts,
        "baseline_deviation": {"deviations": records.iter().filter(|r| r.baseline_deviation == Some(true)).count(),
            "eligible": records.iter().filter(|r| r.baseline_deviation.is_some()).count(),
            "definition": "Next target differs from first unowned popular core item; excludes boots and opening purchases. Descriptive, not a success objective."},
        "plan_latency_us": {"method":"nearest_rank", "samples":latencies.len(), "p50":quantile(&latencies, 0.5),
            "p95":quantile(&latencies, 0.95), "p99":quantile(&latencies, 0.99), "max":quantile(&latencies, 1.0)},
        "loading": loading, "warnings": warnings, "records": records,
    }))
}

fn print_text(report: &Value) {
    println!(
        "Recall replay: {} plans / {} snapshots / {} session groups",
        report["plans"], report["snapshots"], report["sessions"]
    );
    println!("{}", report["session_grouping"].as_str().unwrap_or(""));
    println!(
        "Mode: {}. Source patches {}; catalog {}.",
        report["mode"].as_str().unwrap_or(""),
        report["source_patches"],
        report["catalog_patch"].as_str().unwrap_or("unknown")
    );
    println!(
        "Next target: {} plans; {} target changes; {} action changes (including pauses).",
        report["plans_with_next"], report["target_changes"], report["action_changes"]
    );
    println!(
        "Validation: {} quote checks; {} invalid recommendation cases. Violations: {}",
        report["quote_checks"], report["invalid_recommendation_cases"], report["violation_counts"]
    );
    println!(
        "Targets: {} legal / {} owned / {} blocked / {} unpriced; {} free affordable actions.",
        report["legal_targets"],
        report["owned_targets"],
        report["blocked_targets"],
        report["unpriced_targets"],
        report["free_affordable_actions"]
    );
    println!(
        "Planner latency (microseconds, nearest rank): {}",
        report["plan_latency_us"]
    );
    println!(
        "Baseline deviation: {}/{} eligible decisions (descriptive, not a success objective).",
        report["baseline_deviation"]["deviations"], report["baseline_deviation"]["eligible"]
    );
    println!(
        "Paused/data gaps: {} missing identities; {} unavailable aggregates; {} unsupported modes.",
        report["missing_identity"], report["aggregate_unavailable"], report["unsupported_mode"]
    );
    println!("Loading/skips: {}", report["loading"]);
    for warning in report["warnings"].as_array().into_iter().flatten() {
        println!("Warning: {}", warning.as_str().unwrap_or(""));
    }
    let mut shown = 0;
    for record in report["records"].as_array().into_iter().flatten() {
        for violation in record["assessment"]["violations"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if shown < 30 {
                println!(
                    "{} @ {}: {}",
                    record["source"].as_str().unwrap_or(""),
                    record["game_time"],
                    violation["message"].as_str().unwrap_or("")
                );
            }
            shown += 1;
        }
    }
    if shown > 30 {
        println!(
            "{} further violations are available with --json.",
            shown - 30
        );
    }
    println!("{}", report["evidence_note"].as_str().unwrap_or(""));
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json_output = args.iter().any(|arg| arg == "--json");
    let result = parse_args(&args).and_then(|options| {
        if options.help {
            println!("{HELP}");
            return Ok(0);
        }
        let report = run(&options)?;
        if options.json {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            print_text(&report);
        }
        Ok(
            if report["invalid_recommendation_cases"].as_u64().unwrap_or(0) > 0 {
                1
            } else {
                0
            },
        )
    });
    match result {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            if json_output {
                println!("{}", json!({"error": format!("{error:#}"), "exit_code":2}));
            } else {
                println!("Replay input error: {error:#}\nUse --help for usage.");
            }
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn catalog() -> Catalog {
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../m0/tests/fixtures");
        let items = read_json(&directory.join("item_subset.json")).unwrap();
        let champions = read_json(&directory.join("champion_subset.json")).unwrap();
        Catalog::from_json("16.17.1", &items, &champions, &json!([]))
    }

    fn visible_state(gold: f64, ids: &[u32]) -> LiveSnapshot {
        LiveSnapshot {
            game_time: 900.0,
            mode: "CLASSIC".into(),
            me: Some(Me {
                gold,
                player: Player {
                    champion: "Xayah".into(),
                    level: 9,
                    items: ids
                        .iter()
                        .enumerate()
                        .map(|(slot, &id)| InvItem {
                            id,
                            count: 1,
                            slot: slot as u32,
                            ..Default::default()
                        })
                        .collect(),
                    ..Default::default()
                },
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn target_plan(cat: &Catalog, target: u32, snapshot: &LiveSnapshot) -> Plan {
        let item = cat.item(target).unwrap();
        let path = vec![engine::PlanItem {
            id: target,
            name: item.name.clone(),
            cost: item.total,
            ..Default::default()
        }];
        Plan {
            next: engine::next_item(cat, None, &path, snapshot.me.as_ref(), false, false),
            path,
            ..Default::default()
        }
    }

    #[test]
    fn parses_explicit_capture_inputs_without_inventing_a_role() {
        let options = parse_args(&args(&[
            "--session",
            "captures",
            "--items",
            "items.json",
            "--champions",
            "champions.json",
            "--aggregate",
            "aggregate.json",
            "--champion",
            "Ahri",
            "--role",
            "middle",
            "--json",
        ]))
        .unwrap();
        assert_eq!(options.session, Some(PathBuf::from("captures")));
        assert_eq!(options.role, Some(Position::Mid));
        assert_eq!(options.champion.as_deref(), Some("Ahri"));
        assert!(options.json);
        assert!(options.runes.is_none());
    }

    #[test]
    fn rejects_ambiguous_modes_unknown_flags_and_missing_values() {
        for values in [
            args(&["--fixtures", "--session", "captures"]),
            args(&["--fixtures", "--network"]),
            args(&["--session"]),
            args(&["--fixtures", "--role", "river"]),
            args(&["--fixtures", "--aggregate-role", "river"]),
            args(&[
                "--fixtures",
                "--items",
                "first.json",
                "--items",
                "second.json",
            ]),
        ] {
            assert!(parse_args(&values).is_err(), "accepted {values:?}");
        }
        assert!(
            parse_args(&args(&["--fixtures", "--json"]))
                .unwrap()
                .fixtures
        );
        assert!(parse_args(&args(&["--help"])).unwrap().help);
    }

    #[test]
    fn latency_quantiles_use_nearest_rank_and_reject_invalid_measurements() {
        assert_eq!(quantile(&[5.0, 1.0, 4.0, 2.0, 3.0], 0.5), Some(3.0));
        assert_eq!(quantile(&[5.0, 1.0, 4.0, 2.0, 3.0], 0.95), Some(5.0));
        assert_eq!(quantile(&[7.0], 0.99), Some(7.0));
        assert_eq!(quantile(&[], 0.5), None);
        assert_eq!(quantile(&[f64::NAN, -1.0], 0.5), None);
        assert_eq!(quantile(&[1.0], 1.5), None);
    }

    #[test]
    fn a_missing_live_identity_remains_a_replayable_capture() {
        let value = json!({"gameData": {"gameTime": 42.5, "gameMode": "CLASSIC"}, "activePlayer": {}, "allPlayers": []});
        let snapshot = parse_snapshot(&value).unwrap().unwrap();
        assert_eq!(snapshot.game_time, 42.5);
        assert_eq!(snapshot.mode, "CLASSIC");
        assert!(snapshot.me.is_none());
    }

    #[test]
    fn snapshot_detection_separates_irrelevant_json_from_broken_capture_time() {
        assert!(parse_snapshot(&json!({"data": {"summary": {"id": 498}}}))
            .unwrap()
            .is_none());
        let broken = json!({"gameData": {"gameTime": -2.0}, "allPlayers": []});
        assert!(parse_snapshot(&broken).is_err());
        let absent = json!({"gameData": {}, "allPlayers": []});
        assert!(parse_snapshot(&absent).is_err());
    }

    #[test]
    fn fixture_records_include_production_plans_and_nonidentifying_snapshot_context() {
        let options = parse_args(&args(&["--fixtures", "--champion", "Ahri", "--json"])).unwrap();
        let report = run(&options).unwrap();
        let records = report["records"].as_array().unwrap();
        assert_eq!(records.len(), 3);
        for record in records {
            assert!(
                record["plan"].is_object(),
                "record must retain the complete production plan"
            );
            assert!(record["plan"]["path"].is_array());
            let ids: Vec<_> = record["plan"]["path"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item["id"].clone())
                .collect();
            assert_eq!(json!(ids), record["path"]);
        }
        assert!(records[0]["snapshot"].is_null());
        assert_eq!(records[1]["snapshot"]["identity_resolved"], true);
        assert_eq!(records[1]["snapshot"]["level"], 1);
        assert_eq!(records[1]["snapshot"]["gold"], 500.0);
        assert!(records[1]["snapshot"].get("name").is_none());
        assert!(records[1]["snapshot"].get("player").is_none());
        assert!(!report.to_string().contains("synthetic-replay-player"));
    }

    #[test]
    fn an_actual_smite_loadout_allows_a_legal_jungle_opening_action() {
        let cat = catalog();
        let mut snapshot = visible_state(500.0, &[]);
        let me = snapshot.me.as_mut().unwrap();
        me.player.champion = "Lee Sin".into();
        me.spell_ids = vec![4, 11];
        let plan = target_plan(&cat, 1101, &snapshot);
        assert_eq!(plan.next.as_ref().unwrap().remaining_cost, 450);
        assert!(plan.next.as_ref().unwrap().buy_now_affordable);
        let assessment = validate_plan(&plan, &cat, Some(&snapshot));
        assert!(assessment.violations.is_empty(), "{assessment:?}");
        assert!(assessment.target_legal);
        snapshot.me.as_mut().unwrap().spell_ids = vec![4, 14];
        let wrong_loadout = validate_plan(&plan, &cat, Some(&snapshot));
        assert!(wrong_loadout.target_blocked);
        assert!(wrong_loadout
            .violations
            .iter()
            .any(|violation| violation.kind == "affordable_action"));
    }

    #[test]
    fn exact_combine_cost_and_repeated_component_actions_are_checked_in_target_context() {
        let cat = catalog();
        let snapshot = visible_state(725.0, &[1038, 1037, 1018]);
        let plan = target_plan(&cat, 3031, &snapshot);
        assert_eq!(plan.next.as_ref().unwrap().remaining_cost, 725);
        let assessment = validate_plan(&plan, &cat, Some(&snapshot));
        assert!(assessment.violations.is_empty(), "{assessment:?}");

        let snapshot = visible_state(250.0, &[1042]);
        let plan = target_plan(&cat, 6675, &snapshot);
        let action = plan.next.as_ref().unwrap().buy_now.as_ref().unwrap();
        assert_eq!((action.id, action.cost), (1042, 250));
        let assessment = validate_plan(&plan, &cat, Some(&snapshot));
        assert!(
            assessment.violations.is_empty(),
            "a required second Dagger is legal: {assessment:?}"
        );
    }

    #[test]
    fn a_verified_support_quest_choice_is_free_not_an_unknown_price() {
        let cat = catalog();
        let snapshot = visible_state(0.0, &[3867]);
        let plan = target_plan(&cat, 3869, &snapshot);
        let next = plan.next.as_ref().unwrap();
        assert!(next.price_known && next.buy_now_affordable);
        assert_eq!((next.cost, next.remaining_cost), (400, 0));
        let assessment = validate_plan(&plan, &cat, Some(&snapshot));
        assert!(assessment.violations.is_empty(), "{assessment:?}");
        assert!(assessment.free_affordable_action);
        assert!(!assessment.target_unpriced);
    }

    #[test]
    fn wrong_prices_and_false_affordability_claims_are_actual_violations() {
        let cat = catalog();
        let snapshot = visible_state(725.0, &[1038, 1037, 1018]);
        let mut plan = target_plan(&cat, 3031, &snapshot);
        plan.next.as_mut().unwrap().remaining_cost = 724;
        let assessment = validate_plan(&plan, &cat, Some(&snapshot));
        assert!(assessment
            .violations
            .iter()
            .any(|violation| violation.kind == "price"));

        let snapshot = visible_state(724.0, &[1038, 1037, 1018]);
        let mut plan = target_plan(&cat, 3031, &snapshot);
        assert!(!plan.next.as_ref().unwrap().buy_now_affordable);
        plan.next.as_mut().unwrap().buy_now_affordable = true;
        let assessment = validate_plan(&plan, &cat, Some(&snapshot));
        assert!(assessment
            .violations
            .iter()
            .any(|violation| violation.kind == "affordable_action"));
    }

    #[test]
    fn owned_targets_and_missing_identity_pauses_are_reported_separately_from_illegal_actions() {
        let cat = catalog();
        let snapshot = visible_state(5000.0, &[3036]);
        let mut plan = target_plan(&cat, 3036, &snapshot);
        plan.path.clear();
        let assessment = validate_plan(&plan, &cat, Some(&snapshot));
        assert!(assessment.target_owned && assessment.target_blocked);
        assert!(
            assessment.violations.is_empty(),
            "no affordable action was claimed: {assessment:?}"
        );
        let missing_identity = LiveSnapshot {
            game_time: 42.0,
            mode: "CLASSIC".into(),
            ..Default::default()
        };
        assert!(
            validate_plan(&Plan::default(), &cat, Some(&missing_identity))
                .violations
                .is_empty()
        );
        assert!(validate_plan(&plan, &cat, Some(&missing_identity))
            .violations
            .iter()
            .any(|violation| violation.kind == "identity"));
    }

    #[test]
    fn immutable_items_and_nonfinite_scores_are_checked_even_when_no_buy_is_affordable() {
        let cat = catalog();
        for (owned, future) in [(3036, 3033), (3006, 3047)] {
            let snapshot = visible_state(0.0, &[owned]);
            let plan = target_plan(&cat, future, &snapshot);
            let assessment = validate_plan(&plan, &cat, Some(&snapshot));
            assert!(
                assessment
                    .violations
                    .iter()
                    .any(|violation| violation.kind == "path"),
                "{assessment:?}"
            );
        }
        let mut plan = Plan::default();
        plan.score_trace
            .push(recall_core::decision::CandidateScore {
                total: f64::NAN,
                ..Default::default()
            });
        let assessment = validate_plan(&plan, &cat, None);
        assert!(assessment
            .violations
            .iter()
            .any(|violation| violation.kind == "score"));
    }

    #[test]
    fn captures_sort_by_game_time_inside_folder_groups_not_filename_order() {
        let case = |session: &str, source: &str, time: f64| ReplayCase {
            source: source.into(),
            session: session.into(),
            scenario: "recorded_visible_state",
            champion: "Xayah".into(),
            aggregate_index: None,
            live: Some(LiveSnapshot {
                game_time: time,
                ..Default::default()
            }),
        };
        let mut cases = vec![
            case("second", "a.json", 1.0),
            case("first", "a.json", 20.0),
            case("first", "z.json", 10.0),
        ];
        sort_cases(&mut cases);
        let ordered: Vec<_> = cases
            .iter()
            .map(|case| (case.session.as_str(), case.live.as_ref().unwrap().game_time))
            .collect();
        assert_eq!(ordered, [("first", 10.0), ("first", 20.0), ("second", 1.0)]);
    }

    #[test]
    fn capture_loader_reports_irrelevant_files_and_never_relabels_an_observed_champion() {
        let cat = catalog();
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../m0/tests/fixtures");
        let options = Options {
            session: Some(directory),
            champion: Some("Ahri".into()),
            ..Default::default()
        };
        let mut counts = LoadCounts::default();
        let cases = capture_cases(&options, &cat, None, &mut counts).unwrap();
        assert!(counts.irrelevant_json > 0);
        assert!(counts.non_json_files > 0);
        assert!(counts.champion_filtered > 0);
        assert!(cases.iter().all(|case| case
            .live
            .as_ref()
            .and_then(|snapshot| snapshot.me.as_ref())
            .is_none_or(|me| same_champion(&cat, &me.player.champion, "Ahri"))));
    }

    #[test]
    fn depth_file_and_byte_limits_stop_input_work() {
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../m0/tests/fixtures");
        let mut paths = Vec::new();
        let mut counts = LoadCounts::default();
        collect_files(&directory, MAX_DEPTH + 1, false, &mut paths, &mut counts);
        assert!(paths.is_empty());
        assert_eq!(counts.depth_limited_directories, 1);
        counts.directory_entries = MAX_ENTRIES;
        collect_files(&directory, 0, true, &mut paths, &mut counts);
        assert!(paths.is_empty());
        assert!(counts.entry_or_file_limit_reached);
        assert!(read_bytes(&directory.join("allgamedata.json"), 10).is_err());
    }

    #[test]
    fn saving_hints_and_component_prices_must_match_the_inventory_aware_quote() {
        let cat = catalog();
        let snapshot = visible_state(0.0, &[1038]);
        let mut plan = target_plan(&cat, 3031, &snapshot);
        plan.next.as_mut().unwrap().buy_now.as_mut().unwrap().cost += 1;
        let assessment = validate_plan(&plan, &cat, Some(&snapshot));
        assert!(
            assessment
                .violations
                .iter()
                .any(|violation| violation.kind == "price"),
            "wrong saving hint was accepted: {assessment:?}"
        );
        let mut plan = target_plan(&cat, 3031, &snapshot);
        plan.next.as_mut().unwrap().components[0].cost += 1;
        let assessment = validate_plan(&plan, &cat, Some(&snapshot));
        assert!(
            assessment
                .violations
                .iter()
                .any(|violation| violation.kind == "price"),
            "wrong component price was accepted: {assessment:?}"
        );
    }
}
