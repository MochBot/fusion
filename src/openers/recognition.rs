mod align;
#[cfg(test)]
mod battery;
mod cache;
#[cfg(test)]
#[path = "recognition/cache_tests.rs"]
mod cache_tests;
#[cfg(test)]
mod census;
#[cfg(test)]
mod collision_evidence;
mod compile;
mod cost;
mod edge;
#[cfg(test)]
mod evidence;
mod frames;
mod graph;
mod legality;
#[cfg(test)]
mod metrics;
mod record;
mod retrieval;
pub(super) mod round;

#[cfg(test)]
#[path = "recognition/tests.rs"]
mod tests;

pub(crate) use cache::RecordGraphCache;
#[cfg(test)]
pub(crate) use compile::{compile_recognition_graph, CompileBudget, CompileError};
pub(crate) use graph::RecognitionGraph;

#[cfg(test)]
fn compile_catalog_census(
    catalog: &crate::openers::catalog::OpenerCatalog,
) -> Result<RecognitionGraph, CompileError> {
    compile::compile_catalog_census(catalog, &CompileBudget::default())
}
