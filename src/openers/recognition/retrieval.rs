use super::graph::{CanonicalKey, RecognitionGraph, StateId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SeedBudget {
    pub(crate) max_seeds: u32,
}

impl Default for SeedBudget {
    fn default() -> Self {
        Self { max_seeds: 8_192 }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Seeds {
    pub(crate) exact: Vec<StateId>,
    pub(crate) retained: Vec<StateId>,
    pub(crate) unknown: bool,
    pub(crate) truncated: bool,
}

pub(crate) fn seed_candidates(
    graph: &RecognitionGraph,
    observed: &CanonicalKey,
    retained: &[StateId],
    budget: &SeedBudget,
) -> Seeds {
    let maximum = usize::try_from(budget.max_seeds).unwrap_or(usize::MAX);
    let exact_hits = graph
        .exact_index
        .get(&observed.occupancy_key())
        .map_or(&[][..], |states| states.as_ref());
    let mut seeds = Seeds {
        exact: Vec::new(),
        retained: Vec::new(),
        unknown: true,
        truncated: false,
    };

    seeds.exact.extend_from_slice(exact_hits);
    seeds.exact.sort_by_key(|state| state.0);
    seeds.exact.dedup();
    if seeds.exact.len() > maximum {
        seeds.truncated = true;
    }
    for state in retained {
        if seeds.exact.contains(state) || seeds.retained.contains(state) {
            continue;
        }
        if seeds.exact.len() + seeds.retained.len() >= maximum {
            seeds.truncated = true;
            continue;
        }
        seeds.retained.push(*state);
    }

    seeds
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{seed_candidates, SeedBudget};
    use crate::openers::catalog::OpenerCatalog;
    use crate::openers::recognition::census::CompileCensus;
    use crate::openers::recognition::graph::{CanonicalKey, RecognitionGraph, StateId};

    #[test]
    fn seed_candidates_returns_a_single_exact_hit_before_retained_states() {
        let observed = canonical_key(&[0b0000000001]);
        let graph = graph_with_exact(observed.clone(), vec![StateId(0)]);

        let seeds = seed_candidates(
            &graph,
            &observed,
            &[StateId(0), StateId(1)],
            &SeedBudget::default(),
        );

        assert_eq!(seeds.exact, [StateId(0)]);
        assert_eq!(seeds.retained, [StateId(1)]);
        assert!(seeds.unknown);
        assert!(!seeds.truncated);
    }

    #[test]
    fn seed_candidates_preserves_all_states_in_a_compiled_collision_bucket() {
        let catalog = catalog_from_json(
            r#"{
                "formatVersion":2,
                "openers":[
                    {"id":"first","aliases":{"en":"first"},"shapeKey":"fixture","tree":[{"id":0,"parent":null,"pieces":0,"rows":["I_________"],"placements":[]}]},
                    {"id":"second","aliases":{"en":"second"},"shapeKey":"fixture","tree":[{"id":0,"parent":null,"pieces":0,"rows":["I_________"],"placements":[]}]}
                ]
            }"#,
        );
        let graph = super::super::compile_catalog_census(&catalog).unwrap();
        let observed = graph.states[0].physical_key.clone();

        let seeds = seed_candidates(&graph, &observed, &[], &SeedBudget::default());

        assert_eq!(seeds.exact.len(), 4);
        assert!(seeds.exact.iter().any(|state| {
            graph.states[state.0]
                .origins
                .iter()
                .any(|origin| origin.record.as_ref() == "first")
        }));
        assert!(seeds.exact.iter().any(|state| {
            graph.states[state.0]
                .origins
                .iter()
                .any(|origin| origin.record.as_ref() == "second")
        }));
    }

    #[test]
    fn seed_candidates_returns_unknown_without_candidates_on_an_exact_miss() {
        let graph = graph_with_exact(canonical_key(&[0b0000000001]), vec![StateId(0)]);

        let seeds = seed_candidates(
            &graph,
            &canonical_key(&[0b0000000010]),
            &[],
            &SeedBudget::default(),
        );

        assert!(seeds.exact.is_empty());
        assert!(seeds.retained.is_empty());
        assert!(seeds.unknown);
        assert!(!seeds.truncated);
    }

    #[test]
    fn seed_candidates_truncates_after_exact_hits_before_retained_states() {
        let observed = canonical_key(&[0b0000000001]);
        let graph = graph_with_exact(observed.clone(), vec![StateId(0)]);
        let budget = SeedBudget { max_seeds: 1 };

        let seeds = seed_candidates(&graph, &observed, &[StateId(1)], &budget);

        assert_eq!(seeds.exact, [StateId(0)]);
        assert!(seeds.retained.is_empty());
        assert!(seeds.unknown);
        assert!(seeds.truncated);
    }

    #[test]
    fn seed_candidates_keeps_every_exact_tie_past_the_budget() {
        let observed = canonical_key(&[0b0000000001]);
        let graph = graph_with_exact(observed.clone(), vec![StateId(0), StateId(1)]);

        let seeds = seed_candidates(&graph, &observed, &[], &SeedBudget { max_seeds: 1 });

        assert_eq!(seeds.exact, [StateId(0), StateId(1)]);
        assert!(seeds.truncated);
    }

    fn canonical_key(masks: &[u16]) -> CanonicalKey {
        CanonicalKey {
            masks: masks.into(),
            letters: None,
        }
    }

    fn graph_with_exact(observed: CanonicalKey, exact: Vec<StateId>) -> RecognitionGraph {
        RecognitionGraph {
            states: Vec::new(),
            out: Vec::new(),
            exact_index: HashMap::from([(observed, exact.into())]),
            epsilon_order: Vec::new().into(),
            census: CompileCensus::new(1, 1, 1, 1),
        }
    }

    fn catalog_from_json(raw: &str) -> OpenerCatalog {
        serde_json::from_str(raw).unwrap()
    }
}
