//! Command-line orchestration for image construction and headless execution.

mod manifest;
mod runner;

pub use manifest::{ImageManifest, ModuleManifest, package_manifest};
pub use runner::{
    HeadlessAssertion, HeadlessInput, HeadlessTest, RunOptions, RunResult, run_headless, run_image,
};

use thiserror::Error;

/// Errors reported by CLI orchestration without exposing backend implementation details.
#[derive(Debug, Error)]
pub enum CliError {
    #[error("failed to read {path}: {source}")]
    Read {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write {path}: {source}")]
    Write {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid manifest {path}: {source}")]
    Manifest {
        path: std::path::PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("image validation failed: {0}")]
    Image(#[from] minemu_image::ImageError),
    #[error("runtime setup failed")]
    RuntimeSetup,
    #[error("headless assertion failed: {0}")]
    Assertion(String),
}

pub type Result<T> = std::result::Result<T, CliError>;
