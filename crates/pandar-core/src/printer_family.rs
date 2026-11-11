use serde::{Deserialize, Serialize};

/// The machine protocol family an Agent uses to reach a printer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrinterFamily {
    #[default]
    BambuLab,
    Moonraker,
}

impl PrinterFamily {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BambuLab => "bambu_lab",
            Self::Moonraker => "moonraker",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "bambu_lab" => Some(Self::BambuLab),
            "moonraker" => Some(Self::Moonraker),
            _ => None,
        }
    }

    /// The `type` value accepted by the link-printer API and `LinkPrinter.printer_type`.
    pub const fn link_type(self) -> &'static str {
        match self {
            Self::BambuLab => "BambuLab",
            Self::Moonraker => "Moonraker",
        }
    }

    pub fn from_link_type(value: &str) -> Option<Self> {
        match value {
            "BambuLab" => Some(Self::BambuLab),
            "Moonraker" => Some(Self::Moonraker),
            _ => None,
        }
    }
}
