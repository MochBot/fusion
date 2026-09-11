//! Native-test-only recognition diagnostics.
//!
//! The profile records what the actual recognition and cache execution did:
//! which spans ran and what each shortlist record resolved to, in shortlist
//! order. `None` durations mean the span did not execute, never zero time.
//! These types never cross a serialization or WASM boundary.

use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CacheRecordOutcome {
    MissingCatalogRecord,
    CachedSuccess,
    CachedFailure,
    ColdSuccess,
    ColdFailure,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CacheRecordProfile {
    pub id: String,
    pub outcome: CacheRecordOutcome,
    /// Set only for cold outcomes; a cached or missing record compiles nothing.
    pub compile: Option<Duration>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CacheProfile {
    pub graph_total: Option<Duration>,
    pub union_graphs: Option<Duration>,
    pub merged_compile: Option<Duration>,
    pub graph_states: Option<usize>,
    pub graph_transitions: Option<usize>,
    pub records: Vec<CacheRecordProfile>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RecognitionProfile {
    pub total_recognition: Option<Duration>,
    pub shortlist_selection: Option<Duration>,
    pub observation_mapping: Option<Duration>,
    pub align: Option<Duration>,
    pub result_mapping: Option<Duration>,
    pub shortlist: Vec<String>,
    pub cache: CacheProfile,
}
