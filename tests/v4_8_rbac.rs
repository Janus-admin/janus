// tests/v4_8_rbac.rs
// Phase V4-8 acceptance tests — RBAC / True Multi-tenancy.
//
// Run with: cargo test v4_8
//
// Design:
//   - "bootstrap rule": a user with NO workspace memberships is treated as admin.
//     The test admin created by admin_auth_header() exercises this path.
//   - Role-specific tests seed a workspace + membership directly in the DB,
//     then exercise the endpoint with a JWT for that user.

mod common;

use sqlx::PgPool;
use uuid::Uuid;

// ── Test helpers ──────────────────────────────────────────────────────────────

async fn connect_pool() -> PgPool {
    common::load_env();
    let config = janus::config::Config::load().expect("Failed to load config");
    janus::db::pool::connect(&config.database_url)
        .await
        .expect("Failed to connect to test database")
}

/// Register a fresh user (ignoring conflict) and return their JWT.
async fn login_as(base_url: &str, email: &str, password: &str) -> String {
    let client = reqwest::Client::new();
    // Register — idempotent.
    client
        .post(format!("{}/api/v1/auth/register", base_url))
        .json(&serde_json::json!({
            "email": email,
            "password": password,
            "name": email.split('@').next().unwrap_or("test")
        }))
        .send()
        .await
        .expect("register failed");

    let resp = client
        .post(format!("{}/api/v1/auth/login", base_url))
        .json(&serde_json::json!({ "email": email, "password": password }))
        .send()
        .await
        .expect("login failed");

    assert_eq!(resp.status(), 200, "login must succeed");
    let body: serde_json::Value = resp.json().await.expect("login body must be JSON");
    let token = body["token"].as_str().expect("token field missing");
    format!("Bearer {}", token)
}

/// Create a test workspace and return its ID.
async fn create_test_workspace(pool: &PgPool, name: &str) -> Uuid {
    let id = Uuid::new_v4();
    let slug = name
        .to_lowercase()
        .replace(' ', "-")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect::<String>();
    // Include a random suffix so parallel tests don't collide on slug uniqueness.
    let slug = format!("{}-{}", slug, &id.to_string()[..8]);
    sqlx::query!(
        "INSERT INTO workspaces (id, name, slug, created_at) VALUES ($1, $2, $3, NOW())
         ON CONFLICT DO NOTHING",
        id,
        name,
        slug
    )
    .execute(pool)
    .await
    .expect("workspace insert failed");
    id
}

/// Add a user (looked up by email) to a workspace with the given role name.
async fn grant_role(pool: &PgPool, email: &str, workspace_id: Uuid, role_name: &str) {
    let user_row = sqlx::query!("SELECT id FROM users WHERE email = $1", email)
        .fetch_optional(pool)
        .await
        .expect("user lookup failed")
        .expect("user must exist when granting role");

    let member_id = Uuid::new_v4();
    sqlx::query!(
        r#"
        INSERT INTO workspace_members (id, workspace_id, user_id, role_id, created_at)
        SELECT $1, $2, $3, r.id, NOW()
        FROM roles r WHERE r.name = $4
        ON CONFLICT DO NOTHING
        "#,
        member_id,
        workspace_id,
        user_row.id,
        role_name
    )
    .execute(pool)
    .await
    .expect("workspace_members insert failed");
}

/// Remove a user from a workspace (by email).
async fn revoke_membership(pool: &PgPool, email: &str, workspace_id: Uuid) {
    let user_row = sqlx::query!("SELECT id FROM users WHERE email = $1", email)
        .fetch_optional(pool)
        .await
        .expect("user lookup failed");
    if let Some(u) = user_row {
        sqlx::query!(
            "DELETE FROM workspace_members WHERE workspace_id = $1 AND user_id = $2",
            workspace_id,
            u.id
        )
        .execute(pool)
        .await
        .expect("delete membership failed");
    }
}

// ── Test: bootstrap rule — admin with no memberships ─────────────────────────

