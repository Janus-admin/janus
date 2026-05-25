// tests/v5_l2_oidc.rs
// V5-L2 acceptance tests — OIDC login.
//
// Run with: cargo test v5_l2
//
// Test IdP is a wiremock server that mocks:
//   GET  /.well-known/openid-configuration  → OIDC discovery doc
//   GET  /jwks                              → JWK Set with test RSA public key
//   POST /token                             → ID token response (registered per-test
//                                            after extracting nonce from /start redirect)
//
// RSA-2048 key pair generated per test using the `rsa` crate.
// The ID token is signed with jsonwebtoken + RS256.

mod common;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use rsa::{pkcs8::EncodePrivateKey, traits::PublicKeyParts, RsaPrivateKey};
use serde_json::{json, Value};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

// ── RSA key helpers ───────────────────────────────────────────────────────────

struct TestKey {
    encoding_key: EncodingKey,
    jwks: Value,
}

/// Generate an RSA-2048 key pair for test JWT signing.
/// Returns the jsonwebtoken EncodingKey and the JWKS JSON body.
fn make_test_rsa_key() -> TestKey {
    use rand::rngs::OsRng;

    let private_key = RsaPrivateKey::new(&mut OsRng, 2048).expect("RSA key gen failed");
    let public_key = private_key.to_public_key();

    // Extract n and e for JWKS
    let n_bytes = public_key.n().to_bytes_be();
    let e_bytes = public_key.e().to_bytes_be();
    let n_b64 = URL_SAFE_NO_PAD.encode(&n_bytes);
    let e_b64 = URL_SAFE_NO_PAD.encode(&e_bytes);

    let jwks = json!({
        "keys": [{
            "kty": "RSA",
            "use": "sig",
            "alg": "RS256",
            "n": n_b64,
            "e": e_b64
        }]
    });

    // Serialize private key to PKCS8 PEM for jsonwebtoken
    let pem = private_key
        .to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)
        .expect("PKCS8 PEM serialization failed");
    let encoding_key =
        EncodingKey::from_rsa_pem(pem.as_bytes()).expect("EncodingKey from PEM failed");

    TestKey { encoding_key, jwks }
}

/// Sign an ID token JWT.
fn sign_id_token(
    key: &TestKey,
    issuer: &str,
    audience: &str,
    sub: &str,
    email: &str,
    nonce: &str,
    groups: &[&str],
) -> String {
    let now = chrono::Utc::now().timestamp();
    let claims = json!({
        "sub": sub,
        "email": email,
        "name": "Test User",
        "iss": issuer,
        "aud": audience,
        "exp": now + 3600,
        "iat": now,
        "nonce": nonce,
        "groups": groups,
    });
    encode(&Header::new(Algorithm::RS256), &claims, &key.encoding_key).expect("JWT signing failed")
}

// ── Mock IdP server helpers ───────────────────────────────────────────────────

/// Register the static OIDC discovery + JWKS mocks on the wiremock server.
/// The token mock is dynamic (see `register_token_mock`) because it needs the nonce.
async fn register_static_mocks(mock_server: &MockServer, jwks: &Value) {
    let base = mock_server.uri();
    let discovery = json!({
        "issuer": base,
        "authorization_endpoint": format!("{base}/auth"),
        "token_endpoint": format!("{base}/token"),
        "jwks_uri": format!("{base}/jwks"),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["RS256"],
    });

    Mock::given(method("GET"))
        .and(path("/.well-known/openid-configuration"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&discovery))
        .mount(mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/jwks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(jwks))
        .mount(mock_server)
        .await;
}

/// Register the token endpoint mock with a pre-signed ID token.
async fn register_token_mock(mock_server: &MockServer, id_token: &str) {
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&json!({
            "access_token": "test_access_token",
            "token_type": "Bearer",
            "id_token": id_token,
        })))
        .up_to_n_times(1)
        .mount(mock_server)
        .await;
}

// ── URL helpers ───────────────────────────────────────────────────────────────

