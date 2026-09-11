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
use std::sync::{Arc, Mutex};

#[cfg(test)]
use super::census::CompileCensus;
use super::compile::{compile_recognition_subgraph, CompileBudget};
use super::graph::{CanonicalKey, RecognitionGraph, StateId, Transition};
#[cfg(all(test, not(target_arch = "wasm32")))]
use super::profile::{CacheProfile, CacheRecordOutcome, CacheRecordProfile};
use crate::openers::catalog::{OpenerCatalog, OpenerRecord};

/// Compiled record graphs by record id; `None` marks a record that failed or
/// reached a compile budget and is excluded from every shortlist.
#[derive(Default)]
pub(crate) struct RecordGraphCache {
    records: Mutex<HashMap<String, Option<Arc<RecognitionGraph>>>>,
}

pub(crate) struct ShortlistGraph {
    pub(crate) graph: RecognitionGraph,
    pub(crate) compile_skipped: usize,
}

impl RecordGraphCache {
    /// Returns the merged shortlist graph, compiling only records this snapshot
    /// has not compiled before. `None` mirrors the uncached contract: no
    /// eligible record, or the merged graph reached a compile budget.
    pub(crate) fn shortlist_graph(
        &self,
        catalog: &OpenerCatalog,
        shortlist: &[String],
    ) -> Option<ShortlistGraph> {
        self.shortlist_graph_core(
            catalog,
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
        catalog: &OpenerCatalog,
        shortlist: &[String],
    ) -> (Option<ShortlistGraph>, CacheProfile) {
        let mut profile = CacheProfile::default();
        let graph = self.shortlist_graph_core(catalog, shortlist, Some(&mut profile));
        (graph, profile)
    }

    /// Native-test-only budget override sharing the single core below:
    /// single-record probes keep the production default while the union limit
    /// and the merged fallback run at `budget`, executing the real
    /// union/compiler seam instead of a mocked verdict.
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn shortlist_graph_profiled_with_budget(
        &self,
        catalog: &OpenerCatalog,
        shortlist: &[String],
        budget: &CompileBudget,
    ) -> (Option<ShortlistGraph>, CacheProfile) {
        let mut profile = CacheProfile::default();
        let graph = self.shortlist_graph_core_impl(catalog, shortlist, Some(&mut profile), budget);
        (graph, profile)
    }

    pub(super) fn shortlist_graph_core(
        &self,
        catalog: &OpenerCatalog,
        shortlist: &[String],
        #[cfg(all(test, not(target_arch = "wasm32")))] profile: Option<&mut CacheProfile>,
    ) -> Option<ShortlistGraph> {
        self.shortlist_graph_core_impl(
            catalog,
            shortlist,
            #[cfg(all(test, not(target_arch = "wasm32")))]
            profile,
            &CompileBudget::default(),
        )
    }

    fn shortlist_graph_core_impl(
        &self,
        catalog: &OpenerCatalog,
        shortlist: &[String],
        #[cfg(all(test, not(target_arch = "wasm32")))] mut profile: Option<&mut CacheProfile>,
        budget: &CompileBudget,
    ) -> Option<ShortlistGraph> {
        #[cfg(all(test, not(target_arch = "wasm32")))]
        let total_started = std::time::Instant::now();
        let mut parts: Vec<Arc<RecognitionGraph>> = Vec::with_capacity(shortlist.len());
        let mut compile_skipped = 0;
        let mut records = match self.records.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        for opener_id in shortlist {
            let Some(record) = catalog
                .openers
                .iter()
                .find(|record| record.id == *opener_id)
            else {
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
            let compiled = match records.get(opener_id) {
                Some(compiled) => {
                    #[cfg(all(test, not(target_arch = "wasm32")))]
                    if let Some(profile) = profile.as_mut() {
                        profile.records.push(CacheRecordProfile {
                            id: opener_id.clone(),
                            outcome: if compiled.is_some() {
                                CacheRecordOutcome::CachedSuccess
                            } else {
                                CacheRecordOutcome::CachedFailure
                            },
                            compile: None,
                        });
                    }
                    compiled.clone()
                }
                None => {
                    #[cfg(all(test, not(target_arch = "wasm32")))]
                    let cold_started = profile.as_ref().map(|_| std::time::Instant::now());
                    let compiled = compile_single_record(catalog, record);
                    #[cfg(all(test, not(target_arch = "wasm32")))]
                    let cold_elapsed = cold_started.map(|started| started.elapsed());
                    #[cfg(all(test, not(target_arch = "wasm32")))]
                    if let Some(profile) = profile.as_mut() {
                        profile.records.push(CacheRecordProfile {
                            id: opener_id.clone(),
                            outcome: if compiled.is_some() {
                                CacheRecordOutcome::ColdSuccess
                            } else {
                                CacheRecordOutcome::ColdFailure
                            },
                            compile: cold_elapsed,
                        });
                    }
                    let compiled = compiled.map(Arc::new);
                    records.insert(opener_id.clone(), compiled.clone());
                    compiled
                }
            };
            match compiled {
                Some(graph) => parts.push(graph),
                None => compile_skipped += 1,
            }
        }
        drop(records);
        if parts.is_empty() {
            #[cfg(all(test, not(target_arch = "wasm32")))]
            if let Some(profile) = profile.as_mut() {
                profile.graph_total = Some(total_started.elapsed());
            }
            return None;
        }
        #[cfg(all(test, not(target_arch = "wasm32")))]
        let union_started = std::time::Instant::now();
        let union = union_graphs(&parts, budget.max_total_states);
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
                let merged = merged_compile(catalog, shortlist, &parts, budget);
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
    let mut exact_index: HashMap<CanonicalKey, Vec<StateId>> = HashMap::new();
    for part in parts {
        let offset = states.len();
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
    for (index, state) in states.iter().enumerate() {
        exact_index
            .entry(state.physical_key.occupancy_key())
            .or_default()
            .push(StateId(index));
    }
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
    catalog: &OpenerCatalog,
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
    let records = shortlist
        .iter()
        .filter(|opener_id| eligible.contains(opener_id))
        .filter_map(|opener_id| {
            catalog
                .openers
                .iter()
                .find(|record| record.id == *opener_id)
        })
        .cloned()
        .collect::<Vec<_>>();
    let merged_catalog = OpenerCatalog {
        format_version: catalog.format_version,
        openers: records,
    };
    let outcome = compile_recognition_subgraph(&merged_catalog, budget).ok()?;
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
        graph: outcome.graph,
        compile_skipped,
    })
}

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
