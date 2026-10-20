use super::*;

mod live;

use crate::openers::catalog::{OpenerCatalog, OpenerTreeNode};
use crate::openers::recognition::{LegalityMemo, LegalityVerdict};
use clears::fit_clears;
use frame::{board_of, fill, is_filled, to_physical};
use search::Ending;

fn mini_tree(record: &str) -> Vec<OpenerTreeNode> {
    let catalog: OpenerCatalog = serde_json::from_slice(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/openers/catalog-mini.json"
    )))
    .expect("mini catalog should parse");
    catalog
        .openers
        .into_iter()
        .find(|opener| opener.id == record)
        .map(|opener| opener.tree)
        .expect("record should exist in the mini catalog")
}

fn input(tree: Vec<OpenerTreeNode>, target_node: u32, queue: &str) -> OpenerBuildInput {
    OpenerBuildInput {
        tree,
        mirrored: false,
        targets: vec![target_node],
        start_node: None,
        queue: queue.to_owned(),
        default_queue: false,
        hold: None,
        hold_enabled: true,
    }
}

const QUEUE_ALPHABET: [Piece; 7] = [
    Piece::I,
    Piece::J,
    Piece::L,
    Piece::O,
    Piece::S,
    Piece::T,
    Piece::Z,
];

/// The queue with Lehmer rank `rank` over IJLOSTZ, as Mosaic's
/// `queuePermutationRank` numbers the cover bitmaps.
fn queue_of_rank(mut rank: usize) -> Vec<Piece> {
    let mut letters = QUEUE_ALPHABET.to_vec();
    let mut queue = Vec::with_capacity(7);
    for remaining in (1..=7).rev() {
        let block = (1..remaining).product::<usize>();
        queue.push(letters.remove(rank / block));
        rank %= block;
    }
    queue
}

/// Whether each first-bag queue completes the build, playing the held piece
/// as the next bag arrives (the cover's convention).
fn buildable_queues(target: &BuildTarget, queues: &[Vec<Piece>], shape_only: bool) -> Vec<bool> {
    let search = Search::new(target, &queues[0], None, true, 4_000);
    let mut search = if shape_only {
        search.shape_only()
    } else {
        search
    };
    queues
        .iter()
        .map(|queue| {
            search.set_queue(queue, None, 4_000);
            search
                .run()
                .is_some_and(|plan| plan.ending == Ending::Complete)
        })
        .collect()
}

/// Every order a one-slot hold lets the queue come out in; the held piece
/// may be played last as the piece after the queue arrives.
fn hold_orders(queue: &[Piece]) -> Vec<Vec<Piece>> {
    fn walk(
        queue: &[Piece],
        next: usize,
        hold: Option<Piece>,
        order: &mut Vec<Piece>,
        out: &mut Vec<Vec<Piece>>,
    ) {
        let Some(current) = queue.get(next).copied() else {
            match hold {
                Some(held) => {
                    order.push(held);
                    out.push(order.clone());
                    order.pop();
                }
                None => out.push(order.clone()),
            }
            return;
        };
        order.push(current);
        walk(queue, next + 1, hold, order, out);
        order.pop();
        match hold {
            Some(held) => {
                order.push(held);
                walk(queue, next + 1, Some(current), order, out);
                order.pop();
            }
            None => walk(queue, next + 1, Some(current), order, out),
        }
    }
    let mut out = Vec::new();
    walk(queue, 0, None, &mut Vec::new(), &mut out);
    out
}

/// Independent of the planner's search: tries every hold order, placing
/// each piece in the one catalogued spot of its letter. Only for targets
/// whose pieces all have distinct letters and no unlettered cells.
fn brute_force_buildable(target: &BuildTarget, queue: &[Piece], memo: &mut LegalityMemo) -> bool {
    hold_orders(queue).into_iter().any(|order| {
        let mut filled = target.start;
        for piece in order {
            if filled == target.goal {
                break;
            }
            let Some(slot) = target
                .slots
                .iter()
                .find(|slot| slot.piece == piece && !is_filled(&filled, &slot.cells))
            else {
                return false;
            };
            let legal = matches!(
                memo.check(
                    &board_of(&filled),
                    slot.letter,
                    &to_physical(&filled, &slot.cells)
                )
                .verdict,
                LegalityVerdict::Legal { .. }
            );
            if !legal {
                return false;
            }
            filled = fill(&filled, &slot.cells);
        }
        filled == target.goal
    })
}

