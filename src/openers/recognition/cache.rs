//! Per-snapshot memoization of compiled record graphs.
//!
//! `CompileBudget::default()` is a constant and a record's compiled graph is a
//! pure function of the record, so the installed snapshot owns one compiled
//! graph per record, compiled on first use and retained for the snapshot's
//! lifetime. Records never share interned states (intern keys carry the record
//! index), so the merged shortlist graph is the concatenation of its records'
//! graphs. Rounds within one replay repeat records heavily, and the naive path
//! pays a probe compile per record plus a merged recompile every round, so this
//! removes most per-round compile work without changing any result.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

#[cfg(test)]
use super::census::CompileCensus;
#[cfg(test)]
use super::compile::compile_recognition_subgraph;
use super::compile::{compile_borrowed_records, CompileBudget};
use super::graph::{CanonicalKey, FxMap, RecognitionGraph, StateId, Transition};
#[cfg(all(test, not(target_arch = "wasm32")))]
use super::profile::{CacheProfile, CacheRecordOutcome, CacheRecordProfile};
use crate::openers::catalog::{OpenerCatalog, OpenerRecord};

/// One record's compiled graph; `None` caches compile failure or budget
/// exclusion. The cell compiles at most once, and callers that need the same
/// cold record wait for it there, not on the whole cache.
type RecordSlot = Arc<OnceLock<Option<Arc<RecognitionGraph>>>>;

/// Compiled graphs for one immutable catalog. Missing record IDs are never
/// cached.
pub(crate) struct RecordGraphCache {
    catalog: OpenerCatalog,
    /// Held only to find or add a record's slot, never across a compile, so
    /// concurrent rounds compile different records in parallel.
    records: Mutex<HashMap<String, RecordSlot>>,
    /// First position per record id; duplicate ids keep the first occurrence.
    positions: HashMap<String, usize>,
}

impl RecordGraphCache {
    pub(crate) fn for_catalog(catalog: OpenerCatalog) -> Self {
        let mut positions = HashMap::with_capacity(catalog.openers.len());
        for (index, record) in catalog.openers.iter().enumerate() {
            positions.entry(record.id.clone()).or_insert(index);
        }
        Self {
            catalog,
            records: Mutex::default(),
            positions,
        }
    }

    pub(crate) fn catalog(&self) -> &OpenerCatalog {
        &self.catalog
    }

    fn resolve(&self, id: &str) -> Option<&OpenerRecord> {
        self.positions
            .get(id)
            .map(|&index| &self.catalog.openers[index])
    }
}

pub(crate) struct ShortlistGraph {
    pub(crate) graph: Arc<RecognitionGraph>,
    pub(crate) compile_skipped: usize,
}

impl RecordGraphCache {
    /// Returns the merged shortlist graph, compiling only records this snapshot
    /// has not compiled before. `None` mirrors the uncached contract: no
    /// eligible record, or the merged graph reached a compile budget.
    pub(crate) fn shortlist_graph(&self, shortlist: &[String]) -> Option<ShortlistGraph> {
        self.shortlist_graph_core(
            shortlist,
            #[cfg(all(test, not(target_arch = "wasm32")))]
            None,
        )
    }

    /// Native-test-only diagnostic entry sharing the single core below: the
    /// returned graph is the actual execution's graph, plus what it did.
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn shortlist_graph_profiled(
        &self,
        shortlist: &[String],
    ) -> (Option<ShortlistGraph>, CacheProfile) {
        let mut profile = CacheProfile::default();
        let graph = self.shortlist_graph_core(shortlist, Some(&mut profile));
        (graph, profile)
    }

    /// Native-test-only budget override sharing the single core below:
    /// single-record probes keep the production default while the union limit
    /// and the merged fallback run at `budget`, executing the real
    /// union/compiler seam instead of a mocked verdict.
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn shortlist_graph_profiled_with_budget(
        &self,
        shortlist: &[String],
        budget: &CompileBudget,
    ) -> (Option<ShortlistGraph>, CacheProfile) {
        let mut profile = CacheProfile::default();
        let graph = self.shortlist_graph_core_impl(shortlist, Some(&mut profile), budget);
        (graph, profile)
    }

    pub(super) fn shortlist_graph_core(
        &self,
        shortlist: &[String],
        #[cfg(all(test, not(target_arch = "wasm32")))] profile: Option<&mut CacheProfile>,
    ) -> Option<ShortlistGraph> {
        self.shortlist_graph_core_impl(
            shortlist,
            #[cfg(all(test, not(target_arch = "wasm32")))]
            profile,
            &CompileBudget::default(),
        )
    }

