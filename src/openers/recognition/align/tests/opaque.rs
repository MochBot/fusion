use super::super::{align_round_exact, AlignBudget, Observation};
use crate::openers::catalog::OpenerCatalog;
use crate::openers::recognition::compile_catalog_census;
use crate::openers::recognition::cost::EditCosts;

#[test]
fn opaque_exact_state_charges_entry_cost_and_retains_opaque_evidence() {
    let graph = compile_catalog_census(&opaque_catalog()).unwrap();
    let state = graph
        .states
        .iter()
        .position(|state| state.identity_opaque)
        .unwrap();
    let result = align_round_exact(
        &graph,
        &EditCosts::default(),
        &[Some(Observation {
            key: graph.states[state].physical_key.clone(),
            had_garbage: false,
        })],
        &AlignBudget::default(),
    );
    let best = result.hypotheses.first().unwrap();

    assert_eq!(best.total_cost, 2);
    assert_eq!(best.opaque_steps, 1);
    assert!(best.identity_frozen);
}

fn opaque_catalog() -> OpenerCatalog {
    serde_json::from_str(
        r#"{"formatVersion":2,"openers":[{"id":"opaque","aliases":{"en":"opaque"},"shapeKey":"fixture","tree":[{"id":0,"parent":null,"pieces":0,"rows":["XXXXXXXXXX"],"grey":true}]}]}"#,
    )
    .unwrap()
}
