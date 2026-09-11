use super::compile::{
    compile_recognition_subgraph, compile_recognition_subgraph_with_observer, CompileBudget,
    CompileObserver,
};
use super::compile_stages::{compile_single_record_stages, StageProfilingObserver};
use super::profile_tests::assert_graph_eq;
use crate::openers::catalog::OpenerCatalog;

fn mini_catalog() -> OpenerCatalog {
    serde_json::from_slice(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/openers/catalog-mini.json"
    )))
    .expect("catalog fixture should parse")
}

fn single(catalog: &OpenerCatalog, record_id: &str) -> OpenerCatalog {
    let record = catalog
        .openers
        .iter()
        .find(|record| record.id == record_id)
        .expect("record should exist")
        .clone();
    OpenerCatalog {
        format_version: catalog.format_version,
        openers: vec![record],
    }
}

fn over_budget_catalog() -> OpenerCatalog {
    let mut placements = String::from("[");
    for index in 0..33 {
        if index > 0 {
            placements.push(',');
        }
        placements.push_str(r#"{"letter":"T","cells":[[0,0],[1,0],[2,0],[1,1]]}"#);
    }
    placements.push(']');
    let catalog = format!(
        r#"{{"formatVersion":2,"openers":[{{"id":"over-budget","aliases":{{"en":"Over Budget"}},"shapeKey":"over-budget-shape","tree":[{{"id":0,"parent":null,"pieces":0,"rows":[]}},{{"id":1,"parent":0,"pieces":1,"rows":["IIII______"],"placements":{placements}}}]}}]}}"#,
    );
    serde_json::from_str(&catalog).expect("over-budget catalog should parse")
}

fn assert_execution_matches_normal(catalog: &OpenerCatalog, record_id: &str) {
    let budget = CompileBudget::default();
    let single_catalog = single(catalog, record_id);
    let normal = compile_recognition_subgraph(&single_catalog, &budget);
    let mut profiler = StageProfilingObserver::new();
    let profiled =
        compile_recognition_subgraph_with_observer(&single_catalog, &budget, &mut profiler);
    match (normal, profiled) {
        (Ok(normal), Ok(profiled)) => {
            assert_eq!(
                normal.budget_exceeded, profiled.budget_exceeded,
                "budget outcome for {record_id}"
            );
            assert_graph_eq(&normal.graph, &profiled.graph);
        }
        (Err(normal_error), Err(profiled_error)) => {
            assert_eq!(
                format!("{normal_error:?}"),
                format!("{profiled_error:?}"),
                "compile error for {record_id}"
            );
        }
        (normal, profiled) => panic!(
            "ok/err divergence for {record_id}: normal ok={}, profiled ok={}",
            normal.is_ok(),
            profiled.is_ok()
        ),
    }
}

#[test]
fn profiled_execution_matches_normal_graph_exactly() {
    let catalog = mini_catalog();
    assert_execution_matches_normal(&catalog, "crowbar-v2");
    assert_execution_matches_normal(&catalog, "perfect-clear-opener");
    let over_budget = over_budget_catalog();
    assert_execution_matches_normal(&over_budget, "over-budget");
}

#[test]
fn profiled_total_state_error_matches_normal() {
    let catalog = mini_catalog();
    let budget = CompileBudget {
        max_total_states: 1,
        ..CompileBudget::default()
    };
    let single_catalog = single(&catalog, "crowbar-v2");
    let normal = compile_recognition_subgraph(&single_catalog, &budget);
    let mut profiler = StageProfilingObserver::new();
    let profiled =
        compile_recognition_subgraph_with_observer(&single_catalog, &budget, &mut profiler);
    let normal_error = normal.err().expect("tiny total-state budget must fail");
    let profiled_error = profiled.err().expect("tiny total-state budget must fail");
    assert_eq!(format!("{normal_error:?}"), format!("{profiled_error:?}"));
}

struct ClockSpy {
    legality: u32,
    intern: u32,
    transition: u32,
    finish: u32,
    budget_exceeded: bool,
}

impl CompileObserver for ClockSpy {
    fn budget_exceeded(
        &mut self,
        _record: &crate::openers::catalog::OpenerRecord,
        _node: &crate::openers::catalog::OpenerTreeNode,
        _mirrored: bool,
        _reason: &str,
        _bridge_exposed: bool,
    ) {
        self.budget_exceeded = true;
    }

    fn did_exceed_budget(&self) -> bool {
        self.budget_exceeded
    }

    fn record_legality(&mut self, _elapsed: std::time::Duration) {
        self.legality = self.legality.saturating_add(1);
    }

    fn record_intern(&mut self, _elapsed: std::time::Duration) {
        self.intern = self.intern.saturating_add(1);
    }

    fn record_transition(&mut self, _elapsed: std::time::Duration) {
        self.transition = self.transition.saturating_add(1);
    }

    fn record_finish(&mut self, _elapsed: std::time::Duration) {
        self.finish = self.finish.saturating_add(1);
    }
}

#[test]
fn default_observer_runs_no_stage_clocks() {
    let catalog = mini_catalog();
    let budget = CompileBudget::default();
    let single_catalog = single(&catalog, "crowbar-v2");
    let normal = compile_recognition_subgraph(&single_catalog, &budget)
        .expect("fixture record should compile");
    let mut spy = ClockSpy {
        legality: 0,
        intern: 0,
        transition: 0,
        finish: 0,
        budget_exceeded: false,
    };
    assert!(!spy.wants_stage_profile());
    let spied = compile_recognition_subgraph_with_observer(&single_catalog, &budget, &mut spy)
        .expect("fixture record should compile");
    assert_eq!(spy.legality, 0, "no legality clock without opt-in");
    assert_eq!(spy.intern, 0, "no intern clock without opt-in");
    assert_eq!(spy.transition, 0, "no transition clock without opt-in");
    assert_eq!(spy.finish, 0, "no finish clock without opt-in");
    assert_eq!(normal.budget_exceeded, spied.budget_exceeded);
    assert_graph_eq(&normal.graph, &spied.graph);
}

fn assert_profiled_matches_normal(catalog: &OpenerCatalog, record_id: &str) {
    let budget = CompileBudget::default();
    let normal = compile_recognition_subgraph(&single(catalog, record_id), &budget);
    let profiled =
        compile_single_record_stages(catalog, record_id).expect("record should be present");
    assert_eq!(profiled.record_id, record_id);
    match normal {
        Ok(normal) => {
            assert!(profiled.error.is_none(), "{record_id}");
            assert_eq!(profiled.success, !normal.budget_exceeded, "{record_id}");
            assert_eq!(
                profiled.budget_exceeded, normal.budget_exceeded,
                "{record_id}"
            );
            assert_eq!(
                profiled.graph_states,
                normal.graph.states.len(),
                "{record_id}"
            );
            assert_eq!(
                profiled.graph_transitions,
                normal.graph.out.iter().map(Vec::len).sum::<usize>(),
                "{record_id}"
            );
            assert_eq!(
                profiled.exact_buckets,
                normal.graph.exact_index.len(),
                "{record_id}"
            );
        }
        Err(normal_error) => {
            assert!(!profiled.success, "{record_id}");
            assert_eq!(
                profiled.error.as_deref(),
                Some(normal_error.to_string().as_str()),
                "{record_id}"
            );
        }
    }
}

#[test]
fn profiled_single_record_matches_normal_graph_and_budget() {
    let catalog = mini_catalog();
    assert_profiled_matches_normal(&catalog, "crowbar-v2");
    assert_profiled_matches_normal(&catalog, "perfect-clear-opener");
}

#[test]
fn profiled_budget_exceeded_matches_normal_outcome() {
    let catalog = over_budget_catalog();
    assert_profiled_matches_normal(&catalog, "over-budget");
    let profiled =
        compile_single_record_stages(&catalog, "over-budget").expect("record should be present");
    assert!(!profiled.success);
    assert!(profiled.budget_exceeded);
}

#[test]
fn profiled_missing_record_reports_nothing() {
    let catalog = mini_catalog();
    assert!(compile_single_record_stages(&catalog, "not-in-catalog").is_none());
}

#[test]
fn stage_observer_keeps_production_observer_contracts() {
    struct DefaultObserver;
    impl CompileObserver for DefaultObserver {}

    assert!(
        !DefaultObserver.wants_legal_orders(),
        "default observer must skip legal-order DP"
    );
    assert!(
        !DefaultObserver.wants_stage_profile(),
        "default observer must not run stage clocks"
    );
    let profiler = StageProfilingObserver::new();
    assert!(
        !profiler.wants_legal_orders(),
        "stage profiling must keep legal-order DP off like production"
    );
    assert!(profiler.wants_stage_profile());
}

#[test]
fn stage_report_states_its_overlap_model() {
    let catalog = mini_catalog();
    let profiled =
        compile_single_record_stages(&catalog, "crowbar-v2").expect("record should be present");
    assert!(profiled.success);
    assert!(
        profiled.stages.leaves_sum() <= profiled.total_wall,
        "disjoint leaves must fit inside the inclusive wall time"
    );
    assert_eq!(
        profiled.other_residual,
        profiled
            .total_wall
            .checked_sub(profiled.stages.leaves_sum())
            .expect("leaves must not overlap or exceed the wall time"),
        "other must remain the documented residual, never a timed span"
    );
    assert!(
        profiled.stages.intern_calls > 0,
        "a compiled record must intern states through the timed span"
    );
}