    fn shortlist_graph_core_impl(
        &self,
        shortlist: &[String],
        #[cfg(all(test, not(target_arch = "wasm32")))] mut profile: Option<&mut CacheProfile>,
        budget: &CompileBudget,
    ) -> Option<ShortlistGraph> {
        #[cfg(all(test, not(target_arch = "wasm32")))]
        let total_started = std::time::Instant::now();
        let mut parts: Vec<Arc<RecognitionGraph>> = Vec::with_capacity(shortlist.len());
        let mut compile_skipped = 0;
        for opener_id in shortlist {
            let Some(record) = self.resolve(opener_id) else {
                #[cfg(all(test, not(target_arch = "wasm32")))]
                if let Some(profile) = profile.as_mut() {
                    profile.records.push(CacheRecordProfile {
                        id: opener_id.clone(),
                        outcome: CacheRecordOutcome::MissingCatalogRecord,
                        compile: None,
                    });
                }
                continue;
            };
            let slot = Arc::clone(
                match self.records.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                }
                .entry(opener_id.clone())
                .or_default(),
            );
            #[cfg(all(test, not(target_arch = "wasm32")))]
            let cached = slot.get().is_some();
            #[cfg(all(test, not(target_arch = "wasm32")))]
            let cold_started = profile.as_ref().map(|_| std::time::Instant::now());
            let compiled = slot
                .get_or_init(|| compile_borrowed_single_record(record).map(Arc::new))
                .clone();
            #[cfg(all(test, not(target_arch = "wasm32")))]
            if let Some(profile) = profile.as_mut() {
                profile.records.push(CacheRecordProfile {
                    id: opener_id.clone(),
                    outcome: match (cached, compiled.is_some()) {
                        (true, true) => CacheRecordOutcome::CachedSuccess,
                        (true, false) => CacheRecordOutcome::CachedFailure,
                        (false, true) => CacheRecordOutcome::ColdSuccess,
                        (false, false) => CacheRecordOutcome::ColdFailure,
                    },
                    compile: (!cached)
                        .then_some(cold_started)
                        .flatten()
                        .map(|started| started.elapsed()),
                });
            }
            match compiled {
                Some(graph) => parts.push(graph),
                None => compile_skipped += 1,
            }
        }
        if parts.is_empty() {
            #[cfg(all(test, not(target_arch = "wasm32")))]
            if let Some(profile) = profile.as_mut() {
                profile.graph_total = Some(total_started.elapsed());
            }
            return None;
        }
        #[cfg(all(test, not(target_arch = "wasm32")))]
        let union_started = std::time::Instant::now();
        let union = if parts.len() == 1 {
            let state_count = parts[0].states.len();
            if u32::try_from(state_count).unwrap_or(u32::MAX) > budget.max_total_states {
                None
            } else {
                Some(Arc::clone(&parts[0]))
            }
        } else {
            union_graphs(&parts, budget.max_total_states).map(Arc::new)
        };
        #[cfg(all(test, not(target_arch = "wasm32")))]
        if let Some(profile) = profile.as_mut() {
            profile.union_graphs = Some(union_started.elapsed());
        }
        let graph = match union {
            Some(graph) => graph,
            // The compiler checks the total-state budget only after ordinary
            // lock insertions, so a sum above the limit does not by itself
            // decide the budget verdict; let the compiler decide it.
            None => {
                #[cfg(all(test, not(target_arch = "wasm32")))]
                let merged_started = std::time::Instant::now();
                let merged = merged_compile(self, shortlist, &parts, budget).map(Arc::new);
                #[cfg(all(test, not(target_arch = "wasm32")))]
                if let Some(profile) = profile.as_mut() {
                    profile.merged_compile = Some(merged_started.elapsed());
                }
                let Some(graph) = merged else {
                    #[cfg(all(test, not(target_arch = "wasm32")))]
                    if let Some(profile) = profile.as_mut() {
                        profile.graph_total = Some(total_started.elapsed());
                    }
                    return None;
                };
                graph
            }
        };
        #[cfg(all(test, not(target_arch = "wasm32")))]
        if let Some(profile) = profile.as_mut() {
            profile.graph_states = Some(graph.states.len());
            profile.graph_transitions = Some(graph.out.iter().map(Vec::len).sum());
            profile.graph_total = Some(total_started.elapsed());
        }
        Some(ShortlistGraph {
            graph,
            compile_skipped,
        })
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        match self.records.lock() {
            Ok(guard) => guard.len(),
            Err(poisoned) => poisoned.into_inner().len(),
        }
    }
}

