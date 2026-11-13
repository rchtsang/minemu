//! File access shared by manifest-driven commands.

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::de::DeserializeOwned;

use crate::{CliError, Result};

/// Resolves `path` relative to the directory containing its manifest.
pub(crate) fn resolve(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

/// Returns the directory that relative paths in the manifest at `path` are resolved against.
pub(crate) fn manifest_root(path: &Path) -> &Path {
    path.parent().unwrap_or_else(|| Path::new("."))
}

pub(crate) fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|source| CliError::Read {
        path: path.into(),
        source,
    })
}

/// Reads and strictly decodes a TOML manifest.
pub(crate) fn read_toml<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let source = fs::read_to_string(path).map_err(|source| CliError::Read {
        path: path.into(),
        source,
    })?;
    toml::from_str(&source).map_err(|source| CliError::Manifest {
        path: path.into(),
        source,
    })
}