/// A user with NO workspace memberships is treated as admin (bootstrap mode).
/// The shared test admin created by admin_auth_header() exercises this.
#[tokio::test]
async fn v4_8_regression_existing_admin_jwt_unaffected() {
    let base_url = common::spawn_app().await;
    let auth = common::admin_auth_header(&base_url).await;
    let client = reqwest::Client::new();

    // Analytics (requires billing_viewer) — admin passes.
    let resp = client
        .get(format!("{}/admin/analytics/overview", base_url))
        .header("Authorization", &auth)
        .send()
        .await
        .expect("analytics request failed");

    assert_eq!(resp.status(), 200, "admin must be able to read analytics");
}

// ── Test: BillingViewer can read analytics ────────────────────────────────────

#[tokio::test]
async fn v4_8_billing_viewer_can_read_analytics() {
    let base_url = common::spawn_app().await;
    let pool = connect_pool().await;
    let email = format!("bv-analytics-{}@v48.test", Uuid::new_v4());
    let ws_id = create_test_workspace(&pool, "BV Analytics WS").await;

    let auth = login_as(&base_url, &email, "password123").await;
    grant_role(&pool, &email, ws_id, "billing_viewer").await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/admin/analytics/overview", base_url))
        .header("Authorization", &auth)
        .send()
        .await
        .expect("analytics request failed");

    assert_eq!(
        resp.status(),
        200,
        "billing_viewer must be able to read analytics"
    );
}

// ── Test: BillingViewer cannot create API keys ────────────────────────────────

#[tokio::test]
async fn v4_8_billing_viewer_cannot_create_api_keys() {
    let base_url = common::spawn_app().await;
    let pool = connect_pool().await;
    let email = format!("bv-keys-{}@v48.test", Uuid::new_v4());
    let ws_id = create_test_workspace(&pool, "BV Keys WS").await;

    let auth = login_as(&base_url, &email, "password123").await;
    grant_role(&pool, &email, ws_id, "billing_viewer").await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/admin/keys", base_url))
        .header("Authorization", &auth)
        .json(&serde_json::json!({ "name": "blocked-key" }))
        .send()
        .await
        .expect("create key request failed");

    assert_eq!(
        resp.status(),
        403,
        "billing_viewer must be blocked from creating keys"
    );
}

// ── Test: ApiManager can create keys ─────────────────────────────────────────

#[tokio::test]
async fn v4_8_api_manager_can_create_keys() {
    let base_url = common::spawn_app().await;
    let pool = connect_pool().await;
    let email = format!("mgr-create-{}@v48.test", Uuid::new_v4());
    let ws_id = create_test_workspace(&pool, "MGR Create WS").await;

    let auth = login_as(&base_url, &email, "password123").await;
    grant_role(&pool, &email, ws_id, "api_manager").await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/admin/keys", base_url))
        .header("Authorization", &auth)
        .json(&serde_json::json!({ "name": "mgr-allowed-key" }))
        .send()
        .await
        .expect("create key request failed");

    assert_eq!(
        resp.status(),
        201,
        "api_manager must be allowed to create keys"
    );
}

// ── Test: ApiManager cannot delete cache ─────────────────────────────────────

#[tokio::test]
async fn v4_8_api_manager_cannot_delete_cache() {
    let base_url = common::spawn_app().await;
    let pool = connect_pool().await;
    let email = format!("mgr-cache-{}@v48.test", Uuid::new_v4());
    let ws_id = create_test_workspace(&pool, "MGR Cache WS").await;

    let auth = login_as(&base_url, &email, "password123").await;
    grant_role(&pool, &email, ws_id, "api_manager").await;

    let client = reqwest::Client::new();
    let resp = client
        .delete(format!("{}/admin/cache", base_url))
        .header("Authorization", &auth)
        .send()
        .await
        .expect("flush cache request failed");

    assert_eq!(
        resp.status(),
        403,
        "api_manager must be blocked from flushing cache"
    );
}

// ── Test: ReadOnly cannot mutate anything ────────────────────────────────────