/// Concatenates independently compiled record graphs into the graph that
/// compiling those records together would produce. Record identity enters
/// state interning only through the record's position, so per-record graphs
/// are disjoint and state ids shift by a constant offset per part. Returns
/// `None` when the parts sum past `max_total_states`; the caller must then
/// let the compiler apply its own budget rule.
pub(super) fn union_graphs(
    parts: &[Arc<RecognitionGraph>],
    max_total_states: u32,
) -> Option<RecognitionGraph> {
    let total_states: usize = parts.iter().map(|graph| graph.states.len()).sum();
    if u32::try_from(total_states).unwrap_or(u32::MAX) > max_total_states {
        return None;
    }
    let mut states = Vec::with_capacity(total_states);
    let mut out = Vec::with_capacity(total_states);
    let mut exact_index: FxMap<CanonicalKey, Vec<StateId>> = FxMap::default();
    for part in parts {
        let offset = states.len();
        // Each part's index lists its states in order, so appending the parts
        // in order keeps every key's states in merged order, as indexing the
        // merged states afresh would, without a key built per state.
        for (key, ids) in &part.exact_index {
            exact_index
                .entry(key.clone())
                .or_default()
                .extend(ids.iter().map(|id| StateId(id.0 + offset)));
        }
        states.extend(part.states.iter().cloned());
        out.extend(part.out.iter().map(|transitions| {
            transitions
                .iter()
                .map(|transition| Transition {
                    #[cfg(test)]
                    from: StateId(transition.from.0 + offset),
                    to: StateId(transition.to.0 + offset),
                    label: transition.label.clone(),
                })
                .collect::<Vec<_>>()
        }));
    }
    debug_assert!(states.iter().enumerate().all(|(index, state)| {
        exact_index
            .get(&state.physical_key.occupancy_key())
            .is_some_and(|ids| ids.contains(&StateId(index)))
    }));
    debug_assert_eq!(exact_index.values().map(Vec::len).sum::<usize>(), states.len());
    debug_assert!(exact_index
        .values()
        .all(|ids| ids.windows(2).all(|pair| pair[0].0 < pair[1].0)));
    let exact_index = exact_index
        .into_iter()
        .map(|(key, states)| (key, states.into_boxed_slice()))
        .collect();
    #[cfg(test)]
    let epsilon_order = (0..states.len()).map(StateId).collect();
    Some(RecognitionGraph {
        states,
        out,
        exact_index,
        #[cfg(test)]
        epsilon_order,
        #[cfg(test)]
        census: CompileCensus::new(0, 0, 0, 0),
    })
}

fn merged_compile(
    cache: &RecordGraphCache,
    shortlist: &[String],
    parts: &[Arc<RecognitionGraph>],
    budget: &CompileBudget,
) -> Option<RecognitionGraph> {
    // Eligible records are exactly those with a compiled part, in shortlist order.
    let eligible = parts
        .iter()
        .filter_map(|part| part.states.first())
        .filter_map(|state| state.origins.first())
        .map(|origin| origin.record.to_string())
        .collect::<Vec<_>>();
    // Compile ordinals follow the eligible shortlist order, including repeats.
    let records = shortlist
        .iter()
        .filter(|opener_id| eligible.contains(opener_id))
        .filter_map(|opener_id| cache.resolve(opener_id));
    let outcome = compile_borrowed_records(records, budget).ok()?;
    (!outcome.budget_exceeded).then_some(outcome.graph)
}

/// The naive per-round path: probe every record, then compile the eligible
/// ones together. Kept as the reference the cached path must reproduce.
#[cfg(test)]
pub(crate) fn uncached_shortlist_graph(
    catalog: &OpenerCatalog,
    shortlist: &[String],
) -> Option<ShortlistGraph> {
    uncached_shortlist_graph_with_budget(catalog, shortlist, &CompileBudget::default())
}

#[cfg(test)]
pub(crate) fn uncached_shortlist_graph_with_budget(
    catalog: &OpenerCatalog,
    shortlist: &[String],
    budget: &CompileBudget,
) -> Option<ShortlistGraph> {
    let mut records = Vec::with_capacity(shortlist.len());
    let mut compile_skipped = 0;
    for opener_id in shortlist {
        let Some(record) = catalog
            .openers
            .iter()
            .find(|record| record.id == *opener_id)
        else {
            continue;
        };
        if compile_single_record(catalog, record).is_some() {
            records.push(record.clone());
        } else {
            compile_skipped += 1;
        }
    }
    if records.is_empty() {
        return None;
    }
    let merged_catalog = OpenerCatalog {
        format_version: catalog.format_version,
        openers: records,
    };
    let outcome = compile_recognition_subgraph(&merged_catalog, budget).ok()?;
    if outcome.budget_exceeded {
        return None;
    }
    Some(ShortlistGraph {
        graph: Arc::new(outcome.graph),
        compile_skipped,
    })
}

/// Clone-based reference probe for the uncached path: it compiles one record
/// out of a temporary single-record catalog.
#[cfg(test)]
fn compile_single_record(
    catalog: &OpenerCatalog,
    record: &OpenerRecord,
) -> Option<RecognitionGraph> {
    let single = OpenerCatalog {
        format_version: catalog.format_version,
        openers: vec![record.clone()],
    };
    match compile_recognition_subgraph(&single, &CompileBudget::default()) {
        Ok(outcome) if !outcome.budget_exceeded => Some(outcome.graph),
        Ok(_) | Err(_) => None,
    }
}

/// Cold probe for one record the snapshot already resolved: it compiles that
/// record alone at local ordinal 0, without cloning the record or a catalog.
fn compile_borrowed_single_record(record: &OpenerRecord) -> Option<RecognitionGraph> {
    match compile_borrowed_records(std::iter::once(record), &CompileBudget::default()) {
        Ok(outcome) if !outcome.budget_exceeded => Some(outcome.graph),
        Ok(_) | Err(_) => None,
    }
}
