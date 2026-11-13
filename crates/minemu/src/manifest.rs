use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::{
    Result,
    paths::{manifest_root, read, read_toml, resolve},
};

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
    let manifest: ImageManifest = read_toml(path)?;
    let root = manifest_root(path);
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
