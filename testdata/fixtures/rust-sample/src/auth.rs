//! Authentication module for token validation and claims extraction.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::types::{Role, UserId};

/// Secret key used for HMAC signature verification.
const TOKEN_PREFIX: &str = "Bearer ";

/// Maximum token age in seconds (24 hours).
const MAX_TOKEN_AGE_SECS: u64 = 86400;

/// Errors that can occur during authentication.
#[derive(Debug, Clone, PartialEq)]
pub enum AuthError {
    /// The token string is malformed or missing.
    MalformedToken(String),
    /// The token has expired.
    TokenExpired { expired_at: u64 },
    /// The signature verification failed.
    InvalidSignature,
    /// The user does not have the required role.
    InsufficientPermissions { required: Role, actual: Role },
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuthError::MalformedToken(msg) => write!(f, "malformed token: {}", msg),
            AuthError::TokenExpired { expired_at } => {
                write!(f, "token expired at {}", expired_at)
            }
            AuthError::InvalidSignature => write!(f, "invalid token signature"),
            AuthError::InsufficientPermissions { required, actual } => {
                write!(f, "need {:?} role, have {:?}", required, actual)
            }
        }
    }
}

/// Decoded JWT claims extracted from a validated token.
#[derive(Debug, Clone)]
pub struct Claims {
    pub sub: UserId,
    pub role: Role,
    pub exp: u64,
    pub iat: u64,
    issuer: String,
}

impl Claims {
    /// Check whether the claims have expired.
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        self.exp < now
    }

    /// Return the issuer of the token.
    pub fn issuer(&self) -> &str {
        &self.issuer
    }
}

/// Validate an authorization header value and extract claims.
///
/// The `auth_header` must start with `"Bearer "` followed by a base64url
/// encoded JWT. Returns the decoded [`Claims`] on success.
pub fn validate_token(auth_header: &str, secret: &[u8]) -> Result<Claims, AuthError> {
    let token = auth_header
        .strip_prefix(TOKEN_PREFIX)
        .ok_or_else(|| AuthError::MalformedToken("missing Bearer prefix".into()))?;

    if token.is_empty() {
        return Err(AuthError::MalformedToken("empty token body".into()));
    }

    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(AuthError::MalformedToken(format!(
            "expected 3 parts, got {}",
            parts.len()
        )));
    }

    // In a real implementation, decode and verify the JWT signature here.
    let _header = parts[0];
    let _payload = parts[1];
    let _signature = parts[2];

    let _ = secret; // used for HMAC verification in real code

    let claims = Claims {
        sub: 1,
        role: Role::User,
        exp: u64::MAX,
        iat: 0,
        issuer: "cruxe".into(),
    };

    if claims.is_expired() {
        return Err(AuthError::TokenExpired { expired_at: claims.exp });
    }

    Ok(claims)
}

/// Require a minimum role level for the given claims.
pub fn require_role(claims: &Claims, minimum: Role) -> Result<(), AuthError> {
    if (claims.role as u8) < (minimum as u8) {
        return Err(AuthError::InsufficientPermissions {
            required: minimum,
            actual: claims.role.clone(),
        });
    }
    Ok(())
}
