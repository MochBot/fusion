use crate::board::Board;
use crate::header::Piece;
use crate::move_buffer::MoveBuffer;
use crate::openers::recognition::align::Observation;
use crate::openers::recognition::graph::{CanonicalKey, RecognitionGraph};
use crate::smear_core::generate_placements;

use super::super::report::BatteryReport;
use super::{align, best_cost, Walk};

pub(super) fn run_hybrids(graph: &RecognitionGraph, walks: &[Walk], report: &mut BatteryReport) {
    let mut pairs = Vec::new();
    for (left_index, left) in walks.iter().enumerate() {
        if left.observations.len() < 3 {
            continue;
        }
        if let Some(right) = walks
            .iter()
            .skip(left_index + 1)
            .find(|right| right.observations.len() >= 3 && family(&left.id) != family(&right.id))
        {
            pairs.push((left, right));
        }
        if pairs.len() == 20 {
            break;
        }
    }
    for (left, right) in pairs {
        let mut observations = left.observations[..2].to_vec();
        observations.extend_from_slice(&right.observations[2..]);
        let result = align(
            graph,
            &format!("{}+{}", left.id, right.id),
            &observations,
            &mut report.metrics,
        );
        let cost = best_cost(&result);
        report.hybrid.checked = report.hybrid.checked.saturating_add(1);
        report.hybrid.zero_cost = report.hybrid.zero_cost.saturating_add(u32::from(cost == 0));
        report.hybrid.best_costs.push(cost);
        let winner = result
            .hypotheses
            .first()
            .and_then(|hypothesis| hypothesis.origin.as_ref());
        match winner.map(|origin| origin.record.as_ref()) {
            Some(record) if record == left.id => report.hybrid.x_wins += 1,
            Some(record) if record == right.id => report.hybrid.y_wins += 1,
            _ => report.hybrid.other_wins += 1,
        }
    }
}

pub(super) fn run_transpositions(
    graph: &RecognitionGraph,
    walks: &[Walk],
    report: &mut BatteryReport,
) {
    for walk in walks {
        for (first, second) in &walk.transpositions {
            if report.transposition.checked == 10 {
                return;
            }
            let first_result = align(graph, &walk.id, first, &mut report.metrics);
            let second_result = align(graph, &walk.id, second, &mut report.metrics);
            let valid = best_cost(&first_result) == 0 && best_cost(&second_result) == 0;
            report.transposition.checked += 1;
            report.transposition.equal_zero_cost += u32::from(valid);
            report.transposition.records.push(walk.id.clone());
        }
    }
}

pub(super) fn run_negatives(graph: &RecognitionGraph, report: &mut BatteryReport) {
    for seed in 0..25u32 {
        let observations = non_catalog_stack(seed);
        let result = align(
            graph,
            &format!("negative-{seed}"),
            &observations,
            &mut report.metrics,
        );
        let unknown_wins = result
            .hypotheses
            .first()
            .is_some_and(|hypothesis| hypothesis.origin.is_none());
        report.negatives.checked += 1;
        report.negatives.unknown_wins += u32::from(unknown_wins);
        for hypothesis in &result.hypotheses {
            if hypothesis.total_cost == 0 {
                if let Some(origin) = &hypothesis.origin {
                    report
                        .negatives
                        .zero_cost_collisions
                        .push(format!("seed {seed}: {}", origin.record));
                }
            }
        }
    }
    report.negatives.zero_cost_collisions.sort();
    report.negatives.zero_cost_collisions.dedup();
}

fn family(id: &str) -> &str {
    id.split(['-', '_']).next().unwrap_or(id)
}

fn non_catalog_stack(seed: u32) -> Vec<Option<Observation>> {
    let mut board = Board::new();
    let pieces = [
        Piece::T,
        Piece::S,
        Piece::Z,
        Piece::J,
        Piece::L,
        Piece::I,
        Piece::O,
    ];
    let mut observations = Vec::new();
    let mut state = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
    for lock in 0..16usize {
        let mut moves = MoveBuffer::new();
        let seed_offset = usize::try_from(seed).unwrap_or_default();
        generate_placements(
            &board,
            &mut moves,
            pieces[(lock + seed_offset) % pieces.len()],
            false,
        );
        let candidates = moves.iter().copied().collect::<Vec<_>>();
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let selected = candidates[usize::try_from(state).unwrap_or_default() % candidates.len()];
        board.lock(&selected);
        if lock >= 11 {
            observations.push(Some(Observation {
                key: CanonicalKey::from_board(&board),
                had_garbage: false,
            }));
        }
    }
    observations
}