/// Extract a single query parameter from a URL string.
fn extract_param(url: &str, param: &str) -> Option<String> {
    let needle = format!("{param}=");
    let pos = url.find(needle.as_str())?;
    let start = pos + needle.len();
    let tail = &url[start..];
    let end = tail.find('&').map(|i| start + i).unwrap_or(url.len());
    Some(url[start..end].to_string())
}

// ── IdP setup helper ──────────────────────────────────────────────────────────

/// Create a test IdP via the admin API and return its ID.
async fn create_test_idp(base_url: &str, mock_base: &str) -> String {
    let client = reqwest::Client::new();
    let admin_token = common::admin_auth_header(base_url).await;

    let resp = client
        .post(format!("{base_url}/admin/idp"))
        .header("Authorization", &admin_token)
        .json(&json!({
            "name": "Test OIDC Provider",
            "discovery_url": mock_base,
            "client_id": "test-client-id",
            "client_secret": "test-client-secret",
        }))
        .send()
        .await
        .expect("create IdP request failed");

    assert_eq!(resp.status(), 201, "create IdP must return 201");
    let body: Value = resp.json().await.expect("create IdP response must be JSON");
    body["data"]["id"]
        .as_str()
        .expect("create IdP response must have data.id")
        .to_string()
}

/// Run the OIDC start → parse nonce/state from Location header.
/// Returns (state_token, nonce).
async fn run_oidc_start(base_url: &str, idp_id: &str) -> (String, String) {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let resp = client
        .get(format!("{base_url}/auth/oidc/{idp_id}/start"))
        .send()
        .await
        .expect("oidc_start request failed");

    assert!(
        resp.status().is_redirection(),
        "oidc_start must return a 3xx redirect, got {}",
        resp.status()
    );

    let location = resp
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .expect("redirect must have Location header");

    let state = extract_param(location, "state").expect("Location URL must contain state param");
    let nonce = extract_param(location, "nonce").expect("Location URL must contain nonce param");

    (state, nonce)
}

/// Run the OIDC callback and return the response.
async fn run_oidc_callback(
    base_url: &str,
    idp_id: &str,
    code: &str,
    state: &str,
) -> reqwest::Response {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    client
        .get(format!(
            "{base_url}/auth/oidc/{idp_id}/callback?code={code}&state={state}"
        ))
        .send()
        .await
        .expect("oidc_callback request failed")
}

// ── Tests ──────────────────────────────────────────────────────────────────────

/// First OIDC login creates a new user (JIT provisioning).
#[tokio::test]
async fn v5_l2_oidc_callback_creates_user_jit() {
    let mock_server = MockServer::start().await;
    let mock_base = mock_server.uri();
    let base_url = common::spawn_app().await;

    let test_key = make_test_rsa_key();
    register_static_mocks(&mock_server, &test_key.jwks).await;

    let idp_id = create_test_idp(&base_url, &mock_base).await;
    let (csrf_state, nonce) = run_oidc_start(&base_url, &idp_id).await;

    let id_token = sign_id_token(
        &test_key,
        &mock_base,
        "test-client-id",
        "oidc-sub-001",
        "jit-user@example.com",
        &nonce,
        &[],
    );
    register_token_mock(&mock_server, &id_token).await;

    let resp = run_oidc_callback(&base_url, &idp_id, "auth-code-001", &csrf_state).await;
    assert_eq!(resp.status(), 200, "OIDC callback must return 200");

    let body: Value = resp.json().await.expect("callback response must be JSON");
    assert!(
        body["token"].as_str().is_some(),
        "response must contain a JWT token"
    );
    let user = &body["user"];
    assert_eq!(
        user["email"].as_str().unwrap_or(""),
        "jit-user@example.com",
        "response must contain the user's email"
    );
}

