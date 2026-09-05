use super::report::write_report;
use super::walk::{
    add_lowest_column_zero_cell, substitute_occupied_letter, synthesize_observations,
    synthesize_record, SynthesisFuel,
};
use super::{run_behavioral_battery, run_mini_battery};
use crate::openers::catalog::OpenerCatalog;
use crate::openers::recognition::compile_catalog_census;

const DENSE_LATTICE_FUEL: u32 = 64;

#[test]
fn mini_catalog_battery_reports_a_zero_cost_legal_walk() {
    let graph = compile_catalog_census(&mini_catalog()).unwrap();

    let report = run_mini_battery(&graph, "single-o", false).unwrap();

    assert_eq!(report.legal.checked, 1);
    assert_eq!(report.legal.zero_cost, 1);
    assert!(report.hard.legal_walks);
}

#[test]
fn synthesized_walk_perturbations_change_only_the_intended_observation_shape() {
    let graph = compile_catalog_census(&mini_catalog()).unwrap();
    let observations = synthesize_observations(&graph, "single-o", false).unwrap();
    let observation = observations[0].as_ref().unwrap();

    let substitution = substitute_occupied_letter(observation).unwrap();
    let extra = add_lowest_column_zero_cell(observation);

    assert_eq!(substitution.key.masks, observation.key.masks);
    assert!(substitution.key.letters.is_some());
    assert_ne!(extra.key.masks, observation.key.masks);
}

#[test]
fn synthesis_stops_at_the_dense_lattice_fuel_bound() {
    let graph = compile_catalog_census(&dense_lattice_catalog()).unwrap();
    let mut fuel = SynthesisFuel::new(DENSE_LATTICE_FUEL);

    let synthesis = synthesize_record(&graph, "dense-o", false, 50, &mut fuel);

    assert!(synthesis.is_none());
    assert!(fuel.stats().fuel_exhausted);
    assert!(fuel.stats().visits <= DENSE_LATTICE_FUEL);
}

#[test]
#[ignore]
fn behavioral_battery() {
    let started = std::time::Instant::now();
    let output = std::env::var_os("OPENER_BATTERY_OUT")
        .expect("OPENER_BATTERY_OUT must name the evidence output directory");
    let catalog = serde_json::from_slice::<OpenerCatalog>(include_bytes!(
        "../../../../fixtures/openers/catalog-full.json"
    ))
    .unwrap();
    eprintln!("[battery] global compile start");
    let graph = compile_catalog_census(&catalog).unwrap();
    eprintln!(
        "[battery] global compile finish {}ms",
        started.elapsed().as_millis()
    );
    let report = run_behavioral_battery(&graph, &catalog);
    write_report(&report, std::path::Path::new(&output)).unwrap();

    assert!(report.hard.legal_walks, "legal walk hard assertion failed");
    assert!(
        report.hard.substitution_retention,
        "substitution retention hard assertion failed"
    );
    assert!(
        report.hard.missing_retention,
        "missing retention hard assertion failed"
    );
    assert!(report.hard.hybrid_nonzero, "hybrid hard assertion failed");
    assert!(report.hard.rejoin_nonzero, "rejoin hard assertion failed");
    assert!(
        report.hard.transposition_zero_cost,
        "transposition hard assertion failed"
    );
    assert!(
        report.hard.negatives_unknown,
        "negative hard assertion failed"
    );
    assert!(
        report.hard.legal_exact_without_zero_cost_eviction,
        "legal exactness hard assertion failed"
    );
}

fn mini_catalog() -> OpenerCatalog {
    serde_json::from_str(
        r#"{
            "formatVersion": 2,
            "openers": [{
                "id": "single-o",
                "aliases": {"en": "single o"},
                "shapeKey": "fixture",
                "tree": [
                    {"id": 0, "parent": null, "pieces": 0, "rows": [], "placements": []},
                    {"id": 1, "parent": 0, "pieces": 1, "rows": ["OO________", "OO________"], "placements": [{"letter": "O", "cells": [[0, 0], [1, 0], [0, 1], [1, 1]]}]}
                ]
            }]
        }"#,
    )
    .unwrap()
}

fn dense_lattice_catalog() -> OpenerCatalog {
    serde_json::from_str(
        r#"{
            "formatVersion": 2,
            "openers": [{
                "id": "dense-o",
                "aliases": {"en": "dense o"},
                "shapeKey": "fixture",
                "tree": [
                    {"id": 0, "parent": null, "pieces": 0, "rows": [], "placements": []},
                    {"id": 1, "parent": 0, "pieces": 10, "rows": ["OOOOOOOOOO", "OOOOOOOOOO", "OOOOOOOOOO", "OOOOOOOOOO"], "placements": [
                        {"letter": "O", "cells": [[0, 0], [1, 0], [0, 1], [1, 1]]},
                        {"letter": "O", "cells": [[2, 0], [3, 0], [2, 1], [3, 1]]},
                        {"letter": "O", "cells": [[4, 0], [5, 0], [4, 1], [5, 1]]},
                        {"letter": "O", "cells": [[6, 0], [7, 0], [6, 1], [7, 1]]},
                        {"letter": "O", "cells": [[8, 0], [9, 0], [8, 1], [9, 1]]},
                        {"letter": "O", "cells": [[0, 2], [1, 2], [0, 3], [1, 3]]},
                        {"letter": "O", "cells": [[2, 2], [3, 2], [2, 3], [3, 3]]},
                        {"letter": "O", "cells": [[4, 2], [5, 2], [4, 3], [5, 3]]},
                        {"letter": "O", "cells": [[6, 2], [7, 2], [6, 3], [7, 3]]},
                        {"letter": "O", "cells": [[8, 2], [9, 2], [8, 3], [9, 3]]}
                    ]}
                ]
            }]
        }"#,
    )
    .unwrap()
}
