use std::io;
use std::path::Path;

use super::census::CompileCensus;

pub(super) fn write_census(census: &CompileCensus, output: &Path) -> io::Result<()> {
    std::fs::create_dir_all(output)?;
    let json = serde_json::to_vec_pretty(census).map_err(io::Error::other)?;
    std::fs::write(output.join("census.json"), json)?;
    let summary = format!(
        "# Opener Recognition Census\n\n\
G1: {}\n\n\
- States: {} (lettered {} / grey {})\n\
- Transitions: {} lock / {} epsilon\n\
- Legal orders: {}\n\
- Legality attempts: {}; support-valid attempts: {}; SRS-legal attempts: {}; SRS-rejected attempts: {}\n\
- Unreachable attempts: {}; unsupported attempts: {}; non-placement attempts: {}; endpoint rejections: {}\n\
- Edge support observed: {}; exact SRS-valid edges: {}; support-valid edges without an exact SRS endpoint: {}\n\
- Direct impossible edges: {} of {} lettered edges\n\
- Blocked descendants: {}\n\
- Budget-exceeded edges: {}\n\
- Shifted compiled edges: {}\n\
- DFS compiled large edges: {}\n\
- Frame-inconsistent edges: {}\n\
- Bridge-exposed impossible edges: {}\n\
- Bridge-exposed budget-exceeded edges: {}\n\
- Bridge-exposed frame-inconsistent edges: {}\n\
- Bridged edges: {} (direct impossible {}, budget exceeded {}, frame inconsistent {})\n\
- Bridge-rescued descendants: {}\n\
- DFS visit histogram: {:?}\n\
- Path A impossible edges now compiled: {}\n\
- Path A budget edges now compiled: {}\n\
- Exact-key buckets: {} (max {}, mean {:.3})\n\
- Epsilon closure maximum: {}\n\
- Active product-state maximum: {}\n\
- Compile timing: {} microseconds; prefix alignment timing: {} microseconds\n\
- Resident graph bytes: {}\n\
- Budgets: {} states/edge, {} placements/edge, {} DFS visits/edge, {} total states\n\n\
## Path A Rescues By Record\n\n\
{}\n",
        if census.g1_failures().is_empty() {
            "PASS"
        } else {
            "FAIL"
        },
        census.state_total,
        census.lettered_state_total,
        census.grey_state_total,
        census.lock_transitions,
        census.epsilon_transitions,
        census.legal_order_count,
        census.legality_attempts,
        census.support_valid_attempts,
        census.srs_legal_attempts,
        census.srs_rejected_attempts,
        census.unreachable_from_spawn_attempts,
        census.unsupported_attempts,
        census.not_a_placement_attempts,
        census.endpoint_rejections,
        census.support_observed_edges,
        census.srs_valid_edges,
        census.support_observed_without_exact_srs_edges,
        census.direct_impossible_edges.len(),
        census.lettered_edges,
        census.blocked_descendants.len(),
        census.budget_exceeded_summary(),
        census.shifted_compiled_edges.len(),
        census.dfs_compiled_large_edges.len(),
        census.frame_inconsistent_edges.len(),
        census.bridge_exposed_impossible_edges.len(),
        census.bridge_exposed_budget_edges.len(),
        census.bridge_exposed_frame_inconsistent_edges.len(),
        census.bridged_edges.count,
        census.bridged_edges.direct_impossible,
        census.bridged_edges.budget_exceeded,
        census.bridged_edges.frame_inconsistent,
        census.bridge_rescued_descendants,
        census.dfs_visit_histogram,
        census.baseline_impossible_now_compiled,
        census.baseline_budget_now_compiled,
        census.exact_key_buckets,
        census.max_exact_key_bucket,
        census.mean_exact_key_bucket,
        census.epsilon_closure_max,
        census.active_product_state_max,
        census.compile_micros,
        census.prefix_alignment_micros,
        census.resident_graph_bytes,
        census.max_states_per_edge,
        census.max_placements_per_edge,
        census.max_dfs_visits_per_edge,
        census.max_total_states,
        baseline_breakdown(census),
    );
    std::fs::write(output.join("summary.md"), summary)
}

fn baseline_breakdown(census: &CompileCensus) -> String {
    let impossible = record_breakdown(&census.baseline_impossible_now_compiled_details);
    let budget = record_breakdown(&census.baseline_budget_now_compiled_details);
    format!("- Impossible: {impossible}\n- Budget: {budget}")
}

fn record_breakdown(edges: &[super::census::BaselineRescue]) -> String {
    let mut records = std::collections::BTreeMap::<&str, u32>::new();
    for edge in edges {
        *records.entry(&edge.record).or_default() += 1;
    }
    if records.is_empty() {
        "none".to_owned()
    } else {
        records
            .into_iter()
            .map(|(record, count)| format!("{record} ({count})"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}
