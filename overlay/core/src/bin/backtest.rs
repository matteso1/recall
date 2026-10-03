//! Production planner evaluation on chronological, anonymous corpus observations.
//! Training artifacts are explicit and mandatory; embedded all-player priors cannot be used.
use anyhow::{bail, Context, Result};
use recall_core::{
    aggregate::Aggregate,
    ddragon::Catalog,
    engine::{self, Inputs, PlannerPreferences},
    live::LiveSnapshot,
    nextprior, pack,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader};
use std::path::Path;

#[path = "support/assessment.rs"]
mod assessment;

#[derive(Deserialize)]
struct Label {
    horizon: String,
    item: u32,
    kind: String,
    #[serde(default)]
    component: bool,
}

#[derive(Deserialize)]
struct Frame {
    snapshot: LiveSnapshot,
    labels: Vec<Label>,
    #[serde(default)]
    shop_items: Vec<u32>,
}

#[derive(Deserialize)]
struct Game {
    session: u64,
    champion: String,
    role: String,
    aggregate: String,
    enemies: Vec<String>,
    frames: Vec<Frame>,
}

type Counts = BTreeMap<String, u64>;

struct Previous {
    target: Option<u32>,
    inventory: Vec<(u32, u32)>,
    time: f64,
}

fn add(counts: &mut Counts, key: &str, value: bool) {
    *counts.entry(key.to_string()).or_default() += u64::from(value);
}

fn consecutive_minute(elapsed: f64) -> bool {
    (59.0..=61.0).contains(&elapsed)
}

// These are observed shopping windows, including negative examples. They do not
// assert that the player was in the shop, or had the same gold, at frame time.
fn score_answer(
    counts: &mut Counts,
    name: &str,
    predicted: Option<u32>,
    purchased: &[u32],
    is_answer: impl Fn(u32) -> bool,
) {
    if purchased.is_empty() {
        return;
    }
    let observed = purchased.iter().copied().any(&is_answer);
    let suggested = predicted.is_some_and(is_answer);
    add(counts, &format!("{name}_windows"), true);
    add(counts, &format!("{name}_positive"), observed);
    add(counts, &format!("{name}_negative"), !observed);
    add(counts, &format!("{name}_suggested"), suggested);
    add(
        counts,
        &format!("{name}_true_positive"),
        observed && suggested,
    );
    add(
        counts,
        &format!("{name}_false_positive"),
        !observed && suggested,
    );
}

fn read(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(
        &std::fs::read(path).with_context(|| path.display().to_string())?,
    )?)
}

