use std::collections::HashMap;
use std::mem::size_of;
use std::time::Instant;

use super::census::CompileCensus;
use super::graph::{GraphBuilder, TransitionLabel};

pub(super) fn finalize_census(
    census: &mut CompileCensus,
    builder: &GraphBuilder,
    started: Instant,
) {
    census.state_total = u32::try_from(builder.state_count()).unwrap_or(u32::MAX);
    census.lettered_state_total = u32::try_from(
        builder
            .states()
            .iter()
            .filter(|state| !state.identity_opaque)
            .count(),
    )
    .unwrap_or(u32::MAX);
    census.grey_state_total = census
        .state_total
        .saturating_sub(census.lettered_state_total);
    census.lock_transitions = u32::try_from(
        builder
            .out()
            .iter()
            .flatten()
            .filter(|transition| matches!(transition.label, TransitionLabel::Lock { .. }))
            .count(),
    )
    .unwrap_or(u32::MAX);
    for transitions in builder.out() {
        let degree = u32::try_from(transitions.len()).unwrap_or(u32::MAX);
        *census.branching_histogram.entry(degree).or_default() += 1;
    }
    let mut bucket_sizes = HashMap::<_, u32>::new();
    for state in builder.states() {
        *bucket_sizes.entry(state.physical_key.clone()).or_default() += 1;
    }
    census.exact_key_buckets = u32::try_from(bucket_sizes.len()).unwrap_or(u32::MAX);
    census.max_exact_key_bucket = bucket_sizes.values().copied().max().unwrap_or(0);
    census.mean_exact_key_bucket = if bucket_sizes.is_empty() {
        0.0
    } else {
        f64::from(census.state_total) / f64::from(census.exact_key_buckets)
    };
    census.epsilon_acyclic = epsilon_is_acyclic(builder);
    census.epsilon_closure_max = if builder.state_count() == 0 { 0 } else { 1 };
    census.active_product_state_max = 0;
    census.prefix_alignment_micros = 0;
    census.resident_graph_bytes = resident_bytes(builder);
    census.compile_micros = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
}

fn epsilon_is_acyclic(builder: &GraphBuilder) -> bool {
    let mut indegree = vec![0u32; builder.state_count()];
    for transitions in builder.out() {
        for transition in transitions {
            if matches!(
                transition.label,
                TransitionLabel::Epsilon { .. } | TransitionLabel::Bridge { .. }
            ) {
                indegree[transition.to.0] += 1;
            }
        }
    }
    let mut stack = indegree
        .iter()
        .enumerate()
        .filter_map(|(index, degree)| (*degree == 0).then_some(index))
        .collect::<Vec<_>>();
    let mut visited = 0usize;
    while let Some(state) = stack.pop() {
        visited += 1;
        for transition in &builder.out()[state] {
            if !matches!(
                transition.label,
                TransitionLabel::Epsilon { .. } | TransitionLabel::Bridge { .. }
            ) {
                continue;
            }
            let target = transition.to.0;
            indegree[target] -= 1;
            if indegree[target] == 0 {
                stack.push(target);
            }
        }
    }
    visited == builder.state_count()
}

fn resident_bytes(builder: &GraphBuilder) -> u64 {
    let state_bytes = builder.states().iter().fold(0usize, |total, state| {
        let masks = state.physical_key.masks.len() * size_of::<u16>();
        let origins = state.origins.iter().fold(0usize, |sum, origin| {
            sum + size_of_val(origin)
                + origin.record.len()
                + origin.route_name.as_deref().map_or(0, str::len)
        });
        total + size_of_val(state) + masks + origins
    });
    let transition_bytes = builder
        .out()
        .iter()
        .flatten()
        .fold(0usize, |total, transition| {
            let label_bytes = match &transition.label {
                TransitionLabel::Lock {
                    letter,
                    cells,
                    cleared_rows,
                    is_pc,
                } => {
                    size_of_val(letter)
                        + size_of_val(cells)
                        + cleared_rows.len() * size_of::<u8>()
                        + size_of_val(is_pc)
                }
                TransitionLabel::Epsilon { reason } => size_of_val(reason),
                TransitionLabel::Bridge {
                    #[cfg(test)]
                    reason,
                } => {
                    #[cfg(test)]
                    {
                        size_of_val(reason)
                    }
                    #[cfg(not(test))]
                    {
                        0
                    }
                }
            };
            total + size_of_val(transition) + size_of_val(&transition.from) + label_bytes
        });
    u64::try_from(state_bytes.saturating_add(transition_bytes)).unwrap_or(u64::MAX)
}