fn distinct_lettered(target: &BuildTarget) -> bool {
    target.outline.iter().all(|row| *row == 0)
        && target.slots.iter().enumerate().all(|(index, slot)| {
            target.slots[..index]
                .iter()
                .all(|other| other.piece != slot.piece)
        })
}

/// Queues on which the planner's shape search and the brute force disagree;
/// the brute force knows nothing of clears, so it checks the shape alone.
fn search_disagreements(target: &BuildTarget, queues: &[Vec<Piece>]) -> Vec<usize> {
    let ours = buildable_queues(target, queues, true);
    let mut memo = LegalityMemo::default();
    queues
        .iter()
        .enumerate()
        .filter(|(rank, queue)| ours[*rank] != brute_force_buildable(target, queue, &mut memo))
        .map(|(rank, _)| rank)
        .collect()
}

#[test]
fn planner_agrees_with_brute_force_hold_orders() {
    let queues = (0..5040).map(queue_of_rank).collect::<Vec<_>>();
    for (record, node) in [("perfect-clear-opener", 0), ("crowbar-v2", 0)] {
        let tree = mini_tree(record);
        for mirrored in [false, true] {
            let target = target_with_clears(&tree, mirrored, None, node).expect("target");
            assert!(distinct_lettered(&target));
            let disagreements = search_disagreements(&target, &queues);
            assert!(
                disagreements.is_empty(),
                "{record} mirrored={mirrored}: {} queues disagree, first {:?}",
                disagreements.len(),
                queue_of_rank(disagreements[0])
            );
        }
    }
}

/// swng/opener_db (queue-cover-v1) marks 2016 of the 5040 first bags as
/// covering PCO; the planner reproduces that set exactly.
#[test]
fn pco_first_bag_cover_matches_the_external_count() {
    let target =
        target_with_clears(&mini_tree("perfect-clear-opener"), false, None, 0).expect("target");
    let queues = (0..5040).map(queue_of_rank).collect::<Vec<_>>();
    let covered = buildable_queues(&target, &queues, false)
        .into_iter()
        .filter(|built| *built)
        .count();
    assert_eq!(covered, 2016);
}

fn plan(record: &str, target_node: u32, queue: &str) -> OpenerBuildPlan {
    build_opener(&input(mini_tree(record), target_node, queue)).expect("valid input")
}

#[test]
fn prefers_hard_drops_over_tucks() {
    let built = plan("perfect-clear-opener", 0, "TSZIJOL");
    assert_eq!(built.status, BuildStatus::Complete);
    assert_eq!(built.steps.len(), 7);
    for step in &built.steps {
        assert!(
            !step.keys.contains(&"softDrop"),
            "{} needs a soft drop",
            step.piece
        );
        assert_eq!(step.keys.last(), Some(&"hardDrop"));
    }
}

#[test]
fn every_frame_path_ends_on_its_cells() {
    let built = plan("crowbar-v2", 1, "ZJTIOLSSLTJOZI");
    assert_eq!(built.status, BuildStatus::Complete);
    assert_eq!(built.steps.len(), 14);
    for step in &built.steps {
        let mut last = step.frames.last().expect("frames").clone();
        last.sort_unstable();
        let mut cells = step
            .cells
            .iter()
            .map(|[x, y]| [i16::from(*x), i16::from(*y)])
            .collect::<Vec<_>>();
        cells.sort_unstable();
        assert_eq!(last, cells);
    }
    assert_eq!(
        built
            .stages
            .iter()
            .map(|stage| stage.node_id)
            .collect::<Vec<_>>(),
        [0, 1]
    );
    assert_eq!(
        built.steps.iter().filter_map(|step| step.stage).max(),
        Some(1)
    );
    // The first bag's catalogued board appears before the second bag starts.
    assert_eq!(
        built.steps.iter().position(|step| step.stage == Some(0)),
        Some(6)
    );
    assert!(built.steps.iter().any(|step| !step.cleared_rows.is_empty()));
}

#[test]
fn a_short_queue_is_played_into_the_build_before_asking_for_more() {
    let built = plan("crowbar-v2", 1, "ZJTIOLS");
    assert_eq!(built.status, BuildStatus::NeedsMorePieces);
    assert!(!built.trailing_hold);
    assert_eq!(built.queue_used, 7);
    assert_eq!(built.remaining.pieces.len(), 14 - built.steps.len());
}

#[test]
fn a_letter_the_queue_lacks_is_still_needed() {
    let built = plan("perfect-clear-opener", 0, "TSZIJOO");
    assert_eq!(built.status, BuildStatus::NeedsMorePieces);
    assert_eq!(built.steps.len(), 6);
    assert_eq!(built.end_hold.as_deref(), Some("O"));
    assert_eq!(built.remaining.pieces, ["L"]);
}

