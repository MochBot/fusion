//! Deep-tier and diagnostic checks against Mosaic's hydrated opener assets.

use super::*;

fn live_catalog() -> OpenerCatalog {
    let path = std::env::var("OPENER_BUILD_CATALOG").unwrap_or_else(|_| {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../mosaic-fusion-testing/static/openers/openers-v2.json"
        )
        .to_owned()
    });
    serde_json::from_slice(
        &std::fs::read(&path).unwrap_or_else(|error| panic!("read {path}: {error}")),
    )
    .expect("catalog should parse")
}

fn decode_base64(text: &str) -> Vec<u8> {
    let value = |byte: u8| -> u32 {
        match byte {
            b'A'..=b'Z' => u32::from(byte - b'A'),
            b'a'..=b'z' => u32::from(byte - b'a') + 26,
            b'0'..=b'9' => u32::from(byte - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("invalid base64 byte {byte}"),
        }
    };
    let bytes = text
        .bytes()
        .filter(|byte| *byte != b'=')
        .collect::<Vec<_>>();
    let mut out = Vec::new();
    for chunk in bytes.chunks(4) {
        let mut acc = 0u32;
        for (index, byte) in chunk.iter().enumerate() {
            acc |= value(*byte) << (18 - 6 * index);
        }
        for index in 0..chunk.len() - 1 {
            out.push((acc >> (16 - 8 * index)) as u8);
        }
    }
    out
}

#[derive(Default, Clone, Copy, Debug)]
struct Agreement {
    both: u32,
    ours_only: u32,
    cover_only: u32,
}

impl Agreement {
    fn of(ours: &[bool], bitmap: &[u8]) -> Self {
        let mut result = Self::default();
        for (rank, ours) in ours.iter().enumerate() {
            match (*ours, bitmap[rank >> 3] & (1 << (rank & 7)) != 0) {
                (true, true) => result.both += 1,
                (true, false) => result.ours_only += 1,
                (false, true) => result.cover_only += 1,
                (false, false) => {}
            }
        }
        result
    }

    fn mismatches(&self) -> u32 {
        self.ours_only + self.cover_only
    }

    fn add(&mut self, other: Self) {
        self.both += other.both;
        self.ours_only += other.ours_only;
        self.cover_only += other.cover_only;
    }
}

/// Recomputes the swng/opener_db first-bag cover bitmaps (queue-cover-v1)
/// with the planner. A bitmap can be the union of a record's setups and
/// their mirror, so each record reports its best single setup and the union
/// of all of them: an order the cover accepts that no setup builds is a
/// planner rejection to explain. Inputs default to Mosaic's hydrated assets.
#[test]
#[ignore = "deep tier: needs the hydrated opener assets"]
fn planner_matches_external_first_bag_cover() {
    use rayon::prelude::*;

    let assets = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../mosaic-fusion-testing/static/openers"
    );
    let catalog_path = std::env::var("OPENER_BUILD_CATALOG")
        .unwrap_or_else(|_| format!("{assets}/openers-v2.json"));
    let cover_path = std::env::var("OPENER_BUILD_COVER")
        .unwrap_or_else(|_| format!("{assets}/queue-cover-v1.json"));
    let catalog: OpenerCatalog = serde_json::from_slice(
        &std::fs::read(&catalog_path)
            .unwrap_or_else(|error| panic!("read {catalog_path}: {error}")),
    )
    .expect("catalog should parse");
    let cover: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&cover_path).unwrap_or_else(|error| panic!("read {cover_path}: {error}")),
    )
    .expect("cover should parse");
    let coverage = cover["coverage"].as_object().expect("coverage map");
    let queues = (0..5040).map(queue_of_rank).collect::<Vec<_>>();
    let only = std::env::var("OPENER_BUILD_ONLY").ok();
    // The cover counts shapes; the planner also holds builds to their clears.
    let shape_only = std::env::var("OPENER_BUILD_SHAPE_ONLY").is_ok();

    let rows = catalog
        .openers
        .par_iter()
        .filter(|record| only.as_deref().is_none_or(|id| id == record.id))
        .filter_map(|record| {
            let bitmap = decode_base64(coverage.get(&record.id)?.as_str()?);
            let mut best: Option<(Agreement, u32, bool)> = None;
            let mut union = vec![false; queues.len()];
            for root in record
                .tree
                .iter()
                .filter(|node| node.parent.is_none() && node.pieces <= 7)
            {
                for mirrored in [false, true] {
                    let Ok(mut target) = target_with_clears(&record.tree, mirrored, None, root.id)
                    else {
                        continue;
                    };
                    if !shape_only {
                        fit_clears(&mut target);
                    }
                    let ours = buildable_queues(&target, &queues, shape_only);
                    for (merged, built) in union.iter_mut().zip(&ours) {
                        *merged |= *built;
                    }
                    let score = Agreement::of(&ours, &bitmap);
                    if best
                        .as_ref()
                        .is_none_or(|(known, ..)| score.mismatches() < known.mismatches())
                    {
                        best = Some((score, root.id, mirrored));
                    }
                }
            }
            Some((record.id.clone(), best, Agreement::of(&union, &bitmap)))
        })
        .collect::<Vec<_>>();

    let mut single = Agreement::default();
    let mut merged = Agreement::default();
    let mut exact = 0;
    let mut unbuilt = Vec::new();
    let mut report = Vec::new();
    for (id, best, union) in &rows {
        let Some((score, root, mirrored)) = best else {
            unbuilt.push(id.clone());
            continue;
        };
        single.add(*score);
        merged.add(*union);
        exact += u32::from(score.mismatches() == 0);
        if score.mismatches() > 0 || union.cover_only > 0 {
            report.push(format!(
                "{id} root={root} mirrored={mirrored} best: both={} ours_only={} cover_only={} | union cover_only={}",
                score.both, score.ours_only, score.cover_only, union.cover_only
            ));
        }
    }
    report.sort();
    println!("clears enforced: {}", !shape_only);
    println!(
        "records compared: {}, no buildable root: {} {unbuilt:?}",
        rows.len() - unbuilt.len(),
        unbuilt.len()
    );
    println!(
        "best setup: exact records {exact}, both {}, ours only {}, cover only {}",
        single.both, single.ours_only, single.cover_only
    );
    println!("union of setups: cover only {}", merged.cover_only);
    for line in &report {
        println!("{line}");
    }
}

