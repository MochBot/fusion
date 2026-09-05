use std::env;
use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;

use fusion_engine::openers::{validate_contextual_target_batch, ContextualTargetValidationError};

fn main() -> Result<(), CliError> {
    let paths = parse_args(env::args_os().skip(1))?;
    let catalog = std::fs::read(&paths.catalog).map_err(|source| CliError::Read {
        path: paths.catalog.clone(),
        source,
    })?;
    let input = std::fs::read(&paths.input).map_err(|source| CliError::Read {
        path: paths.input.clone(),
        source,
    })?;
    let results =
        validate_contextual_target_batch(&catalog, &input).map_err(CliError::Validation)?;
    let output = serde_json::to_vec_pretty(&results).map_err(CliError::Serialize)?;
    std::fs::write(&paths.output, output).map_err(|source| CliError::Write {
        path: paths.output,
        source,
    })?;
    Ok(())
}

struct Paths {
    catalog: PathBuf,
    input: PathBuf,
    output: PathBuf,
}

fn parse_args(arguments: impl IntoIterator<Item = OsString>) -> Result<Paths, CliError> {
    let mut catalog = None;
    let mut input = None;
    let mut output = None;
    let mut arguments = arguments.into_iter();
    while let Some(flag) = arguments.next() {
        let Some(value) = arguments.next() else {
            return Err(CliError::Usage);
        };
        match flag.to_str() {
            Some("--catalog") if catalog.is_none() => catalog = Some(PathBuf::from(value)),
            Some("--input") if input.is_none() => input = Some(PathBuf::from(value)),
            Some("--output") if output.is_none() => output = Some(PathBuf::from(value)),
            _ => return Err(CliError::Usage),
        }
    }
    match (catalog, input, output) {
        (Some(catalog), Some(input), Some(output))
            if catalog != input && catalog != output && input != output =>
        {
            Ok(Paths {
                catalog,
                input,
                output,
            })
        }
        _ => Err(CliError::Usage),
    }
}

#[derive(Debug)]
enum CliError {
    Usage,
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    Validation(ContextualTargetValidationError),
    Serialize(serde_json::Error),
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage => write!(
                formatter,
                "usage: validate_contextual_target_batch --catalog CATALOG.json --input BATCH.json --output RESULTS.json"
            ),
            Self::Read { path, source } => {
                write!(formatter, "cannot read {}: {source}", path.display())
            }
            Self::Write { path, source } => {
                write!(formatter, "cannot write {}: {source}", path.display())
            }
            Self::Validation(error) => write!(formatter, "validation failed: {error}"),
            Self::Serialize(error) => write!(formatter, "cannot serialize results: {error}"),
        }
    }
}

impl std::error::Error for CliError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Usage => None,
            Self::Read { source, .. } | Self::Write { source, .. } => Some(source),
            Self::Validation(error) => Some(error),
            Self::Serialize(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_args;
    use std::ffi::OsString;

    #[test]
    fn accepts_three_distinct_paths() {
        let result = parse_args([
            OsString::from("--catalog"),
            OsString::from("catalog.json"),
            OsString::from("--input"),
            OsString::from("input.json"),
            OsString::from("--output"),
            OsString::from("output.json"),
        ]);
        assert!(result.is_ok());
    }
}