#[test]
fn a_missing_letter_is_named_when_the_queue_overflows_the_build() {
    let built = plan("perfect-clear-opener", 0, "TSZIJOOTS");
    assert_eq!(built.status, BuildStatus::Impossible);
    assert_eq!(
        built.reasons,
        [BuildReason::MissingPieces {
            pieces: vec![MissingPiece {
                piece: "L".to_owned(),
                needed: 1,
                available: 0
            }]
        }]
    );
    assert_stuck_placed(&built);
}

#[test]
fn a_queue_order_conflict_names_the_stuck_pieces() {
    let target =
        target_with_clears(&mini_tree("perfect-clear-opener"), false, None, 0).expect("target");
    let queues = (0..5040).map(queue_of_rank).collect::<Vec<_>>();
    let rank = buildable_queues(&target, &queues, false)
        .iter()
        .position(|built| !built)
        .expect("some first bag does not cover PCO");
    let queue = queues[rank]
        .iter()
        .map(|piece| letter_string(*piece))
        .collect::<String>();
    let built = plan("perfect-clear-opener", 0, &queue);
    assert_eq!(built.status, BuildStatus::Impossible);
    assert_eq!(built.reasons, [BuildReason::OrderConflict]);
    assert_stuck_placed(&built);
    let stuck = built.stuck.expect("stuck point");
    assert!(!stuck.placed.is_empty());
    assert!(stuck.pieces.iter().any(|piece| piece
        .slots
        .iter()
        .any(|slot| slot.verdict != SlotVerdict::Placeable)));
}

/// The recognition census lists crowbar-v2 node 3 as having no SRS-valid
/// order from node 0.
#[test]
fn an_unbuildable_diagram_is_reported() {
    let mut request = input(mini_tree("crowbar-v2"), 3, "LJZIOTS");
    request.start_node = Some(0);
    let built = build_opener(&request).expect("valid input");
    assert_eq!(built.status, BuildStatus::Impossible);
    assert_eq!(built.reasons, [BuildReason::Unbuildable]);
    assert_stuck_placed(&built);
}

/// A dead end lists its pieces in `stuck.placed`, not as steps, and they
/// account for exactly the stuck board's cells beyond the start board.
fn assert_stuck_placed(built: &OpenerBuildPlan) {
    let stuck = built.stuck.as_ref().expect("stuck point");
    assert!(built.steps.is_empty());
    assert_eq!(built.end_hold, built.start_hold);
    assert_eq!(stuck.placed.len(), stuck.after_step);
    let mut rows = built
        .start_board
        .iter()
        .map(|row| row.as_bytes().to_vec())
        .collect::<Vec<_>>();
    for placed in &stuck.placed {
        for [x, y] in &placed.cells {
            let y = usize::from(*y);
            if rows.len() <= y {
                rows.resize(y + 1, vec![b'_'; 10]);
            }
            let cell = &mut rows[y][usize::from(*x)];
            assert_eq!(*cell, b'_', "{} overlaps a filled cell", placed.piece);
            *cell = placed.piece.as_bytes()[0];
        }
        rows.retain(|row| row.contains(&b'_'));
    }
    while rows
        .last()
        .is_some_and(|row| row.iter().all(|cell| *cell == b'_'))
    {
        rows.pop();
    }
    let rows = rows
        .into_iter()
        .map(|row| String::from_utf8(row).expect("ascii"))
        .collect::<Vec<_>>();
    assert_eq!(rows, stuck.board);
    let json = serde_json::to_value(built).expect("plan serializes");
    let placed = json["stuck"]["placed"].as_array().expect("placed list");
    assert_eq!(json["stuck"]["afterStep"], placed.len());
    assert!(placed
        .iter()
        .all(|piece| piece["piece"].is_string() && piece["cells"].is_array()));
}

#[test]
fn a_starting_hold_is_kept_apart_from_the_hold_after_the_build() {
    let mut request = input(mini_tree("perfect-clear-opener"), 0, "SZIJOL");
    request.hold = Some("T".to_owned());
    let built = build_opener(&request).expect("valid input");
    assert_eq!(built.status, BuildStatus::Complete, "{:?}", built.reasons);
    assert_eq!(built.start_hold.as_deref(), Some("T"));
    let last = built.steps.last().expect("steps");
    assert_eq!(built.end_hold, last.hold_after);
    let json = serde_json::to_value(&built).expect("plan serializes");
    assert_eq!(json["startHold"], "T");
    assert_eq!(json["endHold"], serde_json::json!(last.hold_after));
    assert!(json.get("hold").is_none());
}

