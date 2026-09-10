//! Command-line orchestration for image construction and headless execution.

mod manifest;
mod runner;
mod tui;

pub use manifest::{ImageManifest, ModuleManifest, package_manifest};
pub use runner::{
    BlockMediaAssertion, HeadlessAssertion, HeadlessInput, HeadlessTest, RamPrefill, RunOptions,
    RunResult, run_headless, run_image,
};
pub use tui::run_tui;

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
    #[error("terminal operation failed: {0}")]
    Terminal(#[from] std::io::Error),
    #[error("failed to open tracing output {path}: {source}")]
    LogFile {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to initialize tracing: {0}")]
    Tracing(String),
    #[error("invalid manifest {path}: {source}")]
    Manifest {
        path: std::path::PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("invalid Boot ROM size for {path}: expected {expected} bytes, found {actual}")]
    InvalidBootRomSize {
        path: std::path::PathBuf,
        expected: usize,
        actual: usize,
    },
    #[error("image validation failed: {0}")]
    Image(#[from] minemu_image::ImageError),
    #[error("runtime setup failed")]
    RuntimeSetup,
    #[error("runtime setup failed: {0}")]
    Runtime(String),
    #[error("headless assertion failed: {0}")]
    Assertion(String),
}

pub type Result<T> = std::result::Result<T, CliError>;
