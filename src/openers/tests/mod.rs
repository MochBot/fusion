mod catalogued_match;
mod catalogued_match_cases;
mod contextual_target_validation;
mod guide;
mod navigation;
pub(crate) mod perf;
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) mod perf_compile_stages;
mod phase;
mod search_shape_validation;
mod segments;
mod supplied_operation_validation;
