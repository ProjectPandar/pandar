use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrintTransferPhase {
    Connect,
    Login,
    Protection,
    DataConnection,
    Write,
    Finalize,
    Verify,
    Timeout,
}

impl fmt::Display for PrintTransferPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Connect => "FTPS connect phase",
            Self::Login => "FTPS login phase",
            Self::Protection => "FTPS protection phase",
            Self::DataConnection => "FTPS data connection phase",
            Self::Write => "FTPS write phase",
            Self::Finalize => "FTPS finalize phase",
            Self::Verify => "FTPS verify phase",
            Self::Timeout => "FTPS timeout phase",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrintTransferFailure {
    pub phase: PrintTransferPhase,
    pub cause: String,
}