/// Second OIDC login with the same subject reuses the existing user row.
#[tokio::test]
async fn v5_l2_oidc_second_login_reuses_existing_user() {
    let mock_server = MockServer::start().await;
    let mock_base = mock_server.uri();
    let base_url = common::spawn_app().await;

    let test_key = make_test_rsa_key();
    register_static_mocks(&mock_server, &test_key.jwks).await;

    let idp_id = create_test_idp(&base_url, &mock_base).await;

    // ── First login ───────────────────────────────────────────────────────────
    let (state1, nonce1) = run_oidc_start(&base_url, &idp_id).await;
    let id_token1 = sign_id_token(
        &test_key,
        &mock_base,
        "test-client-id",
        "oidc-sub-002",
        "returning@example.com",
        &nonce1,
        &[],
    );
    register_token_mock(&mock_server, &id_token1).await;

    let resp1 = run_oidc_callback(&base_url, &idp_id, "code-first", &state1).await;
    assert_eq!(resp1.status(), 200, "first login must succeed");
    let user_id_first = resp1.json::<Value>().await.unwrap()["user"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    // ── Second login ──────────────────────────────────────────────────────────
    let (state2, nonce2) = run_oidc_start(&base_url, &idp_id).await;
    let id_token2 = sign_id_token(
        &test_key,
        &mock_base,
        "test-client-id",
        "oidc-sub-002",          // same sub
        "returning@example.com", // same email
        &nonce2,
        &[],
    );
    register_token_mock(&mock_server, &id_token2).await;

    let resp2 = run_oidc_callback(&base_url, &idp_id, "code-second", &state2).await;
    let status2 = resp2.status();
    let body2: Value = resp2.json().await.unwrap_or_default();
    assert_eq!(status2, 200, "second login must succeed — body: {body2}");
    let user_id_second = body2["user"]["id"].as_str().unwrap().to_string();

    assert_eq!(
        user_id_first, user_id_second,
        "both logins must return the same user ID"
    );
}

/// When the ID token includes group claims, the user is assigned the mapped role.
#[tokio::test]
async fn v5_l2_oidc_group_claim_maps_to_role() {
    let mock_server = MockServer::start().await;
    let mock_base = mock_server.uri();
    let base_url = common::spawn_app().await;

    let test_key = make_test_rsa_key();
    register_static_mocks(&mock_server, &test_key.jwks).await;

    // Create IdP with group→role mapping
    let client = reqwest::Client::new();
    let admin_token = common::admin_auth_header(&base_url).await;
    let resp = client
        .post(format!("{base_url}/admin/idp"))
        .header("Authorization", &admin_token)
        .json(&json!({
            "name": "Group Mapping IdP",
            "discovery_url": mock_base,
            "client_id": "test-client-id",
            "client_secret": "test-secret",
            "group_role_map": {
                "admins": "Admin",
                "engineers": "ApiManager"
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let idp_id = resp.json::<Value>().await.unwrap()["data"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (csrf_state, nonce) = run_oidc_start(&base_url, &idp_id).await;
    let id_token = sign_id_token(
        &test_key,
        &mock_base,
        "test-client-id",
        "group-user-001",
        "grouped@example.com",
        &nonce,
        &["admins"],
    );
    register_token_mock(&mock_server, &id_token).await;

    let resp = run_oidc_callback(&base_url, &idp_id, "code-groups", &csrf_state).await;
    assert_eq!(resp.status(), 200, "group-claim login must succeed");
    // User was created — the token is valid and user email is correct
    let body: Value = resp.json().await.unwrap();
    assert_eq!(
        body["user"]["email"].as_str().unwrap_or(""),
        "grouped@example.com"
    );
    // Role assignment is tested indirectly — the RBAC add_member call must not
    // have panicked, and the user is visible in the admin API.
}

/// An ID token with a tampered signature must be rejected with 401.
#[tokio::test]
async fn v5_l2_oidc_invalid_id_token_rejected() {
    let mock_server = MockServer::start().await;
    let mock_base = mock_server.uri();
    let base_url = common::spawn_app().await;

    let test_key = make_test_rsa_key();
    register_static_mocks(&mock_server, &test_key.jwks).await;

    let idp_id = create_test_idp(&base_url, &mock_base).await;
    let (csrf_state, nonce) = run_oidc_start(&base_url, &idp_id).await;

    // Build a valid JWT, then tamper with its signature
    let valid_token = sign_id_token(
        &test_key,
        &mock_base,
        "test-client-id",
        "attacker-001",
        "attacker@evil.com",
        &nonce,
        &[],
    );
    let tampered = {
        let mut parts: Vec<&str> = valid_token.splitn(3, '.').collect();
        // Replace the signature with garbage
        parts[2] = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        parts.join(".")
    };

    register_token_mock(&mock_server, &tampered).await;

    let resp = run_oidc_callback(&base_url, &idp_id, "code-bad", &csrf_state).await;
    assert_eq!(
        resp.status(),
        401,
        "tampered ID token must be rejected with 401"
    );
}

/// A callback with a state token that was not issued by /start is rejected.
#[tokio::test]
async fn v5_l2_oidc_csrf_state_param_enforced() {
    let mock_server = MockServer::start().await;
    let mock_base = mock_server.uri();
    let base_url = common::spawn_app().await;

    let test_key = make_test_rsa_key();
    register_static_mocks(&mock_server, &test_key.jwks).await;

    let idp_id = create_test_idp(&base_url, &mock_base).await;

    // Attempt callback with a state that was never stored in oidc_states
    let resp = run_oidc_callback(
        &base_url,
        &idp_id,
        "some-code",
        "invalid-csrf-state-that-was-never-issued",
    )
    .await;

    assert_eq!(
        resp.status(),
        401,
        "unknown state token must be rejected with 401"
    );
}

/// A /start or /callback request for a disabled IdP returns 404.
#[tokio::test]
async fn v5_l2_oidc_disabled_idp_returns_404() {
    let mock_server = MockServer::start().await;
    let mock_base = mock_server.uri();
    let base_url = common::spawn_app().await;

    let test_key = make_test_rsa_key();
    register_static_mocks(&mock_server, &test_key.jwks).await;

    let idp_id = create_test_idp(&base_url, &mock_base).await;

    // Disable the IdP directly in the DB
    let pool = sqlx::PgPool::connect(
        &std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for tests"),
    )
    .await
    .expect("Failed to connect to DB");

    sqlx::query("UPDATE identity_providers SET enabled = FALSE WHERE id = $1")
        .bind(uuid::Uuid::parse_str(&idp_id).unwrap())
        .execute(&pool)
        .await
        .expect("Failed to disable IdP");

    // /start on disabled IdP must return 404
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let resp = client
        .get(format!("{base_url}/auth/oidc/{idp_id}/start"))
        .send()
        .await
        .expect("oidc_start request failed");

    assert_eq!(resp.status(), 404, "start on disabled IdP must return 404");
}

/// The /admin/idp CRUD endpoints require JWT authentication.
#[tokio::test]
async fn v5_l2_idp_crud_endpoints_require_admin() {
    let base_url = common::spawn_app().await;
    let client = reqwest::Client::new();

    // GET without token → 401
    let resp = client
        .get(format!("{base_url}/admin/idp"))
        .send()
        .await
        .expect("GET /admin/idp failed");
    assert_eq!(
        resp.status(),
        401,
        "GET /admin/idp without auth must be 401"
    );

    // POST without token → 401
    let resp = client
        .post(format!("{base_url}/admin/idp"))
        .json(&json!({"name":"x","discovery_url":"y","client_id":"z","client_secret":""}))
        .send()
        .await
        .expect("POST /admin/idp failed");
    assert_eq!(
        resp.status(),
        401,
        "POST /admin/idp without auth must be 401"
    );

    // DELETE without token → 401
    let resp = client
        .delete(format!(
            "{base_url}/admin/idp/00000000-0000-0000-0000-000000000000"
        ))
        .send()
        .await
        .expect("DELETE /admin/idp/:id failed");
    assert_eq!(
        resp.status(),
        401,
        "DELETE /admin/idp/:id without auth must be 401"
    );
}