#[test]
fn builds_from_a_catalogued_start_node() {
    let mut request = input(mini_tree("crowbar-v2"), 1, "SLTJOZI");
    request.start_node = Some(0);
    let built = build_opener(&request).expect("valid input");
    assert_eq!(built.status, BuildStatus::Complete);
    assert_eq!(built.steps.len(), 7);
    assert!(!built.start_board.is_empty());
}

#[test]
fn a_mirrored_build_takes_the_mirrored_queue() {
    let mut request = input(mini_tree("perfect-clear-opener"), 0, "IJOLTZS");
    request.mirrored = true;
    let built = build_opener(&request).expect("valid input");
    assert_eq!(built.status, BuildStatus::Complete);
}

/// Each perfect clear leaves four full rows in the planning frame, so a long
/// loop of them runs out of frame rows.
#[test]
fn a_path_longer_than_the_frame_is_reported_by_node() {
    let root = OpenerTreeNode {
        id: 0,
        parent: None,
        pieces: 10,
        rows: Vec::new(),
        pre_clear_rows: Some(vec!["IIIIIIIIII".to_owned(); 4]),
        clear_rows: Some(vec![0, 1, 2, 3]),
        // Ten vertical I pieces, one per column.
        placements: Some(
            (0..10)
                .map(|x| crate::openers::catalog::OpenerPlacement {
                    letter: "I".to_owned(),
                    cells: vec![[x, 0], [x, 1], [x, 2], [x, 3]],
                })
                .collect(),
        ),
        grey: false,
        route_name: None,
        annotations: Vec::new(),
        est: None,
    };
    let tree = (0..11)
        .map(|id| OpenerTreeNode {
            id,
            parent: id.checked_sub(1),
            pieces: 10 * (id + 1),
            ..root.clone()
        })
        .collect::<Vec<_>>();
    let built = build_opener(&input(tree.clone(), 10, "IIIIIIIIII")).expect("valid input");
    assert_eq!(built.reasons, [BuildReason::TooLong { node_id: 10 }]);
    let mut later = input(tree, 10, "IIIIIIIIII");
    later.start_node = Some(9);
    let built = build_opener(&later).expect("valid input");
    assert_eq!(built.status, BuildStatus::Complete);
    assert_eq!(built.steps.len(), 10);
}

#[test]
fn rejects_malformed_requests() {
    assert!(matches!(
        build_opener(&input(mini_tree("perfect-clear-opener"), 0, "TSZ?")),
        Err(OpenerBuildError::InvalidQueue)
    ));
    assert!(matches!(
        build_opener(&input(mini_tree("perfect-clear-opener"), 999, "TSZ")),
        Err(OpenerBuildError::UnknownNode(999))
    ));
    let mut request = input(mini_tree("crowbar-v2"), 0, "TSZ");
    request.start_node = Some(1);
    assert!(matches!(
        build_opener(&request),
        Err(OpenerBuildError::StartNotAncestor)
    ));
}

fn full_tree(record: &str) -> Vec<OpenerTreeNode> {
    let catalog: OpenerCatalog = serde_json::from_slice(
        &std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/openers/catalog-full.json"
        ))
        .expect("the hydrated full catalog fixture"),
    )
    .expect("full catalog should parse");
    catalog
        .openers
        .into_iter()
        .find(|opener| opener.id == record)
        .map(|opener| opener.tree)
        .expect("record should exist in the full catalog")
}

/// A queue some order of which builds the target with all its clears: the
/// any-order search's own order.
fn working_queue(target: &BuildTarget) -> String {
    Search::new(target, &[], None, false, BUILD_BUDGET)
        .any_order_path()
        .expect("the target should be buildable with its clears")
        .iter()
        .map(|choice| letter_string(choice.piece))
        .collect()
}