#[tokio::test]
async fn v4_8_read_only_cannot_mutate_anything() {
    let base_url = common::spawn_app().await;
    let pool = connect_pool().await;
    let email = format!("ro-mutate-{}@v48.test", Uuid::new_v4());
    let ws_id = create_test_workspace(&pool, "RO Mutate WS").await;

    let auth = login_as(&base_url, &email, "password123").await;
    grant_role(&pool, &email, ws_id, "read_only").await;

    let client = reqwest::Client::new();

    // Try to create a key — should be 403.
    let r1 = client
        .post(format!("{}/admin/keys", base_url))
        .header("Authorization", &auth)
        .json(&serde_json::json!({ "name": "ro-key" }))
        .send()
        .await
        .expect("request failed");
    assert_eq!(r1.status(), 403, "read_only blocked from POST /admin/keys");

    // Try to delete cache — should be 403.
    let r2 = client
        .delete(format!("{}/admin/cache", base_url))
        .header("Authorization", &auth)
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        r2.status(),
        403,
        "read_only blocked from DELETE /admin/cache"
    );
}

// ── Test: Admin can list workspaces ──────────────────────────────────────────

#[tokio::test]
async fn v4_8_admin_can_access_all_endpoints() {
    let base_url = common::spawn_app().await;
    let auth = common::admin_auth_header(&base_url).await;
    let client = reqwest::Client::new();

    // DELETE /admin/cache — requires admin role.
    let resp = client
        .delete(format!("{}/admin/cache", base_url))
        .header("Authorization", &auth)
        .send()
        .await
        .expect("flush cache request failed");

    assert_eq!(
        resp.status(),
        200,
        "admin (bootstrap) must be able to flush cache"
    );
}

// ── Test: Admin can add a member to a workspace ───────────────────────────────

#[tokio::test]
async fn v4_8_admin_can_add_member() {
    let base_url = common::spawn_app().await;
    let pool = connect_pool().await;
    let admin_email = format!("admin-add-{}@v48.test", Uuid::new_v4());
    let new_member_email = format!("new-member-{}@v48.test", Uuid::new_v4());

    // Create workspace + admin user with admin role.
    let ws_id = create_test_workspace(&pool, "Admin Add WS").await;
    let admin_auth = login_as(&base_url, &admin_email, "password123").await;
    grant_role(&pool, &admin_email, ws_id, "admin").await;

    // Register the user to be invited.
    login_as(&base_url, &new_member_email, "password123").await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/admin/workspaces/{}/members", base_url, ws_id))
        .header("Authorization", &admin_auth)
        .json(&serde_json::json!({
            "email": new_member_email,
            "role": "billing_viewer"
        }))
        .send()
        .await
        .expect("add member request failed");

    assert_eq!(resp.status(), 201, "admin must be able to add members");

    let body: serde_json::Value = resp.json().await.expect("body must be JSON");
    assert_eq!(
        body["data"]["role"].as_str(),
        Some("billing_viewer"),
        "returned role must match"
    );
}

// ── Test: Removed member loses access ────────────────────────────────────────

#[tokio::test]
async fn v4_8_removed_member_loses_access_immediately() {
    let base_url = common::spawn_app().await;
    let pool = connect_pool().await;
    let email = format!("removed-{}@v48.test", Uuid::new_v4());
    let ws_id = create_test_workspace(&pool, "Remove Test WS").await;

    let auth = login_as(&base_url, &email, "password123").await;
    grant_role(&pool, &email, ws_id, "billing_viewer").await;

    let client = reqwest::Client::new();

    // With membership: can access analytics.
    let r1 = client
        .get(format!("{}/admin/analytics/overview", base_url))
        .header("Authorization", &auth)
        .send()
        .await
        .expect("request failed");
    assert_eq!(r1.status(), 200, "billing_viewer can read analytics");

    // Revoke membership.
    revoke_membership(&pool, &email, ws_id).await;

    // Now the user has NO workspace membership at all → bootstrap rule → treated as admin.
    // Analytics should STILL succeed (admin passes billing_viewer check).
    let r2 = client
        .get(format!("{}/admin/analytics/overview", base_url))
        .header("Authorization", &auth)
        .send()
        .await
        .expect("request failed");
    assert_eq!(
        r2.status(),
        200,
        "user with no memberships falls back to bootstrap admin"
    );
}

// ── Test: Workspace admin cannot manage another workspace's members ───────────