#[test]
#[ignore = "diagnostic: OPENER_BUILD_ONLY=<record> OPENER_BUILD_ROOT=<id> [OPENER_BUILD_MIRRORED=1]"]
fn explain_cover_gap() {
    let assets = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../mosaic-fusion-testing/static/openers"
    );
    let catalog: OpenerCatalog = serde_json::from_slice(
        &std::fs::read(format!("{assets}/openers-v2.json")).expect("catalog"),
    )
    .expect("catalog should parse");
    let cover: serde_json::Value = serde_json::from_slice(
        &std::fs::read(format!("{assets}/queue-cover-v1.json")).expect("cover"),
    )
    .expect("cover should parse");
    let id = std::env::var("OPENER_BUILD_ONLY").expect("OPENER_BUILD_ONLY");
    let root =
        std::env::var("OPENER_BUILD_ROOT").map_or(0, |value| value.parse().expect("root id"));
    let mirrored = std::env::var("OPENER_BUILD_MIRRORED").is_ok();
    let record = catalog
        .openers
        .iter()
        .find(|record| record.id == id)
        .expect("record");
    let bitmap = decode_base64(cover["coverage"][&id].as_str().expect("cover bitmap"));
    let target = target_with_clears(&record.tree, mirrored, None, root).expect("target");
    let mut shown = [0; 2];
    for rank in 0..5040 {
        let queue = queue_of_rank(rank);
        let letters = queue
            .iter()
            .map(|piece| letter_string(*piece))
            .collect::<String>();
        let plan = build_opener(&OpenerBuildInput {
            tree: record.tree.clone(),
            mirrored,
            targets: vec![root],
            start_node: None,
            queue: letters.clone(),
            default_queue: false,
            hold: None,
            hold_enabled: true,
        })
        .expect("valid input");
        let ours = plan.status == BuildStatus::Complete;
        let theirs = bitmap[rank >> 3] & (1 << (rank & 7)) != 0;
        if ours == theirs {
            continue;
        }
        let side = usize::from(ours);
        if shown[side] >= 3 {
            continue;
        }
        shown[side] += 1;
        println!(
            "{} {letters}: {:?} {:?}",
            if ours { "OURS ONLY" } else { "COVER ONLY" },
            plan.status,
            plan.reasons
        );
        for step in &plan.steps {
            let keys = step
                .keys
                .iter()
                .filter(|key| **key != "softDrop")
                .copied()
                .collect::<Vec<_>>();
            let soft = step.keys.iter().filter(|key| **key == "softDrop").count();
            println!(
                "  {} {:?} cells={:?} spin={:?} soft={soft} keys={keys:?}",
                step.piece, step.hold_action, step.cells, step.spin
            );
        }
        if let Some(stuck) = &plan.stuck {
            println!(
                "  stuck: {}",
                serde_json::to_string(stuck).unwrap_or_default()
            );
        }
    }
    println!(
        "target rows (floor-up): {:?}",
        board_rows(&target.goal, &target.letters)
    );
}

