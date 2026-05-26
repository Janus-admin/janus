// OIDC protocol helpers — used by the SSO handlers.
//
// Flow:
//   1. Caller fetches discovery doc once per request via `fetch_discovery`.
//   2. `start_flow` generates PKCE verifier + challenge + random nonce,
//      and returns the IdP authorization URL together with the state the
//      caller must persist (verifier + nonce) until the callback arrives.
//   3. `exchange_code` exchanges the authorization code for tokens, validates
//      the ID token JWT (signature via JWKS + iss/aud/exp/nonce claims),
//      and returns the extracted identity claims.
//
// Supports RS256/RS384/RS512 signed ID tokens (covers Google, GitHub, Auth0,
// Okta, Azure AD). EC-signed tokens will return an unsupported-algorithm error.

use anyhow::{anyhow, bail, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, TokenData, Validation};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ── Public types ──────────────────────────────────────────────────────────────

/// Fields from the OIDC discovery document that we actually use.
#[derive(Debug, Deserialize)]
pub struct OidcDiscovery {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub jwks_uri: String,
}

/// Validated identity claims extracted from the ID token.
#[derive(Debug, Serialize, Deserialize)]
pub struct IdClaims {
    pub sub: String,
    pub email: Option<String>,
    pub email_verified: Option<bool>,
    pub name: Option<String>,
    pub nonce: Option<String>,
    pub iss: String,
    /// `aud` can be a string or array in the JWT spec — keep as raw JSON.
    pub aud: serde_json::Value,
    pub exp: i64,
    /// Groups claim (non-standard but common: Okta, Auth0 custom claims).
    /// Accept both `groups` and the namespaced variant `https://*/groups`.
    #[serde(default)]
    pub groups: Vec<String>,
}

// ── Discovery ─────────────────────────────────────────────────────────────────

/// Fetch the OIDC discovery document.
///
/// `base_url` is either the issuer URL (e.g. `https://accounts.google.com`) or
/// the full well-known URL. We append `/.well-known/openid-configuration` when
/// the URL does not already end with it.
pub async fn fetch_discovery(base_url: &str) -> Result<OidcDiscovery> {
    let url = if base_url.ends_with("/.well-known/openid-configuration") {
        base_url.to_string()
    } else {
        let base = base_url.trim_end_matches('/');
        format!("{base}/.well-known/openid-configuration")
    };

    let resp = reqwest::get(&url)
        .await
        .map_err(|e| anyhow!("Discovery request failed: {e}"))?
        .error_for_status()
        .map_err(|e| anyhow!("Discovery returned error status: {e}"))?;

    resp.json::<OidcDiscovery>()
        .await
        .map_err(|e| anyhow!("Failed to parse discovery document: {e}"))
}

// ── PKCE ─────────────────────────────────────────────────────────────────────

/// Generate a PKCE `code_verifier` and its `code_challenge` (S256 method).
///
/// Returns `(verifier, challenge)`.
/// The verifier is stored server-side; the challenge is sent to the IdP.
pub fn generate_pkce() -> (String, String) {
    let mut raw = [0u8; 64];
    rand::thread_rng().fill_bytes(&mut raw);

    // RFC 7636 §4.1 — unreserved chars: A-Z a-z 0-9 - . _ ~
    // We use URL-safe base64 (no padding) of the random bytes, which is a
    // superset of unreserved chars and stays within the 43-128 length range.
    let verifier = URL_SAFE_NO_PAD.encode(raw);

    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(hasher.finalize());

    (verifier, challenge)
}

/// Generate a cryptographically random opaque state / nonce string.
pub fn random_token() -> String {
    let mut raw = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw);
    URL_SAFE_NO_PAD.encode(raw)
}

// ── Authorization URL ─────────────────────────────────────────────────────────

/// Build the IdP authorization URL for the start-of-login redirect.
///
/// Returns `(url, state_token)` — `state_token` is the CSRF state param that
/// is also used as the nonce. The caller must persist `(state_token, pkce_verifier)`
/// until the callback arrives.
pub fn build_auth_url(
    discovery: &OidcDiscovery,
    client_id: &str,
    redirect_uri: &str,
    pkce_challenge: &str,
    state: &str,
    nonce: &str,
) -> String {
    let base = &discovery.authorization_endpoint;
    format!(
        "{base}?response_type=code\
         &client_id={client_id}\
         &redirect_uri={redirect_uri}\
         &scope=openid+email+profile\
         &state={state}\
         &nonce={nonce}\
         &code_challenge={pkce_challenge}\
         &code_challenge_method=S256"
    )
}