/// Stickspin's route to its 35-piece board: a single, then T-spin double,
/// T-spin double, T-spin triple, T-spin double. The plan performs each one,
/// with the spin, in order; the shape alone is not a success.
#[test]
fn stickspin_performs_every_catalogued_clear_in_order() {
    let tree = full_tree("stickspin");
    let target = target_with_clears(&tree, false, None, 48).expect("target");
    let expected = [
        "single",
        "T-spin double",
        "T-spin double",
        "T-spin triple",
        "T-spin double",
    ];
    assert_eq!(
        target
            .stages
            .iter()
            .filter_map(|stage| stage.label.as_deref())
            .collect::<Vec<_>>(),
        expected
    );
    let queue = working_queue(&target);
    let built = build_opener(&input(tree, 48, &queue)).expect("valid input");
    assert_eq!(built.status, BuildStatus::Complete, "{:?}", built.reasons);
    let performed = built
        .steps
        .iter()
        .filter_map(|step| step.clear.as_ref())
        .filter(|clear| clear.catalogued.is_some())
        .map(|clear| clear.label.as_str())
        .collect::<Vec<_>>();
    // A catalogued clear is performed at least as listed: a spin label must
    // match, and a plain clear may come with a spin bonus.
    assert_eq!(performed.len(), expected.len(), "{performed:?}");
    for (performed, expected) in performed.iter().zip(expected) {
        if expected.contains("spin") {
            assert_eq!(performed, &expected);
        } else {
            assert!(performed.ends_with(expected), "{performed} for {expected}");
        }
    }
    assert!(built.clears.iter().all(|clear| clear.enforced));
    assert_eq!(
        built.attack,
        built
            .steps
            .iter()
            .filter_map(|step| step.clear.as_ref())
            .map(|clear| clear.attack)
            .sum::<f32>()
    );
    for step in &built.steps {
        if let Some(clear) = &step.clear {
            if clear.label.contains("T-spin") {
                assert_eq!(step.piece, "T");
                assert!(
                    matches!(step.keys.iter().rev().nth(1), Some(key) if key.starts_with("rotate")),
                    "a spin ends with a rotation before the drop: {:?}",
                    step.keys
                );
            }
        }
    }
}

/// With hold off and the second bag's T dealt before the overhang it spins
/// under, the T could only drop flat into its slot: that builds the shape but
/// clears the rows without the T-spin double, so the reason names the lost
/// clear and the T's slot says it cannot spin in.
#[test]
fn a_queue_that_builds_the_shape_but_not_the_tsd_names_the_lost_clear() {
    let tree = full_tree("stickspin");
    let target = target_with_clears(&tree, false, None, 28).expect("target");
    let queue = working_queue(&target);
    let (first, second) = queue.split_at(7);
    let rest = second.replacen('T', "", 1);
    let pieces = |text: &str| {
        text.bytes()
            .filter_map(piece_for_letter)
            .collect::<Vec<_>>()
    };
    let early_t = (0..rest.len())
        .map(|at| format!("{first}{}T{}", &rest[..at], &rest[at..]))
        .find(|candidate| {
            let letters = pieces(candidate);
            Search::new(&target, &letters, None, false, BUILD_BUDGET)
                .run()
                .is_none()
                && Search::new(&target, &letters, None, false, BUILD_BUDGET)
                    .shape_only()
                    .run()
                    .is_some_and(|plan| plan.ending == Ending::Complete)
        })
        .expect("some early T builds the shape but not the T-spin double");
    let mut request = input(tree, 28, &early_t);
    request.hold_enabled = false;
    let built = build_opener(&request).expect("valid input");
    assert_eq!(built.status, BuildStatus::Impossible);
    assert_eq!(
        built.reasons,
        [BuildReason::ClearsLost {
            node_id: 28,
            label: "T-spin double".to_owned()
        }]
    );
    assert_stuck_placed(&built);
    let stuck = built.stuck.expect("stuck point");
    let t = stuck
        .pieces
        .iter()
        .find(|piece| piece.piece == "T")
        .expect("the T is explained");
    assert!(
        t.slots.iter().any(|slot| {
            slot.verdict == SlotVerdict::NoSpin && slot.clear.as_deref() == Some("T-spin double")
        }),
        "{}",
        serde_json::to_string(&stuck).unwrap_or_default()
    );
}

/// The shape-only planner would finish this 14-piece board with hold, but
/// holding the build to its T-spin double is what the page promises.
#[test]
fn every_catalogued_spin_is_performed_as_a_spin() {
    let tree = full_tree("stickspin");
    let target = target_with_clears(&tree, true, None, 28).expect("target");
    let queue = working_queue(&target);
    let mut request = input(tree, 28, &queue);
    request.mirrored = true;
    let built = build_opener(&request).expect("valid input");
    assert_eq!(built.status, BuildStatus::Complete);
    let tsd = built
        .steps
        .iter()
        .find(|step| {
            step.clear
                .as_ref()
                .is_some_and(|clear| clear.catalogued == Some(28))
        })
        .expect("the T-spin double");
    assert_eq!(tsd.piece, "T");
    assert_eq!(tsd.spin, SpinName::Full);
    assert_eq!(tsd.clear.as_ref().map(|clear| clear.lines), Some(2));
}