/// Checks the planner's search against the brute force on every first-bag
/// setup of the live catalog with distinct letters.
#[test]
#[ignore = "deep tier: needs the hydrated opener assets"]
fn planner_agrees_with_brute_force_on_live_first_bag_setups() {
    use rayon::prelude::*;

    let assets = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../mosaic-fusion-testing/static/openers"
    );
    let catalog_path = std::env::var("OPENER_BUILD_CATALOG")
        .unwrap_or_else(|_| format!("{assets}/openers-v2.json"));
    let catalog: OpenerCatalog = serde_json::from_slice(
        &std::fs::read(&catalog_path)
            .unwrap_or_else(|error| panic!("read {catalog_path}: {error}")),
    )
    .expect("catalog should parse");
    let only = std::env::var("OPENER_BUILD_ONLY").ok();
    let queues = (0..5040).map(queue_of_rank).collect::<Vec<_>>();
    let targets = catalog
        .openers
        .iter()
        .filter(|record| only.as_deref().is_none_or(|id| id == record.id))
        .flat_map(|record| {
            record
                .tree
                .iter()
                .filter(|node| node.parent.is_none() && node.pieces == 7)
                .filter_map(|node| {
                    let target = target_with_clears(&record.tree, false, None, node.id).ok()?;
                    distinct_lettered(&target).then(|| (record.id.clone(), node.id, target))
                })
        })
        .collect::<Vec<_>>();
    let failures = targets
        .par_iter()
        .filter_map(|(id, node, target)| {
            let disagreements = search_disagreements(target, &queues);
            (!disagreements.is_empty())
                .then(|| format!("{id} node {node}: {}", disagreements.len()))
        })
        .collect::<Vec<_>>();
    println!("setups checked: {}", targets.len());
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
#[ignore = "diagnostic: times one live build phase by phase"]
fn time_live_build_phases() {
    let assets = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../mosaic-fusion-testing/static/openers"
    );
    let catalog: OpenerCatalog = serde_json::from_slice(
        &std::fs::read(format!("{assets}/openers-v2.json")).expect("catalog"),
    )
    .expect("catalog should parse");
    let record = std::env::var("OPENER_BUILD_ONLY").unwrap_or_else(|_| "stickspin".to_owned());
    let node = std::env::var("OPENER_BUILD_ROOT").map_or(79, |value| value.parse().expect("node"));
    let queue = std::env::var("OPENER_BUILD_QUEUE")
        .unwrap_or_else(|_| "OTZJISLSLOIJZTZLJOTISZLOSJITOTZSIJLLJOZIST".to_owned());
    let tree = catalog
        .openers
        .iter()
        .find(|opener| opener.id == record)
        .expect("record")
        .tree
        .clone();
    let letters = queue
        .bytes()
        .filter_map(piece_for_letter)
        .collect::<Vec<_>>();
    let clock = std::time::Instant::now();
    let mut target = target_with_clears(&tree, false, None, node).expect("target");
    eprintln!("target {:?}", clock.elapsed());
    let clock = std::time::Instant::now();
    fit_clears(&mut target);
    eprintln!("fit_clears {:?}", clock.elapsed());
    let clock = std::time::Instant::now();
    let mut search = Search::new(&target, &letters, None, true, BUILD_BUDGET);
    let found = search.run();
    eprintln!(
        "queue search {:?} found {} exhausted {}",
        clock.elapsed(),
        found.is_some(),
        search.exhausted
    );
    let clock = std::time::Instant::now();
    let built = build_opener(&input(tree, node, &queue)).expect("valid input");
    eprintln!(
        "whole request {:?} -> {:?} {:?}",
        clock.elapsed(),
        built.status,
        built.reasons
    );
}

