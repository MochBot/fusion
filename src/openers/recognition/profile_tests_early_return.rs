use super::super::cache::RecordGraphCache;
use super::super::profile::CacheRecordOutcome;
use super::super::round::recognize_round_profiled;
use super::{assessment, fixture_catalog, observation, singleton_match};
use crate::openers::phase::prepare_observations;

#[test]
fn empty_shortlist_records_selection_only() {
    let catalog = fixture_catalog();
    let assessments = vec![None];
    let observations = vec![Some(observation())];
    let prepared = prepare_observations(&observations);

    let (profiled, profile) = recognize_round_profiled(
        &RecordGraphCache::for_catalog(catalog.clone()),
        &assessments,
        None,
        &prepared,
    );

    assert!(profiled.is_none());
    assert!(profile.total_recognition.is_some());
    assert!(profile.shortlist_selection.is_some());
    assert_eq!(profile.observation_mapping, None);
    assert_eq!(profile.cache.graph_total, None);
    assert!(profile.cache.records.is_empty());
    assert_eq!(profile.align, None);
}

#[test]
fn unmappable_observations_stop_before_cache() {
    let catalog = fixture_catalog();
    let assessments = vec![Some(assessment("fixture", &[]))];
    let observations = vec![None, None];
    let prepared = prepare_observations(&observations);

    let (profiled, profile) = recognize_round_profiled(
        &RecordGraphCache::for_catalog(catalog.clone()),
        &assessments,
        None,
        &prepared,
    );

    assert!(profiled.is_none());
    assert!(profile.shortlist_selection.is_some());
    assert!(profile.observation_mapping.is_some());
    assert_eq!(profile.cache.graph_total, None);
    assert!(profile.cache.records.is_empty());
    assert_eq!(profile.align, None);
    assert_eq!(profile.result_mapping, None);
}

#[test]
fn unknown_shortlist_records_missing_without_graph_work() {
    let catalog = fixture_catalog();
    let assessments = vec![Some(assessment("ghost", &[]))];
    let observations = vec![Some(observation())];
    let prepared = prepare_observations(&observations);

    let (profiled, profile) = recognize_round_profiled(
        &RecordGraphCache::for_catalog(catalog.clone()),
        &assessments,
        Some(&singleton_match("ghost")),
        &prepared,
    );

    assert!(profiled.is_none());
    assert!(profile.cache.graph_total.is_some());
    assert_eq!(profile.cache.union_graphs, None);
    assert_eq!(profile.cache.merged_compile, None);
    assert_eq!(profile.cache.records.len(), 1);
    assert_eq!(profile.cache.records[0].id, "ghost");
    assert_eq!(
        profile.cache.records[0].outcome,
        CacheRecordOutcome::MissingCatalogRecord
    );
    assert_eq!(profile.cache.records[0].compile, None);
    assert_eq!(profile.align, None);
    assert_eq!(profile.result_mapping, None);
}
