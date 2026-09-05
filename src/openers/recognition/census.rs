use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::compile::CompileObserver;
use super::graph::BridgeReason;
use super::legality::LegalityVerdict;
use crate::openers::recognition::record::edge_ref;

// The user accepted these two canonical budget exclusions on 2026-08-31.
const USER_ACCEPTED_BUDGET_EXCLUSIONS: [(&str, u32); 2] =
    [("sasasa123-634", 4), ("sasasa123-933", 2)];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EdgeRef {
    pub(crate) record: String,
    pub(crate) node_id: u32,
    pub(crate) mirrored: bool,
    pub(crate) reason: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BridgedEdges {
    pub(crate) count: u32,
    pub(crate) direct_impossible: u32,
    pub(crate) budget_exceeded: u32,
    pub(crate) frame_inconsistent: u32,
}

impl BridgedEdges {
    fn record(&mut self, reason: BridgeReason) {
        self.count = self.count.saturating_add(1);
        match reason {
            BridgeReason::DirectImpossible => {
                self.direct_impossible = self.direct_impossible.saturating_add(1);
            }
            BridgeReason::BudgetExceeded => {
                self.budget_exceeded = self.budget_exceeded.saturating_add(1);
            }
            BridgeReason::FrameInconsistent => {
                self.frame_inconsistent = self.frame_inconsistent.saturating_add(1);
            }
        }
    }
}

impl Default for BridgedEdges {
    fn default() -> Self {
        Self {
            count: 0,
            direct_impossible: 0,
            budget_exceeded: 0,
            frame_inconsistent: 0,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CompileCensus {
    pub(crate) state_total: u32,
    pub(crate) lettered_state_total: u32,
    pub(crate) grey_state_total: u32,
    pub(crate) lock_transitions: u32,
    pub(crate) epsilon_transitions: u32,
    pub(crate) branching_histogram: BTreeMap<u32, u32>,
    pub(crate) legal_order_count: u64,
    pub(crate) legality_attempts: u64,
    pub(crate) support_valid_attempts: u64,
    pub(crate) srs_legal_attempts: u64,
    pub(crate) srs_rejected_attempts: u64,
    pub(crate) unreachable_from_spawn_attempts: u64,
    pub(crate) unsupported_attempts: u64,
    pub(crate) not_a_placement_attempts: u64,
    pub(crate) endpoint_rejections: u64,
    pub(crate) support_observed_edges: u32,
    pub(crate) srs_valid_edges: u32,
    pub(crate) support_observed_without_exact_srs_edges: u32,
    pub(crate) lettered_edges: u32,
    pub(crate) direct_impossible_edges: Vec<EdgeRef>,
    pub(crate) bridge_exposed_impossible_edges: Vec<EdgeRef>,
    pub(crate) blocked_descendants: Vec<EdgeRef>,
    pub(crate) budget_exceeded_edges: Vec<EdgeRef>,
    pub(crate) bridge_exposed_budget_edges: Vec<EdgeRef>,
    pub(crate) shifted_compiled_edges: Vec<EdgeRef>,
    pub(crate) dfs_compiled_large_edges: Vec<EdgeRef>,
    pub(crate) frame_inconsistent_edges: Vec<EdgeRef>,
    pub(crate) bridge_exposed_frame_inconsistent_edges: Vec<EdgeRef>,
    pub(crate) bridged_edges: BridgedEdges,
    pub(crate) bridge_rescued_descendants: u32,
    pub(crate) max_states_per_edge: u32,
    pub(crate) max_placements_per_edge: u8,
    pub(crate) max_dfs_visits_per_edge: u32,
    pub(crate) max_total_states: u32,
    pub(crate) observed_max_states_per_edge: u32,
    pub(crate) dfs_visit_histogram: BTreeMap<String, u32>,
    pub(crate) baseline_impossible_now_compiled: u32,
    pub(crate) baseline_budget_now_compiled: u32,
    pub(crate) exact_key_buckets: u32,
    pub(crate) max_exact_key_bucket: u32,
    pub(crate) mean_exact_key_bucket: f64,
    pub(crate) epsilon_closure_max: u32,
    pub(crate) active_product_state_max: u32,
    pub(crate) compile_micros: u64,
    pub(crate) prefix_alignment_micros: u64,
    pub(crate) resident_graph_bytes: u64,
    pub(crate) epsilon_acyclic: bool,
    #[serde(skip)]
    compiled_edges: HashSet<EdgeKey>,
    pub(super) baseline_impossible_now_compiled_details: Vec<BaselineRescue>,
    pub(super) baseline_budget_now_compiled_details: Vec<BaselineRescue>,
}

impl CompileCensus {
    pub(crate) fn new(
        max_states_per_edge: u32,
        max_placements_per_edge: u8,
        max_dfs_visits_per_edge: u32,
        max_total_states: u32,
    ) -> Self {
        Self {
            state_total: 0,
            lettered_state_total: 0,
            grey_state_total: 0,
            lock_transitions: 0,
            epsilon_transitions: 0,
            branching_histogram: BTreeMap::new(),
            legal_order_count: 0,
            legality_attempts: 0,
            support_valid_attempts: 0,
            srs_legal_attempts: 0,
            srs_rejected_attempts: 0,
            unreachable_from_spawn_attempts: 0,
            unsupported_attempts: 0,
            not_a_placement_attempts: 0,
            endpoint_rejections: 0,
            support_observed_edges: 0,
            srs_valid_edges: 0,
            support_observed_without_exact_srs_edges: 0,
            lettered_edges: 0,
            direct_impossible_edges: Vec::new(),
            bridge_exposed_impossible_edges: Vec::new(),
            blocked_descendants: Vec::new(),
            budget_exceeded_edges: Vec::new(),
            bridge_exposed_budget_edges: Vec::new(),
            shifted_compiled_edges: Vec::new(),
            dfs_compiled_large_edges: Vec::new(),
            frame_inconsistent_edges: Vec::new(),
            bridge_exposed_frame_inconsistent_edges: Vec::new(),
            bridged_edges: BridgedEdges::default(),
            bridge_rescued_descendants: 0,
            max_states_per_edge,
            max_placements_per_edge,
            max_dfs_visits_per_edge,
            max_total_states,
            observed_max_states_per_edge: 0,
            dfs_visit_histogram: BTreeMap::new(),
            baseline_impossible_now_compiled: 0,
            baseline_budget_now_compiled: 0,
            exact_key_buckets: 0,
            max_exact_key_bucket: 0,
            mean_exact_key_bucket: 0.0,
            epsilon_closure_max: 0,
            active_product_state_max: 0,
            compile_micros: 0,
            prefix_alignment_micros: 0,
            resident_graph_bytes: 0,
            epsilon_acyclic: true,
            compiled_edges: HashSet::new(),
            baseline_impossible_now_compiled_details: Vec::new(),
            baseline_budget_now_compiled_details: Vec::new(),
        }
    }

    pub(crate) fn mark_compiled(
        &mut self,
        record: &crate::openers::catalog::OpenerRecord,
        node: &crate::openers::catalog::OpenerTreeNode,
        mirrored: bool,
    ) {
        self.compiled_edges
            .insert(EdgeKey::from_edge(record, node, mirrored));
    }

    pub(crate) fn record_dfs_visits(&mut self, visits: u32) {
        *self
            .dfs_visit_histogram
            .entry(dfs_visit_bucket(visits))
            .or_default() += 1;
    }

    pub(crate) fn join_path_a_baseline(&mut self, path: &Path) -> Result<(), String> {
        let baseline_bytes = std::fs::read(path)
            .map_err(|error| format!("Path A baseline missing at {}: {error}", path.display()))?;
        let baseline =
            serde_json::from_slice::<PathABaseline>(&baseline_bytes).map_err(|error| {
                format!(
                    "Path A baseline at {} is unreadable: {error}",
                    path.display()
                )
            })?;
        self.baseline_impossible_now_compiled_details = baseline
            .direct_impossible_edges
            .into_iter()
            .filter(|edge| self.compiled_edges.contains(&EdgeKey::from_ref(edge)))
            .map(|edge| self.baseline_rescue(edge))
            .collect();
        self.baseline_budget_now_compiled_details = baseline
            .budget_exceeded_edges
            .into_iter()
            .filter(|edge| self.compiled_edges.contains(&EdgeKey::from_ref(edge)))
            .map(|edge| self.baseline_rescue(edge))
            .collect();
        self.baseline_impossible_now_compiled =
            u32::try_from(self.baseline_impossible_now_compiled_details.len()).unwrap_or(u32::MAX);
        self.baseline_budget_now_compiled =
            u32::try_from(self.baseline_budget_now_compiled_details.len()).unwrap_or(u32::MAX);
        Ok(())
    }

    fn baseline_rescue(&self, edge: EdgeRef) -> BaselineRescue {
        let key = EdgeKey::from_ref(&edge);
        let shifted = self
            .shifted_compiled_edges
            .iter()
            .any(|candidate| EdgeKey::from_ref(candidate) == key);
        let large = self
            .dfs_compiled_large_edges
            .iter()
            .any(|candidate| EdgeKey::from_ref(candidate) == key);
        let mechanism = match (shifted, large) {
            (true, true) => "both",
            (true, false) => "frame-shift",
            (false, true) => "dfs-large-k",
            (false, false) => "other",
        };
        BaselineRescue {
            record: edge.record,
            node_id: edge.node_id,
            mirrored: edge.mirrored,
            mechanism: mechanism.to_owned(),
        }
    }

    pub(crate) fn reports_every_required_metric(&self) -> bool {
        self.max_states_per_edge > 0
            && self.max_placements_per_edge > 0
            && self.max_dfs_visits_per_edge > 0
            && self.max_total_states > 0
            && self.mean_exact_key_bucket.is_finite()
            && self.epsilon_closure_max > 0
            && self.resident_graph_bytes > 0
    }

    pub(crate) fn g1_failures(&self) -> Vec<String> {
        let mut failures = Vec::new();
        if !self.epsilon_acyclic {
            failures.push("epsilon graph is cyclic".to_owned());
        }
        if self.lettered_edges == 0 {
            failures.push("no lettered catalog edges were compiled".to_owned());
        } else if u64::try_from(self.direct_impossible_edges.len()).unwrap_or(u64::MAX) * 100
            > u64::from(self.lettered_edges) * 2
        {
            failures.push(format!(
                "direct impossible edge rate {}/{} exceeds 2%",
                self.direct_impossible_edges.len(),
                self.lettered_edges
            ));
        }
        let unaccepted_budget_edges = self
            .budget_exceeded_edges
            .iter()
            .filter(|edge| !is_user_accepted_budget_exclusion(edge))
            .map(budget_edge_identity)
            .collect::<Vec<_>>();
        if !unaccepted_budget_edges.is_empty() {
            failures.push(format!(
                "budget-exceeded edges: {}",
                unaccepted_budget_edges.join(", ")
            ));
        }
        if !self.mean_exact_key_bucket.is_finite() {
            failures.push("exact-key bucket metrics are non-finite".to_owned());
        }
        if self.resident_graph_bytes == 0 {
            failures.push("graph size was not recorded".to_owned());
        }
        failures
    }

    pub(crate) fn budget_exceeded_summary(&self) -> String {
        let exclusions = USER_ACCEPTED_BUDGET_EXCLUSIONS
            .iter()
            .filter(|(record, node_id)| {
                self.budget_exceeded_edges
                    .iter()
                    .any(|edge| edge.record == *record && edge.node_id == *node_id)
            })
            .map(|(record, node_id)| format!("{record}:{node_id}"))
            .collect::<Vec<_>>();
        let unaccepted = self
            .budget_exceeded_edges
            .iter()
            .filter(|edge| !is_user_accepted_budget_exclusion(edge))
            .count();
        let count = self.budget_exceeded_edges.len();

        if exclusions.is_empty() {
            count.to_string()
        } else if unaccepted == 0 {
            format!(
                "{count} (all user-accepted exclusions: {})",
                exclusions.join(", ")
            )
        } else {
            format!(
                "{count} (user-accepted exclusions: {}; unaccepted: {unaccepted})",
                exclusions.join(", ")
            )
        }
    }
}

impl CompileObserver for CompileCensus {
    fn budget_exceeded(
        &mut self,
        record: &crate::openers::catalog::OpenerRecord,
        node: &crate::openers::catalog::OpenerTreeNode,
        mirrored: bool,
        reason: &str,
        bridge_exposed: bool,
    ) {
        let edge = edge_ref(record, node, mirrored, reason);
        if bridge_exposed {
            self.bridge_exposed_budget_edges.push(edge);
        } else {
            self.budget_exceeded_edges.push(edge);
        }
    }

    fn blocked_descendant(
        &mut self,
        record: &crate::openers::catalog::OpenerRecord,
        node: &crate::openers::catalog::OpenerTreeNode,
        mirrored: bool,
    ) {
        self.blocked_descendants.push(edge_ref(
            record,
            node,
            mirrored,
            "parent has no compiled endpoint",
        ));
    }

    fn direct_impossible(
        &mut self,
        record: &crate::openers::catalog::OpenerRecord,
        node: &crate::openers::catalog::OpenerTreeNode,
        mirrored: bool,
        reason: &str,
        bridge_exposed: bool,
    ) {
        let edge = edge_ref(record, node, mirrored, reason);
        if bridge_exposed {
            self.bridge_exposed_impossible_edges.push(edge);
        } else {
            self.direct_impossible_edges.push(edge);
        }
    }

    fn legality_attempt(&mut self, support_valid: bool, verdict: &LegalityVerdict) {
        self.legality_attempts = self.legality_attempts.saturating_add(1);
        if support_valid {
            self.support_valid_attempts = self.support_valid_attempts.saturating_add(1);
        }
        match verdict {
            LegalityVerdict::Legal { .. } => {
                self.srs_legal_attempts = self.srs_legal_attempts.saturating_add(1);
            }
            LegalityVerdict::UnreachableFromSpawn => {
                self.srs_rejected_attempts = self.srs_rejected_attempts.saturating_add(1);
                self.unreachable_from_spawn_attempts =
                    self.unreachable_from_spawn_attempts.saturating_add(1);
            }
            LegalityVerdict::Unsupported => {
                self.srs_rejected_attempts = self.srs_rejected_attempts.saturating_add(1);
                self.unsupported_attempts = self.unsupported_attempts.saturating_add(1);
            }
            LegalityVerdict::NotAPlacement => {
                self.srs_rejected_attempts = self.srs_rejected_attempts.saturating_add(1);
                self.not_a_placement_attempts = self.not_a_placement_attempts.saturating_add(1);
            }
        }
    }

    fn lettered_edge(&mut self, bridge_exposed: bool) {
        if !bridge_exposed {
            self.lettered_edges = self.lettered_edges.saturating_add(1);
        }
    }

    fn edge_state_count(&mut self, count: u32) {
        self.observed_max_states_per_edge = self.observed_max_states_per_edge.max(count);
    }

    fn dfs_visits(&mut self, visits: u32) {
        self.record_dfs_visits(visits);
    }

    fn legal_orders(&mut self, count: u64) {
        self.legal_order_count = self.legal_order_count.saturating_add(count);
    }

    fn support_observed(&mut self) {
        self.support_observed_edges = self.support_observed_edges.saturating_add(1);
    }

    fn support_without_exact_srs(&mut self) {
        self.support_observed_without_exact_srs_edges = self
            .support_observed_without_exact_srs_edges
            .saturating_add(1);
    }

    fn srs_valid(
        &mut self,
        record: &crate::openers::catalog::OpenerRecord,
        node: &crate::openers::catalog::OpenerTreeNode,
        mirrored: bool,
    ) {
        self.srs_valid_edges = self.srs_valid_edges.saturating_add(1);
        self.mark_compiled(record, node, mirrored);
    }

    fn shifted_compiled(
        &mut self,
        record: &crate::openers::catalog::OpenerRecord,
        node: &crate::openers::catalog::OpenerTreeNode,
        mirrored: bool,
    ) {
        self.shifted_compiled_edges.push(edge_ref(
            record,
            node,
            mirrored,
            "physical cell shift required",
        ));
    }

    fn dfs_compiled_large(
        &mut self,
        record: &crate::openers::catalog::OpenerRecord,
        node: &crate::openers::catalog::OpenerTreeNode,
        mirrored: bool,
    ) {
        self.dfs_compiled_large_edges.push(edge_ref(
            record,
            node,
            mirrored,
            "compiled with DFS beyond Path A cap",
        ));
    }

    fn epsilon_transition(&mut self) {
        self.epsilon_transitions = self.epsilon_transitions.saturating_add(1);
    }

    fn frame_inconsistent(
        &mut self,
        record: &crate::openers::catalog::OpenerRecord,
        node: &crate::openers::catalog::OpenerTreeNode,
        mirrored: bool,
        reason: &str,
        bridge_exposed: bool,
    ) {
        let edge = edge_ref(record, node, mirrored, reason);
        let edges = if bridge_exposed {
            &mut self.bridge_exposed_frame_inconsistent_edges
        } else {
            &mut self.frame_inconsistent_edges
        };
        if !edges.iter().any(|candidate| {
            candidate.record == edge.record
                && candidate.node_id == edge.node_id
                && candidate.mirrored == edge.mirrored
        }) {
            edges.push(edge);
        }
    }

    fn bridged_edge(
        &mut self,
        _record: &crate::openers::catalog::OpenerRecord,
        _node: &crate::openers::catalog::OpenerTreeNode,
        _mirrored: bool,
        reason: BridgeReason,
    ) {
        self.bridged_edges.record(reason);
    }

    fn bridge_rescued_descendants(&mut self, state_count: u32) {
        self.bridge_rescued_descendants =
            self.bridge_rescued_descendants.saturating_add(state_count);
    }
}

fn is_user_accepted_budget_exclusion(edge: &EdgeRef) -> bool {
    USER_ACCEPTED_BUDGET_EXCLUSIONS
        .iter()
        .any(|(record, node_id)| edge.record == *record && edge.node_id == *node_id)
}

fn budget_edge_identity(edge: &EdgeRef) -> String {
    format!(
        "{}:{} mirrored={}",
        edge.record, edge.node_id, edge.mirrored
    )
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PathABaseline {
    direct_impossible_edges: Vec<EdgeRef>,
    budget_exceeded_edges: Vec<EdgeRef>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BaselineRescue {
    pub(super) record: String,
    pub(super) node_id: u32,
    pub(super) mirrored: bool,
    pub(super) mechanism: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct EdgeKey {
    record: String,
    node_id: u32,
    mirrored: bool,
}

impl EdgeKey {
    fn from_edge(
        record: &crate::openers::catalog::OpenerRecord,
        node: &crate::openers::catalog::OpenerTreeNode,
        mirrored: bool,
    ) -> Self {
        Self {
            record: record.id.clone(),
            node_id: node.id,
            mirrored,
        }
    }

    fn from_ref(edge: &EdgeRef) -> Self {
        Self {
            record: edge.record.clone(),
            node_id: edge.node_id,
            mirrored: edge.mirrored,
        }
    }
}

fn dfs_visit_bucket(visits: u32) -> String {
    match visits {
        0 => "0".to_owned(),
        1..=16 => "1-16".to_owned(),
        17..=64 => "17-64".to_owned(),
        65..=256 => "65-256".to_owned(),
        257..=1_024 => "257-1024".to_owned(),
        1_025..=4_096 => "1025-4096".to_owned(),
        4_097..=20_000 => "4097-20000".to_owned(),
        _ => "20001+".to_owned(),
    }
}
