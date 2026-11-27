/// A non-success Moonraker HTTP response, kept typed so link failures can
/// distinguish rejected API keys from other errors.
#[derive(Debug)]
pub struct MoonrakerHttpError {
    pub(super) what: String,
    pub status: reqwest::StatusCode,
    pub(super) body: String,
}

impl std::fmt::Display for MoonrakerHttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Moonraker {} failed with HTTP {}: {}",
            self.what, self.status, self.body
        )
    }
}

impl std::error::Error for MoonrakerHttpError {}