fn run(cache: &Path, split: &str, catalog: &Path) -> Result<Value> {
    if !matches!(split, "validation" | "test") {
        bail!("split must be validation or test");
    }
    let manifest = read(&cache.join("manifest.json"))?;
    if manifest["schema"] != 1 || !manifest["train_players"].as_u64().is_some_and(|n| n > 0) {
        bail!("missing training-only artifact manifest");
    }
    nextprior::load_for_evaluation(&std::fs::read_to_string(cache.join("next_items.json"))?)?;
    recall_core::answers::load_for_evaluation(&std::fs::read_to_string(
        cache.join("answers.json"),
    )?)?;
    recall_core::bootsprior::load_for_evaluation(&std::fs::read_to_string(
        cache.join("boots.json"),
    )?)?;
    let items = read(&catalog.join("item.json"))?;
    let cat = Catalog::from_json(
        items["version"].as_str().unwrap_or("unknown"),
        &items,
        &read(&catalog.join("champion.json"))?,
        &json!([]),
    );
    let traits = pack::load_traits()?;
    let aggregates: HashMap<String, Aggregate> =
        serde_json::from_value(read(&cache.join("aggregates.json"))?)?;
    let mut groups: BTreeMap<String, Counts> = BTreeMap::new();
    let mut violation_counts: Counts = BTreeMap::new();
    let mut examples = Vec::new();
    let mut repeat_examples = Vec::new();
    let mut repeat_target_examples = 0;
    let mut invalid = 0;
    let mut total_frames = 0;
    let mut game_count = 0;
    let mut last_session = 0;
    for line in BufReader::new(std::fs::File::open(cache.join(format!("{split}.jsonl")))?).lines() {
        let game: Game = serde_json::from_str(&line?)?;
        if game.session <= last_session {
            bail!("games must have increasing unique session IDs");
        }
        last_session = game.session;
        game_count += 1;
        if game_count % 250 == 0 {
            eprintln!("Evaluated {game_count} games, {total_frames} frames");
        }
        let agg = aggregates
            .get(&game.aggregate)
            .context("missing training aggregate")?;
        if cat.champion_key(&game.champion) != Some(agg.champion_key) {
            bail!("case and aggregate champion differ");
        }
        let mut preferences = PlannerPreferences::default();
        let mut previous: Option<Previous> = None;
        for frame in game.frames {
            let snap = &frame.snapshot;
            let me = snap
                .me
                .as_ref()
                .context("corpus frame has no active player")?;
            if previous.as_ref().is_some_and(|p| snap.game_time <= p.time) {
                bail!("frames must be strictly chronological within each game");
            }
            let input = Inputs {
                champion: &game.champion,
                pack: None,
                aggregate: Some(agg),
                traits: &traits,
                catalog: &cat,
                enemies: &game.enemies,
                live: Some(snap),
            };
            let plan = engine::plan_with_preferences(&input, &preferences);
            let target = plan.next.as_ref().map(|n| nextprior::normalize(n.id));
            let buy_now = plan
                .next
                .as_ref()
                .filter(|n| n.buy_now_affordable)
                .and_then(|n| n.buy_now.as_ref())
                .map(|item| nextprior::normalize(item.id));
            let repeat = engine::plan_with_preferences(&input, &plan.preferences);
            let repeat_target = repeat.next.as_ref().map(|n| nextprior::normalize(n.id));
            if (target != repeat_target && repeat_target_examples < 20)
                || (plan.preferences.last_path != repeat.preferences.last_path
                    && repeat_examples.len() < 20)
            {
                repeat_target_examples += usize::from(target != repeat_target);
                repeat_examples.push(json!({
                    "session": game.session, "champion": game.champion, "role": game.role,
                    "time": snap.game_time, "gold": me.gold,
                    "inventory": me.player.items.iter().map(|i| (i.id, i.count)).collect::<Vec<_>>(),
                    "target": target, "repeat_target": repeat_target,
                    "before": preferences, "after": plan.preferences, "repeat": repeat.preferences,
                    "scores": plan.score_trace, "repeat_scores": repeat.score_trace
                }));
            }
            preferences = plan.preferences.clone();
            let checked = assessment::validate_plan(&plan, &cat, Some(snap));
            let is_invalid = !checked.violations.is_empty();
            invalid += u64::from(is_invalid);
            for violation in checked.violations {
                *violation_counts.entry(violation.kind.into()).or_default() += 1;
                if examples.len() < 20 {
                    examples.push(json!({"session": game.session, "champion": game.champion,
                        "time": snap.game_time, "kind": violation.kind, "message": violation.message,
                        "inventory": me.player.items.iter().map(|i| i.id).collect::<Vec<_>>(), "path": plan.path,
                        "target": plan.next.as_ref().map(|n| n.id)}));
                }
            }
            let mut inventory: Vec<_> = me
                .player
                .items
                .iter()
                .filter(|i| i.count > 0)
                .map(|i| (i.id, i.count))
                .collect();
            inventory.sort_unstable();
            let same_inventory = previous.as_ref().is_some_and(|p| p.inventory == inventory);
            let changed = previous.as_ref().is_some_and(|p| p.target != target);
            let consecutive = previous
                .as_ref()
                .is_some_and(|p| consecutive_minute(snap.game_time - p.time));
            let mut frame_counts = Counts::new();
            add(&mut frame_counts, "frames", true);
            add(&mut frame_counts, "no_target", target.is_none());
            add(
                &mut frame_counts,
                "transitions",
                same_inventory && consecutive,
            );
            let flip = same_inventory && consecutive && changed;
            add(&mut frame_counts, "flips", flip);
            // What kind of change: boots or a component detour entering or leaving, an item that
            // gold now finishes, or two finished items trading places with nothing bought.
            if flip {
                let kind = |id: Option<u32>| {
                    id.and_then(|id| cat.item(id)).map(|item| {
                        if item.effects.boots {
                            "boots"
                        } else if item.is_finished(&cat) {
                            "item"
                        } else {
                            "detour"
                        }
                    })
                };
                let kinds = [kind(previous.as_ref().and_then(|p| p.target)), kind(target)];
                let finishes = plan.next.as_ref().is_some_and(|n| {
                    n.buy_now_affordable && n.buy_now.as_ref().is_some_and(|b| b.id == n.id)
                });
                let name = if kinds.contains(&Some("boots")) {
                    "flips_boots"
                } else if kinds.contains(&Some("detour")) {
                    "flips_detour"
                } else if finishes {
                    "flips_finishable"
                } else {
                    "flips_order"
                };
                add(&mut frame_counts, name, true);
            }
            add(&mut frame_counts, "repeat_flips", target != repeat_target);
            add(
                &mut frame_counts,
                "repeat_path_flips",
                plan.preferences.last_path != repeat.preferences.last_path,
            );
            add(&mut frame_counts, "invalid_frames", is_invalid);
            score_answer(
                &mut frame_counts,
                "antiheal",
                buy_now,
                &frame.shop_items,
                |id| {
                    cat.item(id)
                        .is_some_and(|i| i.effects.grievous_wounds.is_some())
                },
            );
            score_answer(
                &mut frame_counts,
                "cleanse",
                buy_now,
                &frame.shop_items,
                |id| cat.item(id).is_some_and(|i| i.effects.cleanse.is_some()),
            );
            let missing_candidates = plan.path.iter().any(|p| {
                !p.owned
                    && nextprior::is_legendary(&cat, p.id)
                    && !plan.score_trace.iter().any(|s| s.id == p.id)
            });
            add(
                &mut frame_counts,
                "missing_target_candidates",
                missing_candidates,
            );
            add(
                &mut frame_counts,
                "affordable_detours",
                plan.next.as_ref().is_some_and(|n| {
                    n.buy_now_affordable
                        && cat.item(n.id).is_some_and(|i| {
                            !i.is_finished(&cat)
                                && !i.effects.boots
                                && (i.effects.grievous_wounds.is_some()
                                    || i.effects.cleanse.is_some())
                        })
                }),
            );
            let mut group_keys = vec!["all".to_string(), game.role.clone()];
            if game.champion == "Xayah" {
                group_keys.push("Xayah".into());
            }
            for key in &group_keys {
                let counts = groups.entry(key.clone()).or_default();
                for (name, n) in &frame_counts {
                    *counts.entry(name.clone()).or_default() += n;
                }
            }
            let future_legs: Vec<u32> = plan
                .path
                .iter()
                .filter(|p| !p.owned && nextprior::is_legendary(&cat, p.id))
                .map(|p| nextprior::normalize(p.id))
                .collect();
            let planned_boot = plan
                .path
                .iter()
                .find(|p| !p.owned && cat.item(p.id).is_some_and(|i| i.effects.boots))
                .map(|p| nextprior::normalize(p.id));
            for label in &frame.labels {
                let ambiguous = frame
                    .labels
                    .iter()
                    .filter(|l| l.horizon == label.horizon && l.kind == label.kind)
                    .count()
                    > 1;
                let already_owned = me
                    .player
                    .items
                    .iter()
                    .any(|i| i.count > 0 && nextprior::normalize(i.id) == label.item);
                for key in &group_keys {
                    let counts = groups
                        .entry(format!("{key}/{}", label.horizon))
                        .or_default();
                    add(counts, "ambiguous_labels", ambiguous);
                    add(counts, "already_owned_labels", already_owned);
                    if ambiguous || (already_owned && label.kind != "buy") {
                        continue;
                    }
                    if label.kind == "buy" {
                        add(counts, "buy_decisions", true);
                        add(counts, "buy_hits", buy_now == Some(label.item));
                        if label.component {
                            add(counts, "component_decisions", true);
                            add(counts, "component_hits", buy_now == Some(label.item));
                        }
                        continue;
                    }
                    add(counts, "decisions", true);
                    add(counts, "target_hits", target == Some(label.item));
                    if label.kind == "leg" {
                        add(counts, "legendary_decisions", true);
                        add(
                            counts,
                            "path_hits",
                            future_legs.first() == Some(&label.item),
                        );
                        add(
                            counts,
                            "path_top3_hits",
                            future_legs.iter().take(3).any(|i| *i == label.item),
                        );
                    } else if label.kind == "boots" {
                        add(counts, "boots_decisions", true);
                        add(counts, "boots_hits", planned_boot == Some(label.item));
                    }
                }
            }
            total_frames += 1;
            previous = Some(Previous {
                target,
                inventory,
                time: snap.game_time,
            });
        }
    }
    if total_frames == 0 {
        bail!("no frames evaluated");
    }
    Ok(
        json!({"schema": 1,"fingerprint":manifest["fingerprint"], "split":split,
        "games":game_count,"frames":total_frames,"invalid_frames":invalid,"groups":groups,
        "violation_counts":violation_counts,"violation_examples":examples,
        "repeat_examples":repeat_examples}),
    )
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 {
        eprintln!("backtest TRAINING_CACHE validation|test CATALOG_DIRECTORY (normally called by tools/priors/backtest.py)");
        std::process::exit(2);
    }
    match run(Path::new(&args[0]), &args[1], Path::new(&args[2])) {
        Ok(report) => {
            println!("{}", report);
            if report["invalid_frames"].as_u64().unwrap_or(1) > 0 {
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("{e:#}");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minute_jitter_counts_but_missing_frames_do_not() {
        assert!(consecutive_minute(60.034));
        assert!(!consecutive_minute(120.034));
        assert!(!consecutive_minute(0.5));
    }

    #[test]
    fn answer_windows_include_negative_purchases_and_ignore_no_shop() {
        let mut counts = Counts::new();
        score_answer(&mut counts, "answer", Some(100), &[], |id| id == 100);
        assert!(counts.is_empty());
        score_answer(&mut counts, "answer", Some(100), &[200], |id| id == 100);
        score_answer(&mut counts, "answer", None, &[100, 200], |id| id == 100);
        assert_eq!(counts["answer_windows"], 2);
        assert_eq!(counts["answer_positive"], 1);
        assert_eq!(counts["answer_negative"], 1);
        assert_eq!(counts["answer_false_positive"], 1);
        assert_eq!(counts["answer_true_positive"], 0);
    }
}
