use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{fmt, io::Read};
use zeroize::{Zeroize, ZeroizeOnDrop};

pub(crate) const MAX_STDIN_API_KEY_BYTES: usize = 4096;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(transparent)]
pub(crate) struct SecretString(String);

impl SecretString {
    pub(crate) fn new(value: String) -> Self {
        Self(value)
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret([redacted])")
    }
}

/// Reads an API key once from `reader` with strict hygiene:
/// - bounded to `MAX_STDIN_API_KEY_BYTES` (4096 bytes)
/// - strips at most one trailing `\r\n` or `\n`
/// - rejects empty input or any embedded whitespace/newlines
/// - never echoes secret bytes in error messages
pub(crate) fn read_api_key_from_reader<R: Read>(reader: R) -> Result<SecretString> {
    let mut raw = Vec::new();
    reader
        .take((MAX_STDIN_API_KEY_BYTES + 1) as u64)
        .read_to_end(&mut raw)
        .context("failed to read API key from stdin")?;
    if raw.len() > MAX_STDIN_API_KEY_BYTES {
        raw.zeroize();
        bail!("API key from stdin exceeds {MAX_STDIN_API_KEY_BYTES} bytes");
    }
    let mut text = String::from_utf8(raw).map_err(|err| {
        let mut bytes = err.into_bytes();
        bytes.zeroize();
        anyhow::anyhow!("API key from stdin is not valid UTF-8")
    })?;

    if text.ends_with('\n') {
        text.pop();
        if text.ends_with('\r') {
            text.pop();
        }
    }
    if text.is_empty() {
        text.zeroize();
        bail!("API key from stdin is empty");
    }
    if text.chars().any(char::is_whitespace) {
        text.zeroize();
        bail!("API key from stdin must be a single token without whitespace or multiple lines");
    }
    Ok(SecretString::new(text))
}
