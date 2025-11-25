//! Role-based access control

use crate::http::errors::HttpError;

/// User roles in the system
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserRole {
    User,
    Manager,
    Admin,
}

impl UserRole {
    /// Parse role from string (note: use std::str::FromStr trait for parsing)
    pub fn parse_role(role: &str) -> Result<Self, HttpError> {
        match role.to_lowercase().as_str() {
            "user" => Ok(UserRole::User),
            "manager" => Ok(UserRole::Manager),
            "admin" => Ok(UserRole::Admin),
            _ => Err(HttpError::BadRequest(format!("Invalid role: {}", role))),
        }
    }

    /// Convert role to string
    pub fn as_str(&self) -> &'static str {
        match self {
            UserRole::User => "user",
            UserRole::Manager => "manager",
            UserRole::Admin => "admin",
        }
    }

    /// Check if role has permission for read operations
    pub fn can_read(&self) -> bool {
        matches!(self, UserRole::User | UserRole::Manager | UserRole::Admin)
    }

    /// Check if role has permission for write operations
    pub fn can_write(&self) -> bool {
        matches!(self, UserRole::Manager | UserRole::Admin)
    }

    /// Check if role has admin permissions
    pub fn is_admin(&self) -> bool {
        matches!(self, UserRole::Admin)
    }

    /// Check if this role includes another role (hierarchy)
    pub fn includes(&self, other: &UserRole) -> bool {
        match self {
            UserRole::Admin => true, // Admin includes all roles
            UserRole::Manager => matches!(other, UserRole::User | UserRole::Manager),
            UserRole::User => matches!(other, UserRole::User),
        }
    }
}

impl std::fmt::Display for UserRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Permission check result
#[derive(Debug)]
pub struct PermissionCheck {
    pub allowed: bool,
    pub required_role: UserRole,
    pub user_role: Option<UserRole>,
}

impl PermissionCheck {
    pub fn allowed() -> Self {
        Self {
            allowed: true,
            required_role: UserRole::User,
            user_role: None,
        }
    }

    pub fn denied(required: UserRole, user: Option<UserRole>) -> Self {
        Self {
            allowed: false,
            required_role: required,
            user_role: user,
        }
    }

    pub fn check_role(user_role: Option<&UserRole>, required_role: UserRole) -> Self {
        match user_role {
            Some(role) if role.includes(&required_role) => Self::allowed(),
            Some(role) => Self::denied(required_role, Some(role.clone())),
            None => Self::denied(required_role, None),
        }
    }
}

/// Permission requirements for endpoints
#[derive(Debug, Clone)]
pub enum Permission {
    Read,   // Any authenticated user
    Write,  // Manager or Admin
    Admin,  // Admin only
    Public, // No authentication required
}

impl Permission {
    /// Check if a role satisfies this permission
    pub fn check(&self, role: Option<&UserRole>) -> PermissionCheck {
        match self {
            Permission::Public => PermissionCheck::allowed(),
            Permission::Read => PermissionCheck::check_role(role, UserRole::User),
            Permission::Write => PermissionCheck::check_role(role, UserRole::Manager),
            Permission::Admin => PermissionCheck::check_role(role, UserRole::Admin),
        }
    }
}

/// Endpoint permission mapping
pub struct EndpointPermissions;

impl EndpointPermissions {
    // Skills endpoints
    pub const SKILLS_LIST: Permission = Permission::Read;
    pub const SKILLS_GET: Permission = Permission::Read;
    pub const SKILLS_CREATE: Permission = Permission::Write;
    pub const SKILLS_UPDATE: Permission = Permission::Write;
    pub const SKILLS_DELETE: Permission = Permission::Write;

    // Search endpoints
    pub const SEARCH: Permission = Permission::Read;

    // Reindex endpoints
    pub const REINDEX: Permission = Permission::Write;
    pub const REINDEX_SKILL: Permission = Permission::Write;

    // Auth endpoints
    pub const AUTH_TOKEN: Permission = Permission::Public; // For local dev
    pub const AUTH_VERIFY: Permission = Permission::Read;

    // Status endpoints
    pub const STATUS: Permission = Permission::Read;
    pub const ROOT: Permission = Permission::Public;

    // Registry endpoints
    pub const REGISTRY_PUBLISH: Permission = Permission::Write;
    pub const REGISTRY_PUBLISH_STATUS: Permission = Permission::Read;
    pub const REGISTRY_YANK: Permission = Permission::Write;
}
