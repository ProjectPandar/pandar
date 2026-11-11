use super::{BambuPrinterEndpoint, diagnostics, moonraker::MoonrakerCameraSource};

/// Where a printer camera stream is produced from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CameraSource {
    Bambu(BambuPrinterEndpoint),
    Moonraker(MoonrakerCameraSource),
}

impl CameraSource {
    pub fn redact(&self, message: &str) -> String {
        match self {
            Self::Bambu(endpoint) => {
                diagnostics::redact_access_code(message, &endpoint.access_code)
            }
            Self::Moonraker(source) => match source.client.api_key() {
                Some(key) => message.replace(key, "[REDACTED_API_KEY]"),
                None => message.to_owned(),
            },
        }
    }
}
