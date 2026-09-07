use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use crate::{CliError, Result};

/// Declarative system-image input manifest.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageManifest {
    pub kernel: PathBuf,
    #[serde(default)]
    pub modules: Vec<ModuleManifest>,
}

/// One fixed-address user module included in an image manifest.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleManifest {
    pub name: String,
    pub elf: PathBuf,
}

/// Parses a manifest and packages its referenced independently linked ELFs.
pub fn package_manifest(path: impl AsRef<Path>) -> Result<minemu_image::SystemImage> {
    let path = path.as_ref();
    let manifest = read_manifest(path)?;
    let root = path.parent().unwrap_or_else(|| Path::new("."));
    let kernel_path = resolve(root, &manifest.kernel);
    let kernel = read(&kernel_path)?;
    let modules = manifest
        .modules
        .iter()
        .map(|module| Ok((module.name.as_str(), read(&resolve(root, &module.elf))?)))
        .collect::<Result<Vec<_>>>()?;
    let mut builder = minemu_image::ImageBuilder::new(&kernel);
    for (name, elf) in &modules {
        builder = builder.add_module(minemu_image::ModuleInput { name, elf });
    }
    Ok(builder.build()?)
}

fn read_manifest(path: &Path) -> Result<ImageManifest> {
    let source = fs::read_to_string(path).map_err(|source| CliError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    toml::from_str(&source).map_err(|source| CliError::Manifest {
        path: path.to_path_buf(),
        source,
    })
}

fn resolve(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|source| CliError::Read {
        path: path.into(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::ImageManifest;

    #[test]
    fn manifest_decodes_kernel_and_modules() {
        let manifest: ImageManifest = toml::from_str(
            "kernel = 'kernel.elf'\n[[modules]]\nname = 'shell'\nelf = 'shell.elf'\n",
        )
        .unwrap();
        assert_eq!(manifest.kernel.to_str(), Some("kernel.elf"));
        assert_eq!(manifest.modules[0].name, "shell");
    }

    #[test]
    fn manifest_rejects_unknown_top_level_and_module_fields() {
        assert!(
            toml::from_str::<ImageManifest>("kernel = 'kernel.elf'\nkerne = 'typo'\n").is_err()
        );
        assert!(
            toml::from_str::<ImageManifest>(
                "kernel = 'kernel.elf'\n[[modules]]\nname = 'shell'\nelf = 'shell.elf'\naddress = 1\n"
            )
            .is_err()
        );
    }
}