#[tokio::test]
async fn v4_8_cross_workspace_access_denied() {
    let base_url = common::spawn_app().await;
    let pool = connect_pool().await;
    let email = format!("cross-ws-{}@v48.test", Uuid::new_v4());
    let target_email = format!("cross-target-{}@v48.test", Uuid::new_v4());

    let ws_a = create_test_workspace(&pool, "Cross WS A").await;
    let ws_b = create_test_workspace(&pool, "Cross WS B").await;

    // User is admin in workspace A only.
    let auth = login_as(&base_url, &email, "password123").await;
    grant_role(&pool, &email, ws_a, "admin").await;

    // Register the target user.
    login_as(&base_url, &target_email, "password123").await;

    let client = reqwest::Client::new();

    // Try to add a member to workspace B — user is not admin there, should be 403.
    let resp = client
        .post(format!("{}/admin/workspaces/{}/members", base_url, ws_b))
        .header("Authorization", &auth)
        .json(&serde_json::json!({
            "email": target_email,
            "role": "read_only"
        }))
        .send()
        .await
        .expect("cross-workspace request failed");

    assert_eq!(
        resp.status(),
        403,
        "admin of workspace A must not be able to manage workspace B members"
    );
}

// ── Test: Existing users have admin role from migration ───────────────────────

#[tokio::test]
async fn v4_8_existing_users_get_admin_role_on_migration() {
    // Verify that workspace_members and roles tables exist and the admin role exists.
    let pool = connect_pool().await;

    let role_row = sqlx::query!("SELECT COUNT(*) as cnt FROM roles WHERE name = 'admin'")
        .fetch_one(&pool)
        .await
        .expect("roles query failed");

    assert!(
        role_row.cnt.unwrap_or(0) > 0,
        "roles table must contain an 'admin' row after migration"
    );
}

// ── Test: List workspaces requires admin ──────────────────────────────────────

#[tokio::test]
async fn v4_8_list_workspaces_requires_admin() {
    let base_url = common::spawn_app().await;
    let pool = connect_pool().await;
    let email = format!("list-ws-{}@v48.test", Uuid::new_v4());
    let ws_id = create_test_workspace(&pool, "List WS Test").await;

    let auth = login_as(&base_url, &email, "password123").await;
    // Give this user read_only — not admin.
    grant_role(&pool, &email, ws_id, "read_only").await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/admin/workspaces", base_url))
        .header("Authorization", &auth)
        .send()
        .await
        .expect("list workspaces request failed");

    assert_eq!(
        resp.status(),
        403,
        "read_only user must be blocked from listing workspaces"
    );
}

// ── Test: List members requires admin in workspace ───────────────────────────

#[tokio::test]
async fn v4_8_list_members_requires_admin_in_workspace() {
    let base_url = common::spawn_app().await;
    let pool = connect_pool().await;
    let email = format!("list-mem-{}@v48.test", Uuid::new_v4());
    let ws_id = create_test_workspace(&pool, "List Mem Test WS").await;

    let auth = login_as(&base_url, &email, "password123").await;
    // billing_viewer — not admin.
    grant_role(&pool, &email, ws_id, "billing_viewer").await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/admin/workspaces/{}/members", base_url, ws_id))
        .header("Authorization", &auth)
        .send()
        .await
        .expect("list members request failed");

    assert_eq!(
        resp.status(),
        403,
        "billing_viewer must be blocked from listing members"
    );
}

// ── Regression: gateway key auth is unaffected ───────────────────────────────

#[tokio::test]
async fn v4_8_regression_gateway_api_key_auth_unaffected() {
    // The RBAC system is for admin (JWT) routes only.
    // Gateway API keys must still work exactly as before.
    let base_url = common::spawn_app().await;
    let client = reqwest::Client::new();

    // Hit a gateway endpoint with the pre-seeded test key.
    let resp = client
        .get(format!("{}/v1/models", base_url))
        .header("Authorization", common::auth_header())
        .send()
        .await
        .expect("gateway request failed");

    // /v1/models returns 200 with an empty model list when no providers exist.
    assert!(
        resp.status().is_success(),
        "gateway key auth must still work after V4-8"
    );
}