/// The opener data labels Melon PC's first clear a mini T-spin single from the
/// static diagram, but the T drops straight into its wall slot and no path
/// spins it in. The build holds the rows to clearing together and marks the
/// label as relaxed instead of calling the opener impossible.
#[test]
fn an_unperformable_catalogued_spin_is_relaxed_and_marked() {
    let root = OpenerTreeNode {
        id: 0,
        parent: None,
        pieces: 7,
        rows: vec![
            "I_________".to_owned(),
            "ILS_______".to_owned(),
            "ILSSOOZZ__".to_owned(),
            "ILLSOO_ZZ_".to_owned(),
        ],
        pre_clear_rows: None,
        clear_rows: None,
        placements: None,
        grey: false,
        route_name: None,
        annotations: Vec::new(),
        est: None,
    };
    let mut child = root.clone();
    child.id = 1;
    child.parent = Some(0);
    child.pieces = 8;
    child.pre_clear_rows = Some(vec![
        "I_________".to_owned(),
        "ILS______T".to_owned(),
        "ILSSOOZZTT".to_owned(),
        "ILLSOO_ZZT".to_owned(),
    ]);
    child.rows = vec![
        "I_________".to_owned(),
        "ILS______T".to_owned(),
        "ILLSOO_ZZT".to_owned(),
    ];
    child.clear_rows = Some(vec![1]);
    child.placements = Some(vec![crate::openers::catalog::OpenerPlacement {
        letter: "T".to_owned(),
        cells: vec![[9, 0], [8, 1], [9, 1], [9, 2]],
    }]);
    child.est = Some(crate::openers::OpenerNodeEst {
        attack: 0,
        cum_attack: 0,
        clear: Some("mini T-spin single".to_owned()),
    });
    let tree = vec![root, child];
    let mut target = target_with_clears(&tree, false, Some(0), 1).expect("target");
    assert_eq!(
        target.stages[0].clear.as_ref().map(|clear| clear.min_spin),
        Some(crate::header::SpinType::Mini)
    );
    fit_clears(&mut target);
    assert!(target.stages[0].relaxed);
    let mut request = input(tree, 1, "T");
    request.start_node = Some(0);
    let built = build_opener(&request).expect("valid input");
    assert_eq!(built.status, BuildStatus::Complete);
    assert!(built.clears[0].relaxed);
    assert_eq!(
        built.steps[0]
            .clear
            .as_ref()
            .map(|clear| clear.label.as_str()),
        Some("single")
    );
}

/// A board the source shows only as an outline larger than eight pieces is
/// not guessed at.
#[test]
fn an_outline_only_board_is_not_planned() {
    let root = OpenerTreeNode {
        id: 0,
        parent: None,
        pieces: 9,
        rows: vec!["XXXXXX____".to_owned(); 6],
        pre_clear_rows: None,
        clear_rows: None,
        placements: None,
        grey: true,
        route_name: None,
        annotations: Vec::new(),
        est: None,
    };
    let built = build_opener(&input(vec![root.clone()], 0, "IJLOSTZ")).expect("valid input");
    assert_eq!(built.status, BuildStatus::Impossible);
    assert_eq!(built.reasons, [BuildReason::OutlineOnly { cells: 36 }]);

    // A version whose every shape is outlined answers the same way for a
    // typed queue: a plan with the reason, not an error.
    let other = OpenerTreeNode {
        id: 1,
        ..root.clone()
    };
    let mut both = input(vec![root, other], 0, "IJLOSTZ");
    both.targets = vec![0, 1];
    let built = build_opener(&both).expect("an outlined version is not an error");
    assert_eq!(built.status, BuildStatus::Impossible);
    assert_eq!(built.reasons, [BuildReason::OutlineOnly { cells: 36 }]);
}

