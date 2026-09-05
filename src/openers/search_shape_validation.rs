mod contextual;
mod direct_target;
mod flow;
mod frames;
mod model;
mod run;
mod search;
mod verify;
mod witness;

pub use contextual::{
    validate_contextual_target_batch, ContextualTargetResultsV1, ContextualTargetValidationError,
};
pub use model::{ValidationError, ValidationResultsV1};

/// Validates an explicit schema-v1 proposed search-shape batch against a v2 catalog.
pub fn validate_search_shape_batch(
    catalog_json: &[u8],
    batch_json: &[u8],
) -> Result<ValidationResultsV1, ValidationError> {
    run::validate(catalog_json, batch_json)
}
