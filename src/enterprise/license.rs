// src/enterprise/license.rs — Offline JWT license validation.
//
// License flow:
//   1. Customer receives a signed JWT from Janus-admin (RS256, private key stays
//      with us; public key is embedded in the binary or overridden via env).
//   2. Binary validates the JWT locally — no network call required.
//   3. A background task re-validates every 24 h to detect key rotation or revocation
//      (customers on air-gapped installs simply never get this background update).
//   4. Grace period: a license that expired < GRACE_DAYS ago enters `Degraded`
//      rather than `Expired`, so the gateway keeps running during billing delays.
//
// Env vars consumed here:
//   JANUS_LICENSE_JWT — the signed license token issued to the customer

use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Days past expiry during which the system enters `Degraded` rather than `Expired`.
const GRACE_DAYS: i64 = 30;

// ── Feature flags carried in the license JWT ──────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LicenseFeature {
    AuditLog,
    Saml,
    Scim,
    PolicyEngine,
    FinOps,
    PrioritySupport,
}

// ── License payload (JWT claims) ──────────────────────────────────────────────

/// Decoded claims from a Janus enterprise license JWT.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseInfo {
    /// Issuer — must be "Janus-admin".
    pub iss: String,
    /// Subject — customer organization name.
    pub sub: String,
    /// Audience — must contain "self-hosted".
    pub aud: Vec<String>,
    /// Expiry (Unix timestamp).
    pub exp: i64,
    /// Issued-at (Unix timestamp).
    pub iat: i64,
    /// Human-readable edition label, e.g. "growth" or "enterprise".
    #[serde(default = "default_edition")]
    pub edition: String,
    /// Explicit list of enabled enterprise features.
    #[serde(default)]
    pub features: Vec<LicenseFeature>,
    /// Maximum licensed seats (None = unlimited).
    pub seats: Option<u32>,
}

fn default_edition() -> String {
    "enterprise".into()
}

impl LicenseInfo {
    pub fn expires_at(&self) -> DateTime<Utc> {
        DateTime::from_timestamp(self.exp, 0).unwrap_or(DateTime::<Utc>::MIN_UTC)
    }

    pub fn has_feature(&self, f: &LicenseFeature) -> bool {
        self.features.contains(f)
    }
}

// ── License runtime state ─────────────────────────────────────────────────────

/// What the running binary currently knows about its license.
/// Held in an `ArcSwap` so the background refresh task can update it atomically
/// without blocking any in-flight request.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum LicenseState {
    /// No license key present — community edition behaviour.
    Community,
    /// License is valid and not expired.
    Active(LicenseInfo),
    /// License expired but is within the `GRACE_DAYS` grace period.
    /// Enterprise features continue to work; the dashboard shows a warning.
    Degraded {
        info: LicenseInfo,
        grace_days_left: i64,
    },
    /// License expired beyond the grace period — enterprise features are disabled.
    Expired { expired_at: DateTime<Utc> },
    /// `JANUS_LICENSE_JWT` was set but failed validation (wrong key, tampered
    /// token, wrong issuer, etc.). Enterprise features are disabled. The reason
    /// is surfaced via `GET /admin/enterprise/license` so operators can diagnose
    /// without digging through logs.
    Invalid { reason: String },
}

impl LicenseState {
    pub fn is_active(&self) -> bool {
        matches!(self, LicenseState::Active(_) | LicenseState::Degraded { .. })
    }

    pub fn has_feature(&self, f: &LicenseFeature) -> bool {
        match self {
            LicenseState::Active(info) | LicenseState::Degraded { info, .. } => {
                info.has_feature(f)
            }
            _ => false,
        }
    }

    pub fn edition_label(&self) -> &str {
        match self {
            LicenseState::Community => "community",
            LicenseState::Active(i) | LicenseState::Degraded { info: i, .. } => &i.edition,
            LicenseState::Expired { .. } => "expired",
            LicenseState::Invalid { .. } => "invalid",
        }
    }
}

// ── Validation errors ─────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum LicenseError {
    #[error("no license JWT configured (set JANUS_LICENSE_JWT)")]
    NotConfigured,
    #[error("invalid public key PEM")]
    InvalidKey,
    #[error("JWT validation failed: {0}")]
    JwtError(#[from] jsonwebtoken::errors::Error),
    #[error("wrong issuer — expected Janus-admin")]
    WrongIssuer,
}

// ── Public-key material ───────────────────────────────────────────────────────

