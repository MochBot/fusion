//! Whole-catalog compile time: every record's graph compiled one by one, the
//! serial warm-up a long labeling run pays once.
//!
//! ```bash
//! cargo test --profile fast --lib openers::tests::perf_compile_all -- --ignored --nocapture
//! ```

use std::time::Instant;

use super::perf::millis;
use crate::openers::catalog::{installed_opener_catalog, isolated_catalog_test};
use crate::openers::install_opener_runtime;

#[test]
#[ignore]
fn compile_whole_catalog_timing() {
    let _scope = isolated_catalog_test();
    install_opener_runtime(
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/openers/catalog-full.json"
        )),
        None,
    )
    .expect("catalog should install");
    let installed = installed_opener_catalog().expect("installed");
    let started = Instant::now();
    let compiled = installed
        .openers
        .iter()
        .filter(|record| {
            installed
                .compiled
                .shortlist_graph(std::slice::from_ref(&record.id))
                .is_some()
        })
        .count();
    println!(
        "compiled {compiled}/{} records one by one in {:.0} ms",
        installed.openers.len(),
        millis(started.elapsed())
    );
}
