use super::super::cache::RecordGraphCache;
use super::super::profile::CacheRecordOutcome;
use super::super::round::{recognize_round, recognize_round_profiled};
use super::{
    assessment, fixture_catalog, legal_observations, mini_catalog, observation, singleton_match,
};
use crate::openers::catalogued_match::match_catalogued_boards;
use crate::openers::phase::assess_opener_phase;
use crate::openers::target::build_targets;

#[test]
fn profiled_recognition_matches_normal_on_fresh_and_warm_caches() {
    let catalog = fixture_catalog();
    let assessments = vec![Some(assessment("fixture", &[]))];
    let observations = vec![Some(observation())];
    let normal_cache = RecordGraphCache::default();
    let profiled_cache = RecordGraphCache::default();

    let normal = recognize_round(
        Some(&catalog),
        &normal_cache,
        &assessments,
        None,
        &observations,
    );
    let (profiled, profile) = recognize_round_profiled(
        Some(&catalog),
        &profiled_cache,
        &assessments,
        None,
        &observations,
    );

    assert!(normal.is_some());
    assert_eq!(
        serde_json::to_vec(&normal).expect("recognition should serialize"),
        serde_json::to_vec(&profiled).expect("recognition should serialize")
    );
    assert_eq!(profile.shortlist, ["fixture"]);
    assert_eq!(profile.cache.records.len(), 1);
    assert_eq!(
        profile.cache.records[0].outcome,
        CacheRecordOutcome::ColdSuccess
    );
    assert!(profile.cache.records[0].compile.is_some());
    assert!(profile.total_recognition.is_some());
    assert!(profile.shortlist_selection.is_some());
    assert!(profile.observation_mapping.is_some());
    assert!(profile.cache.graph_total.is_some());
    assert!(profile.align.is_some());
    assert!(profile.result_mapping.is_some());

    let warm_normal = recognize_round(
        Some(&catalog),
        &normal_cache,
        &assessments,
        None,
        &observations,
    );
    let (warm_profiled, warm_profile) = recognize_round_profiled(
        Some(&catalog),
        &profiled_cache,
        &assessments,
        None,
        &observations,
    );

    assert_eq!(
        serde_json::to_vec(&warm_normal).expect("recognition should serialize"),
        serde_json::to_vec(&warm_profiled).expect("recognition should serialize")
    );
    assert_eq!(warm_profile.cache.records.len(), 1);
    assert_eq!(
        warm_profile.cache.records[0].outcome,
        CacheRecordOutcome::CachedSuccess
    );
    assert_eq!(warm_profile.cache.records[0].compile, None);
}

#[test]
fn profiled_shortlist_preserves_dedup_cap_and_order() {
    let catalog = fixture_catalog();
    let mut assessments = vec![Some(assessment("fixture", &["runner", "fixture"]))];
    assessments.extend((2..25).map(|index| Some(assessment(&format!("record-{index}"), &[]))));
    let observations = vec![Some(observation())];
    let matched = singleton_match("fixture");

    let normal = recognize_round(
        Some(&catalog),
        &RecordGraphCache::default(),
        &assessments,
        Some(&matched),
        &observations,
    );
    let (profiled, profile) = recognize_round_profiled(
        Some(&catalog),
        &RecordGraphCache::default(),
        &assessments,
        Some(&matched),
        &observations,
    );

    assert_eq!(
        serde_json::to_vec(&normal).expect("recognition should serialize"),
        serde_json::to_vec(&profiled).expect("recognition should serialize")
    );
    let recognition = profiled.expect("fixture shortlist should produce recognition");
    assert_eq!(recognition.shortlist_size, 24);
    assert!(recognition.truncated);
    let mut expected = vec!["fixture".to_owned(), "runner".to_owned()];
    expected.extend((2..24).map(|index| format!("record-{index}")));
    assert_eq!(profile.shortlist, expected);
}

#[test]
fn profiled_singleton_confirmed_identity_leads_shortlist() {
    let catalog = fixture_catalog();
    let assessments = vec![Some(assessment("other", &[]))];
    let observations = vec![Some(observation())];
    let matched = singleton_match("fixture");

    let (profiled, profile) = recognize_round_profiled(
        Some(&catalog),
        &RecordGraphCache::default(),
        &assessments,
        Some(&matched),
        &observations,
    );

    assert!(profiled.is_some());
    assert_eq!(
        profile.shortlist.first().map(String::as_str),
        Some("fixture")
    );
}

#[test]
fn profiled_null_slots_match_normal() {
    let catalog = fixture_catalog();
    let assessments = vec![
        Some(assessment("fixture", &[])),
        None,
        Some(assessment("fixture", &[])),
    ];
    let observations = vec![Some(observation()), None, Some(observation())];

    let normal = recognize_round(
        Some(&catalog),
        &RecordGraphCache::default(),
        &assessments,
        None,
        &observations,
    );
    let (profiled, _) = recognize_round_profiled(
        Some(&catalog),
        &RecordGraphCache::default(),
        &assessments,
        None,
        &observations,
    );

    assert_eq!(
        serde_json::to_vec(&normal).expect("recognition should serialize"),
        serde_json::to_vec(&profiled).expect("recognition should serialize")
    );
    assert_eq!(
        profiled
            .expect("some slots should produce recognition")
            .per_lock
            .len(),
        2
    );
}

#[test]
fn profiled_legal_walk_matches_normal_with_all_spans() {
    let catalog = mini_catalog();
    let record = catalog
        .openers
        .iter()
        .find(|record| record.id == "crowbar-v2")
        .expect("catalog fixture should contain crowbar")
        .clone();
    let observations = legal_observations(&record);
    let assessments = assess_opener_phase(&build_targets(&catalog), &observations);
    let matched = match_catalogued_boards(&catalog, &observations);

    let normal = recognize_round(
        Some(&catalog),
        &RecordGraphCache::default(),
        &assessments,
        matched.as_ref(),
        &observations,
    );
    let profiled_cache = RecordGraphCache::default();
    let (profiled, profile) = recognize_round_profiled(
        Some(&catalog),
        &profiled_cache,
        &assessments,
        matched.as_ref(),
        &observations,
    );

    assert!(normal.is_some());
    assert_eq!(
        serde_json::to_vec(&normal).expect("recognition should serialize"),
        serde_json::to_vec(&profiled).expect("recognition should serialize")
    );
    assert!(profile.total_recognition.is_some());
    assert!(profile.shortlist_selection.is_some());
    assert!(profile.observation_mapping.is_some());
    assert!(profile.cache.graph_total.is_some());
    assert!(profile.align.is_some());
    assert!(profile.result_mapping.is_some());
    assert!(!profile.shortlist.is_empty());
    assert!(profile
        .cache
        .records
        .iter()
        .all(|record| record.outcome == CacheRecordOutcome::ColdSuccess));

    let (warm, warm_profile) = recognize_round_profiled(
        Some(&catalog),
        &profiled_cache,
        &assessments,
        matched.as_ref(),
        &observations,
    );
    assert_eq!(
        serde_json::to_vec(&profiled).expect("recognition should serialize"),
        serde_json::to_vec(&warm).expect("recognition should serialize")
    );
    assert!(warm_profile
        .cache
        .records
        .iter()
        .all(|record| record.compile.is_none()));
    assert!(warm_profile.cache.records.iter().all(|record| matches!(
        record.outcome,
        CacheRecordOutcome::CachedSuccess | CacheRecordOutcome::MissingCatalogRecord
    )));
}