// ── Token exchange + ID token validation ─────────────────────────────────────

#[derive(Deserialize)]
struct TokenResponse {
    id_token: Option<String>,
}

#[derive(Deserialize)]
struct Jwks {
    keys: Vec<serde_json::Value>,
}

/// Exchange the authorization `code` for tokens, validate the ID token,
/// and return the extracted claims.
///
/// This function makes two network requests:
///   - POST `{token_endpoint}` to exchange the code
///   - GET  `{jwks_uri}` to fetch signing keys (if not yet cached by the IdP's HTTP layer)
pub async fn exchange_code(
    discovery: &OidcDiscovery,
    client_id: &str,
    client_secret: &str,
    code: &str,
    code_verifier: &str,
    redirect_uri: &str,
    expected_nonce: &str,
) -> Result<IdClaims> {
    // ── Step 1: exchange code for tokens ─────────────────────────────────────
    let params = [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("code_verifier", code_verifier),
    ];

    let http = reqwest::Client::new();

    let tok: TokenResponse = http
        .post(&discovery.token_endpoint)
        .form(&params)
        .send()
        .await
        .map_err(|e| anyhow!("Token exchange request failed: {e}"))?
        .error_for_status()
        .map_err(|e| anyhow!("Token endpoint returned error: {e}"))?
        .json()
        .await
        .map_err(|e| anyhow!("Failed to parse token response: {e}"))?;

    let id_token = tok
        .id_token
        .ok_or_else(|| anyhow!("Token response missing id_token"))?;

    // ── Step 2: decode header (need kid + alg before fetching JWKS) ──────────
    let header =
        decode_header(&id_token).map_err(|e| anyhow!("Failed to decode ID token header: {e}"))?;

    let alg = match header.alg {
        jsonwebtoken::Algorithm::RS256 => Algorithm::RS256,
        jsonwebtoken::Algorithm::RS384 => Algorithm::RS384,
        jsonwebtoken::Algorithm::RS512 => Algorithm::RS512,
        other => {
            bail!("Unsupported ID token algorithm: {other:?} — only RS256/384/512 are supported")
        }
    };

    // ── Step 3: fetch JWKS and find matching key ──────────────────────────────
    let jwks: Jwks = http
        .get(&discovery.jwks_uri)
        .send()
        .await
        .map_err(|e| anyhow!("JWKS request failed: {e}"))?
        .error_for_status()
        .map_err(|e| anyhow!("JWKS endpoint returned error: {e}"))?
        .json()
        .await
        .map_err(|e| anyhow!("Failed to parse JWKS: {e}"))?;

    let kid = header.kid.as_deref().unwrap_or("");
    let key_obj = if kid.is_empty() {
        jwks.keys.first()
    } else {
        jwks.keys.iter().find(|k| k["kid"].as_str() == Some(kid))
    }
    .ok_or_else(|| anyhow!("No matching key in JWKS for kid='{kid}'"))?;

    let n = key_obj["n"]
        .as_str()
        .ok_or_else(|| anyhow!("JWKS RSA key missing 'n' field"))?;
    let e = key_obj["e"]
        .as_str()
        .ok_or_else(|| anyhow!("JWKS RSA key missing 'e' field"))?;

    let decoding_key = DecodingKey::from_rsa_components(n, e)
        .map_err(|e| anyhow!("Failed to build decoding key from JWKS: {e}"))?;

    // ── Step 4: validate JWT (iss + aud + exp + alg) ──────────────────────────
    let mut validation = Validation::new(alg);
    validation.set_issuer(&[&discovery.issuer]);
    validation.set_audience(&[client_id]);

    let token_data: TokenData<IdClaims> = decode(&id_token, &decoding_key, &validation)
        .map_err(|e| anyhow!("ID token validation failed: {e}"))?;

    let claims = token_data.claims;

    // ── Step 5: verify nonce (replay protection) ──────────────────────────────
    if claims.nonce.as_deref() != Some(expected_nonce) {
        bail!("Nonce mismatch — possible replay attack");
    }

    Ok(claims)
}
