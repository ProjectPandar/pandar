use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use super::{
    lock::{config_parent_dir, ensure_dir_secure, reject_symlink, verify_open_file_security},
    secret::SecretString,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CliAuthKind {
    ApiKey,
    BrowserSession,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CliProfile {
    pub hub_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web_url: Option<String>,
    pub tenant_id: String,
    pub tenant_slug: String,
    pub tenant_display_name: String,
    pub token: SecretString,
    pub auth_kind: CliAuthKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_name: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CliConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_profile: Option<String>,
    #[serde(default)]
    pub profiles: BTreeMap<String, CliProfile>,
}

pub(crate) fn resolve_config_path(override_path: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = override_path {
        return Ok(path.to_path_buf());
    }
    if let Ok(env_path) = std::env::var("PANDAR_CLI_CONFIG")
        && !env_path.trim().is_empty()
    {
        return Ok(PathBuf::from(env_path));
    }
    default_config_path()
}

fn default_config_path() -> Result<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let base = std::env::var_os("APPDATA")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .context("APPDATA is not set; pass --config or set PANDAR_CLI_CONFIG")?;
        return Ok(base.join("pandar").join("cli.json"));
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
            return Ok(PathBuf::from(xdg).join("pandar").join("cli.json"));
        }
        let home = std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .context("HOME is not set; pass --config or set PANDAR_CLI_CONFIG")?;
        Ok(home.join(".config").join("pandar").join("cli.json"))
    }
}

pub(crate) fn load_config(path: &Path) -> Result<CliConfig> {
    reject_symlink(config_parent_dir(path))?;
    reject_symlink(path)?;

    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        if let Some(flag) = super::lock::unix_o_nofollow() {
            options.custom_flags(flag);
        }
    }
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(CliConfig::default()),
        Err(err) => {
            return Err(anyhow::Error::new(err)
                .context(format!("failed to open CLI config at {}", path.display())));
        }
    };
    verify_open_file_security(&file, path)?;

    let mut raw = String::new();
    file.read_to_string(&mut raw)
        .with_context(|| format!("failed to read CLI config at {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse CLI config at {}", path.display()))
}

pub(crate) fn save_config(path: &Path, config: &CliConfig) -> Result<()> {
    let parent = config_parent_dir(path);
    ensure_dir_secure(parent)?;
    reject_symlink(path)?;

    let serialized =
        serde_json::to_string_pretty(config).context("failed to serialize CLI config")?;
    let temp_path = parent.join(format!(".cli.json.tmp.{}", uuid::Uuid::new_v4().simple()));
    write_restricted_temp_file(&temp_path, serialized.as_bytes())?;
    fs::rename(&temp_path, path).with_context(|| {
        let _ = fs::remove_file(&temp_path);
        format!(
            "failed to replace CLI config {} from {}",
            path.display(),
            temp_path.display()
        )
    })?;
    Ok(())
}

fn write_restricted_temp_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
        if let Some(flag) = super::lock::unix_o_nofollow() {
            options.custom_flags(flag);
        }
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("failed to create temp CLI config {}", path.display()))?;
    verify_open_file_security(&file, path)?;
    file.write_all(bytes)
        .with_context(|| format!("failed to write temp CLI config {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("failed to sync temp CLI config {}", path.display()))?;
    Ok(())
}

pub(crate) fn validate_profile_name(name: &str) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || !trimmed
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        bail!("profile name must contain only ASCII letters, digits, '-', '_', or '.'");
    }
    Ok(trimmed.to_owned())
}

pub(crate) fn normalize_base_url(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    let parsed =
        reqwest::Url::parse(trimmed).with_context(|| format!("invalid server URL `{trimmed}`"))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || trimmed.contains('?')
        || trimmed.contains('#')
        || parsed.port() == Some(0)
    {
        bail!(
            "server URL must be an http(s) URL with a valid host and no credentials, query, or fragment: `{trimmed}`"
        );
    }
    Ok(parsed.as_str().trim_end_matches('/').to_owned())
}
