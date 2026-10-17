//! Accounts: who may sign in, and what their role lets them do.

use serde::{Deserialize, Serialize};

/// Admins manage projects, labels, images, exports and users; annotators annotate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Admin,
    Annotator,
}

impl Role {
    pub const ALL: [Role; 2] = [Role::Annotator, Role::Admin];

    /// The name people see.
    pub fn name(self) -> &'static str {
        match self {
            Role::Admin => "Admin",
            Role::Annotator => "Annotator",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Annotator => "annotator",
        }
    }

    pub fn parse(text: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|role| role.as_str() == text)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub role: Role,
}

/// Who this browser is signed in as, and whether the server still needs its first account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub setup: bool,
    pub user: Option<User>,
}

/// A username and password, to log in or to create the first account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewUser {
    pub username: String,
    pub password: String,
    pub role: Role,
}

/// An admin's change to an account: a new role, a new password, or both.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserChange {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

/// Passwords shorter than this are refused.
pub const MIN_PASSWORD_LENGTH: usize = 8;