/// RSA-2048 public key used to validate all Janus enterprise license JWTs.
/// The matching private key is held offline by Janus-admin and never distributed.
/// Customers cannot self-sign a valid license — only tokens signed with the
/// corresponding private key will pass validation.
pub const JANUS_LICENSE_PUBLIC_KEY: &str = "-----BEGIN PUBLIC KEY-----
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA0y2MhhtqaKcx00o0nF28
ASfKGyefs/tqb+8D96uEy6MEvcgl/tlZqwjx+JWCeDeR64OTCTnV82N9TxFNjejt
E7c03GI2PUSg0vGvaO1LkbtDWN/wshhmlvxVkDrrNvNlGaZ+ubfXrQNcqEb/2boL
m1K40DzXtnpFRdSh0X0uSO+q3gDK4Sy9iZhaWxcIE1evt0xVsh6tP65YphiZLS5J
Max/e85OQHNPnpVB+rNT2IBIuoo0gNn5RvyypI7EcT4KPp5OrI1l/+gflP3XH+Pf
GOE9pHWXiEBQdhYJk87JfmIf8+v2489sBQxujVzH9bLFtFaJ7hEwwO4OyqA7wLq6
1QIDAQAB
-----END PUBLIC KEY-----";

pub fn load_public_key() -> &'static str {
    JANUS_LICENSE_PUBLIC_KEY
}

// ── Core validation logic ─────────────────────────────────────────────────────

/// Validate a license JWT and return the decoded `LicenseInfo`.
///
/// Algorithm: RS256
/// Required claims: `iss = "Janus-admin"`, `aud ∋ "self-hosted"`, valid `exp`
pub fn validate_jwt(token: &str, public_key_pem: &str) -> Result<LicenseInfo, LicenseError> {
    let decoding_key = DecodingKey::from_rsa_pem(public_key_pem.as_bytes())
        .map_err(|_| LicenseError::InvalidKey)?;

    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_issuer(&["Janus-admin"]);
    validation.set_audience(&["self-hosted"]);
    // We handle expiry ourselves to implement the grace period.
    validation.validate_exp = false;

    let data =
        decode::<LicenseInfo>(token, &decoding_key, &validation).map_err(LicenseError::JwtError)?;

    let info = data.claims;
    if info.iss != "Janus-admin" {
        return Err(LicenseError::WrongIssuer);
    }

    Ok(info)
}

/// Evaluate the current license state from an already-decoded `LicenseInfo`.
pub fn evaluate_state(info: LicenseInfo) -> LicenseState {
    let now = Utc::now();
    let expires_at = info.expires_at();

    if expires_at > now {
        LicenseState::Active(info)
    } else {
        let days_since_expiry = (now - expires_at).num_days();
        if days_since_expiry <= GRACE_DAYS {
            let grace_days_left = GRACE_DAYS - days_since_expiry;
            LicenseState::Degraded {
                info,
                grace_days_left,
            }
        } else {
            LicenseState::Expired {
                expired_at: expires_at,
            }
        }
    }
}

/// Read `JANUS_LICENSE_JWT` from the environment, validate it, and return
/// the resulting `LicenseState`. Returns `Community` if the env var is absent.
pub fn load_from_env() -> LicenseState {
    let token = match std::env::var("JANUS_LICENSE_JWT") {
        Ok(t) if !t.trim().is_empty() => t,
        _ => return LicenseState::Community,
    };

    let public_key = load_public_key();

    match validate_jwt(&token, public_key) {
        Ok(info) => {
            let state = evaluate_state(info);
            match &state {
                LicenseState::Active(i) => {
                    tracing::info!(
                        org = %i.sub,
                        edition = %i.edition,
                        expires = %i.expires_at(),
                        "Enterprise license active"
                    );
                }
                LicenseState::Degraded { info: i, grace_days_left } => {
                    tracing::warn!(
                        org = %i.sub,
                        grace_days_left,
                        "Enterprise license expired — grace period active"
                    );
                }
                LicenseState::Expired { expired_at } => {
                    tracing::error!(
                        %expired_at,
                        "Enterprise license expired — enterprise features disabled"
                    );
                }
                LicenseState::Community | LicenseState::Invalid { .. } => unreachable!(),
            }
            state
        }
        Err(LicenseError::NotConfigured) => LicenseState::Community,
        Err(e) => {
            let reason = e.to_string();
            tracing::error!(
                reason = %reason,
                "JANUS_LICENSE_JWT is set but failed validation — \
                 enterprise features are disabled. \
                 Check GET /admin/enterprise/license for details."
            );
            LicenseState::Invalid { reason }
        }
    }
}

// ── Grace-period helpers ──────────────────────────────────────────────────────

/// Return a human-readable grace period duration for use in warning banners.
pub fn grace_duration() -> Duration {
    Duration::days(GRACE_DAYS)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn community_state_has_no_features() {
        let state = LicenseState::Community;
        assert!(!state.is_active());
        assert!(!state.has_feature(&LicenseFeature::AuditLog));
        assert_eq!(state.edition_label(), "community");
    }

    #[test]
    fn load_from_env_returns_community_when_no_jwt() {
        // Remove the env var if set, then validate we get Community.
        std::env::remove_var("JANUS_LICENSE_JWT");
        let state = load_from_env();
        assert!(matches!(state, LicenseState::Community));
    }
}
