use pandar_core::TenantId;
use serde::Serialize;

use crate::repositories::{RepositoryError, RepositoryResult, User};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TenantTokenScope {
    All,
    Operator,
    Viewer,
    AgentRegister,
    PluginStudio,
    MobileSession,
    WebSession,
    CliSession,
}

impl TenantTokenScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "*",
            Self::Operator => "operator",
            Self::Viewer => "viewer",
            Self::AgentRegister => "agent:register",
            Self::PluginStudio => "plugin:studio",
            Self::MobileSession => "mobile:session",
            Self::WebSession => "web:session",
            Self::CliSession => "cli:session",
        }
    }

    pub fn parse(value: &str) -> RepositoryResult<Self> {
        match value {
            "*" => Ok(Self::All),
            "operator" => Ok(Self::Operator),
            "viewer" => Ok(Self::Viewer),
            "agent:register" => Ok(Self::AgentRegister),
            "plugin:studio" => Ok(Self::PluginStudio),
            "mobile:session" => Ok(Self::MobileSession),
            "web:session" => Ok(Self::WebSession),
            "cli:session" => Ok(Self::CliSession),
            other => Err(RepositoryError::InvalidTokenScope(other.to_owned())),
        }
    }

    pub fn canonical_rank(self) -> u8 {
        match self {
            Self::All => 0,
            Self::Operator => 1,
            Self::Viewer => 2,
            Self::AgentRegister => 3,
            Self::PluginStudio => 4,
            Self::MobileSession => 5,
            Self::WebSession => 6,
            Self::CliSession => 7,
        }
    }

    /// Scopes issued by a browser login-ticket exchange that act with the issuing user's current role.
    pub fn is_user_session(self) -> bool {
        matches!(
            self,
            Self::MobileSession | Self::WebSession | Self::CliSession
        )
    }
}

pub fn normalize_tenant_token_scopes(mut scopes: Vec<TenantTokenScope>) -> Vec<TenantTokenScope> {
    scopes.sort_by_key(|scope| scope.canonical_rank());
    scopes.dedup();
    scopes
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantToken {
    pub id: String,
    pub tenant_id: TenantId,
    pub name: String,
    pub scopes: Vec<TenantTokenScope>,
    pub created_by_user_id: Option<String>,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub expires_at: Option<String>,
    pub revoked_at: Option<String>,
}

impl TenantToken {
    pub fn has_scope(&self, scope: TenantTokenScope) -> bool {
        self.scopes.contains(&TenantTokenScope::All) || self.scopes.contains(&scope)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantTokenWithPlaintext {
    pub token: TenantToken,
    pub plaintext_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedTenantToken {
    pub token: TenantToken,
    pub session_user: Option<User>,
}