/// An outline of up to eight pieces is filled with pieces the queue has; a
/// default order the search cannot find ends at once, without a queue search
/// over the empty queue.
#[test]
fn an_outline_of_eight_pieces_or_fewer_is_planned() {
    // Five columns four high: five vertical I pieces, among other fills.
    let root = OpenerTreeNode {
        id: 0,
        parent: None,
        pieces: 5,
        rows: vec!["XXXXX_____".to_owned(); 4],
        pre_clear_rows: None,
        clear_rows: None,
        placements: None,
        grey: true,
        route_name: None,
        annotations: Vec::new(),
        est: None,
    };
    let built = build_opener(&input(vec![root.clone()], 0, "IIIII")).expect("valid input");
    assert_eq!(built.status, BuildStatus::Complete);
    assert_eq!(built.steps.len(), 5);

    // The perfect clear opener's first bag as a bare outline: one bag fills
    // it, but no letter guides the default order search.
    let pco = OpenerTreeNode {
        pieces: 7,
        rows: ["ZZ____JJLL", "TZZ___JOOL", "TTSS__JOOL", "TSS___IIII"]
            .iter()
            .map(|row| row.replace(|cell: char| cell != '_', "X"))
            .collect(),
        ..root
    };
    let mut default = input(vec![pco], 0, "");
    default.default_queue = true;
    let built = build_opener(&default).expect("valid input");
    assert_eq!(built.status, BuildStatus::Undetermined);
    assert_eq!(built.reasons, [BuildReason::SearchLimit]);
    assert!(built.steps.is_empty() && built.queue.is_empty());
    assert!(built.stuck.is_none());
}

/// Live Stickspin first bags: node 0 (six pieces) then node 1, its TSS; and
/// node 4, a root that ends in a plain single.
const STICKSPIN_FIRST_BAGS: &str = r#"[{"id": 0, "parent": null, "pieces": 6, "rows": ["________S_", "I_______SS", "IJJ______S", "IJLLLZZ_OO", "IJL___ZZOO"], "placements": [{"letter": "I", "cells": [[0, 0], [0, 1], [0, 2], [0, 3]]}, {"letter": "J", "cells": [[1, 0], [1, 1], [1, 2], [2, 2]]}, {"letter": "L", "cells": [[2, 0], [2, 1], [3, 1], [4, 1]]}, {"letter": "Z", "cells": [[6, 0], [7, 0], [5, 1], [6, 1]]}, {"letter": "O", "cells": [[8, 0], [9, 0], [8, 1], [9, 1]]}, {"letter": "S", "cells": [[9, 2], [8, 3], [9, 3], [8, 4]]}], "est": {"attack": 0, "cumAttack": 0, "b2b": -1, "combo": -1}}, {"id": 1, "parent": 0, "pieces": 7, "rows": ["________S_", "I______TSS", "IJJ____TTS", "IJL___ZZOO"], "preClearRows": ["________S_", "I______TSS", "IJJ____TTS", "IJLLLZZTOO", "IJL___ZZOO"], "clearRows": [1], "placements": [{"letter": "T", "cells": [[7, 1], [7, 2], [8, 2], [7, 3]]}], "est": {"attack": 2, "cumAttack": 2, "b2b": 0, "combo": 0, "clear": "T-spin single"}}, {"id": 4, "parent": null, "pieces": 7, "rows": ["________T_", "J______TTI", "JJJ____STI", "OOL___ZZSI"], "preClearRows": ["________T_", "J______TTI", "JJJ____STI", "OOLLLZZSSI", "OOL___ZZSI"], "clearRows": [1], "placements": [{"letter": "O", "cells": [[0, 0], [1, 0], [0, 1], [1, 1]]}, {"letter": "L", "cells": [[2, 0], [2, 1], [3, 1], [4, 1]]}, {"letter": "Z", "cells": [[6, 0], [7, 0], [5, 1], [6, 1]]}, {"letter": "S", "cells": [[8, 0], [7, 1], [8, 1], [7, 2]]}, {"letter": "I", "cells": [[9, 0], [9, 1], [9, 2], [9, 3]]}, {"letter": "J", "cells": [[0, 2], [1, 2], [2, 2], [0, 3]]}, {"letter": "T", "cells": [[8, 2], [7, 3], [8, 3], [8, 4]]}], "est": {"attack": 0, "cumAttack": 0, "b2b": -1, "combo": 0, "clear": "single"}}]"#;

fn stickspin_first_bags() -> Vec<OpenerTreeNode> {
    serde_json::from_str(STICKSPIN_FIRST_BAGS).expect("tree should parse")
}

#[test]
fn a_version_is_its_clears_in_order() {
    let tree = stickspin_first_bags();
    assert_eq!(versions::clear_names(&tree, 0), Some(vec![]));
    assert_eq!(
        versions::clear_names(&tree, 1),
        Some(vec!["T-spin single".to_owned()])
    );
    assert_eq!(
        versions::clear_names(&tree, 4),
        Some(vec!["single".to_owned()])
    );
    assert_eq!(
        versions::version_shapes(&tree),
        [
            (vec!["T-spin single".to_owned()], vec![1]),
            (vec!["single".to_owned()], vec![4]),
        ]
    );
}

