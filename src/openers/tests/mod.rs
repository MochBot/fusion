mod catalogued_match;
mod catalogued_match_cases;
mod guide;
mod navigation;
pub(crate) mod perf;
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) mod perf_compile_stages;
mod phase;
mod segments;
