use std::{io, path::PathBuf, process::ExitStatus};

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("cannot {operation} `{path}`: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    #[error("cannot parse `{path}` as TOML: {source}")]
    Toml {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("cannot encode JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid Foundry contract: {0}")]
    Contract(String),
    #[error("proof `{proof}` failed with {status}")]
    ProofFailed { proof: String, status: ExitStatus },
    #[error("cannot execute `{command}`: {source}")]
    Spawn { command: String, source: io::Error },
    #[error("command `{command}` returned no standard output")]
    MissingOutput { command: String },
}

pub fn io(operation: &'static str, path: impl Into<PathBuf>, source: io::Error) -> Error {
    Error::Io {
        operation,
        path: path.into(),
        source,
    }
}