#[test]
fn an_unnamed_clear_is_named_by_its_lines() {
    let mut tree = stickspin_first_bags();
    tree[2].est = None;
    assert_eq!(
        versions::clear_names(&tree, 4),
        Some(vec!["single".to_owned()])
    );
    tree[2].rows = vec!["__________".to_owned()];
    assert_eq!(
        versions::clear_names(&tree, 4),
        Some(vec!["single PC".to_owned()])
    );
}

/// Shapes of different versions are never built in each other's place.
#[test]
fn targets_of_different_versions_are_rejected() {
    let mut request = input(stickspin_first_bags(), 1, "IJLZOST");
    request.targets = vec![1, 4];
    assert!(matches!(
        build_opener(&request),
        Err(OpenerBuildError::MixedVersions)
    ));
    request.targets = Vec::new();
    assert!(matches!(
        build_opener(&request),
        Err(OpenerBuildError::NoTargets)
    ));
}

/// Of one version's shapes, the build takes the one the queue fits.
#[test]
fn a_version_builds_the_shape_the_queue_fits() {
    let tree = full_tree("stickspin");
    let (names, shapes) = versions::version_shapes(&tree)
        .into_iter()
        .find(|(_, shapes)| shapes.len() >= 2)
        .expect("a version with two shapes");
    let last = *shapes.last().expect("shape");
    let queue =
        working_queue(&clears::target_with_clears(&tree, false, None, last).expect("target"));
    let mut request = input(tree.clone(), shapes[0], &queue);
    request.targets = shapes.clone();
    let built = build_opener(&request).expect("valid input");
    assert_eq!(built.status, BuildStatus::Complete, "{:?}", built.reasons);
    assert!(shapes.contains(&built.target_node));
    assert_eq!(versions::clear_names(&tree, built.target_node), Some(names));
    assert!(!built.mirrored);
}

/// The mirror is the player's choice: a request for the unmirrored version
/// is planned unmirrored whatever the pieces, and a mirrored request takes
/// the mirrored queue.
#[test]
fn the_mirror_is_never_chosen_for_the_player() {
    let tree = full_tree("stickspin");
    let queue = working_queue(&clears::target_with_clears(&tree, false, None, 28).expect("target"));
    let mirrored = mirror_queue(&queue);
    let built = build_opener(&input(tree.clone(), 28, &mirrored)).expect("valid input");
    assert!(!built.mirrored);
    let mut request = input(tree, 28, &mirrored);
    request.mirrored = true;
    let built = build_opener(&request).expect("valid input");
    assert!(built.mirrored);
    assert_eq!(built.status, BuildStatus::Complete, "{:?}", built.reasons);
}

fn mirror_queue(queue: &str) -> String {
    queue
        .chars()
        .map(|letter| match letter {
            'L' => 'J',
            'J' => 'L',
            'S' => 'Z',
            'Z' => 'S',
            other => other,
        })
        .collect()
}

fn assert_whole_bags(queue: &str) {
    assert_eq!(queue.len() % 7, 0, "{queue}");
    for bag in queue.as_bytes().chunks(7) {
        let mut sorted = bag.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, b"IJLOSTZ", "{queue}");
    }
}

/// The default order is whole 7-bags and builds the version with its clears.
#[test]
fn the_default_order_is_whole_bags_that_build_the_version() {
    let tree = full_tree("stickspin");
    let mut request = input(tree, 28, "");
    request.default_queue = true;
    let built = build_opener(&request).expect("valid input");
    assert_eq!(built.status, BuildStatus::Complete, "{:?}", built.reasons);
    assert_whole_bags(&built.queue);
    assert_eq!(built.steps.len(), 14);
    let landed = built
        .steps
        .iter()
        .filter(|step| {
            step.clear
                .as_ref()
                .is_some_and(|clear| clear.catalogued.is_some())
        })
        .count();
    assert_eq!(landed, 2);
}

/// A first bag the opener needs no hold for gets an order without one.
#[test]
fn the_default_order_avoids_holds() {
    let mut request = input(stickspin_first_bags(), 1, "");
    request.default_queue = true;
    let built = build_opener(&request).expect("valid input");
    assert_eq!(built.status, BuildStatus::Complete, "{:?}", built.reasons);
    assert_whole_bags(&built.queue);
    assert!(built
        .steps
        .iter()
        .all(|step| step.hold_action == HoldActionName::None));
    assert_eq!(built.end_hold, built.start_hold);
}