/// Times the default order of every version of every live record with a
/// tree, and prints the slowest requests and the versions left without one.
#[test]
#[ignore = "diagnostic: times default orders on live records"]
fn time_default_orders() {
    use rayon::prelude::*;

    let catalog = live_catalog();
    let only = std::env::var("OPENER_BUILD_ONLY").ok();
    let requests = catalog
        .openers
        .iter()
        .filter(|record| only.as_ref().is_none_or(|id| *id == record.id))
        .flat_map(|record| {
            versions::version_shapes(&record.tree)
                .into_iter()
                .map(|(names, shapes)| (record.id.clone(), record.tree.clone(), names, shapes))
        })
        .collect::<Vec<_>>();
    let mut timings = requests
        .par_iter()
        .map(|(id, tree, names, shapes)| {
            let request = OpenerBuildInput {
                tree: tree.clone(),
                mirrored: false,
                targets: shapes.clone(),
                start_node: None,
                queue: String::new(),
                default_queue: true,
                hold: None,
                hold_enabled: true,
            };
            let clock = std::time::Instant::now();
            let plan = build_opener(&request).expect("valid input");
            let reason = plan.reasons.first().map_or_else(
                || "none".to_owned(),
                |reason| {
                    format!("{reason:?}")
                        .split([' ', '{', '('])
                        .next()
                        .unwrap_or("")
                        .to_owned()
                },
            );
            (
                clock.elapsed(),
                id.clone(),
                names.join(" > "),
                plan.queue.len(),
                plan.status,
                reason,
            )
        })
        .collect::<Vec<_>>();
    timings.sort_by_key(|timing| std::cmp::Reverse(timing.0));
    let total = timings.len();
    let median = timings[total / 2].0;
    let p95 = timings[total / 20].0;
    let without = timings.iter().filter(|timing| timing.3 == 0).count();
    let incomplete = timings
        .iter()
        .filter(|timing| timing.3 > 0 && timing.4 != BuildStatus::Complete)
        .count();
    println!(
        "versions {total}: median {median:?}, p95 {p95:?}; no order {without}; order not complete {incomplete}"
    );
    for (elapsed, id, names, len, status, _) in timings.iter().take(10) {
        println!("  {elapsed:?} {id} [{names}] queue {len} {status:?}");
    }
    let mut causes = std::collections::BTreeMap::<String, usize>::new();
    for timing in timings.iter().filter(|timing| timing.3 == 0) {
        *causes
            .entry(format!("{:?} {}", timing.4, timing.5))
            .or_default() += 1;
    }
    println!("no order by cause: {causes:?}");
}
