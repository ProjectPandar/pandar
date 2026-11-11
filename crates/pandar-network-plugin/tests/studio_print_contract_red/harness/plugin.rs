use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

pub(crate) const PLUGIN_OVERRIDE_ENV: &str = "PANDAR_STUDIO_PRINT_CONTRACT_PLUGIN";

pub(super) struct PluginIdentity {
    pub(super) path: PathBuf,
    pub(super) sha256: String,
}

pub(super) struct SelectedLibrary {
    pub(super) identity: PluginIdentity,
    pub(super) source: &'static str,
}

fn plugin_identity(path: &Path) -> Result<PluginIdentity, String> {
    let canonical = fs::canonicalize(path)
        .map_err(|error| format!("resolve plugin {}: {error}", path.display()))?;
    let metadata = fs::metadata(&canonical)
        .map_err(|error| format!("inspect plugin {}: {error}", canonical.display()))?;
    if !metadata.is_file() {
        return Err(format!(
            "plugin is not a regular file: {}",
            canonical.display()
        ));
    }
    let contents = fs::read(&canonical)
        .map_err(|error| format!("read plugin {}: {error}", canonical.display()))?;
    Ok(PluginIdentity {
        path: canonical,
        sha256: Sha256::digest(contents)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    })
}

pub(super) fn select_library(
    configured: Option<&OsStr>,
    build_debug: impl FnOnce() -> Result<PathBuf, String>,
) -> Result<SelectedLibrary, String> {
    let (path, source) = match configured {
        Some(configured) => (PathBuf::from(configured), "override"),
        None => (build_debug()?, "debug-build"),
    };
    Ok(SelectedLibrary {
        identity: plugin_identity(&path)?,
        source,
    })
}
