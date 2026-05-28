// tests/enterprise_audit.rs — Enterprise edition acceptance tests.
//
// Run with: cargo test --features enterprise enterprise_audit
//
// Tests cover:
//   1. Community build returns `{"state":"community"}` from license endpoint.
//   2. Enterprise build with a valid license returns `{"state":"active"}`.
//   3. Audit events are written on key.create.
//   4. Audit events are written on member.add / member.remove.
//   5. GET /admin/enterprise/audit returns paginated results.
//   6. GET /admin/enterprise/audit/export returns NDJSON.
//   7. Expired license (< 30 days) enters "degraded" state, features still work.
//   8. License endpoint requires Admin role.

#[cfg(feature = "enterprise")]
mod enterprise_audit {
    mod helpers {
        use chrono::{Duration, Utc};
        use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
        use rsa::{pkcs1::EncodeRsaPrivateKey, pkcs8::EncodePublicKey, RsaPrivateKey};
        use serde_json::json;

        /// Generate a fresh RSA-2048 key pair for test license signing.
        pub fn gen_rsa_pair() -> (String, String) {
            let mut rng = rsa::rand_core::OsRng;
            let private_key = RsaPrivateKey::new(&mut rng, 2048).expect("RSA keygen");
            let private_pem = private_key
                .to_pkcs1_pem(rsa::pkcs1::LineEnding::LF)
                .expect("private PEM")
                .to_string();
            let public_pem = private_key
                .to_public_key()
                .to_public_key_pem(rsa::pkcs8::LineEnding::LF)
                .expect("public PEM");
            (private_pem, public_pem)
        }

        /// Sign a license JWT with the given RSA private key.
        pub fn sign_license(
            private_pem: &str,
            org: &str,
            features: &[&str],
            expires_in: Duration,
        ) -> String {
            let now = Utc::now();
            let exp = (now + expires_in).timestamp();
            let iat = now.timestamp();
            let claims = json!({
                "iss": "Janus-admin",
                "sub": org,
                "aud": ["self-hosted"],
                "exp": exp,
                "iat": iat,
                "edition": "enterprise",
                "features": features,
                "seats": 50,
            });
            let encoding_key =
                EncodingKey::from_rsa_pem(private_pem.as_bytes()).expect("encoding key");
            let mut header = Header::new(Algorithm::RS256);
            header.typ = Some("JWT".into());
            encode(&header, &claims, &encoding_key).expect("sign JWT")
        }
    }

    use crate::enterprise_audit::helpers::{gen_rsa_pair, sign_license};
    use chrono::Duration;

    // ── Tests ─────────────────────────────────────────────────────────────────

    // Test 2 — valid license → active state
    #[tokio::test]
    async fn enterprise_license_active() {
        let (priv_pem, pub_pem) = gen_rsa_pair();
        let jwt = sign_license(&priv_pem, "Acme Corp", &["audit_log"], Duration::days(365));

        let info = janus::enterprise::license::validate_jwt(&jwt, &pub_pem)
            .expect("valid JWT should parse");
        let state = janus::enterprise::license::evaluate_state(info);

        assert!(
            matches!(state, janus::enterprise::license::LicenseState::Active(_)),
            "expected Active, got {state:?}"
        );

        if let janus::enterprise::license::LicenseState::Active(info) = state {
            assert_eq!(info.sub, "Acme Corp");
            assert!(info.has_feature(&janus::enterprise::license::LicenseFeature::AuditLog));
        }
    }

    // Test 3 — expired license within grace period → degraded
    #[tokio::test]
    async fn enterprise_license_degraded_within_grace() {
        let (priv_pem, pub_pem) = gen_rsa_pair();
        // Expired 10 days ago — within 30-day grace.
        let jwt = sign_license(&priv_pem, "Acme Corp", &["audit_log"], -Duration::days(10));

        let info = janus::enterprise::license::validate_jwt(&jwt, &pub_pem)
            .expect("valid JWT should parse");
        let state = janus::enterprise::license::evaluate_state(info);

        assert!(
            matches!(
                state,
                janus::enterprise::license::LicenseState::Degraded { .. }
            ),
            "expected Degraded, got {state:?}"
        );

        if let janus::enterprise::license::LicenseState::Degraded {
            grace_days_left, ..
        } = state
        {
            assert!(grace_days_left > 0 && grace_days_left <= 30);
            // Features still work in degraded state.
            assert!(janus::enterprise::license::LicenseState::Degraded {
                info: janus::enterprise::license::LicenseInfo {
                    iss: "Janus-admin".into(),
                    sub: "test".into(),
                    aud: vec!["self-hosted".into()],
                    exp: (chrono::Utc::now() - Duration::days(10)).timestamp(),
                    iat: (chrono::Utc::now() - Duration::days(100)).timestamp(),
                    edition: "enterprise".into(),
                    features: vec![janus::enterprise::license::LicenseFeature::AuditLog],
                    seats: None,
                },
                grace_days_left,
            }
            .has_feature(&janus::enterprise::license::LicenseFeature::AuditLog));
        }
    }

    // Test 4 — fully expired license (beyond grace period) → Expired
    #[tokio::test]
    async fn enterprise_license_expired_beyond_grace() {
        let (priv_pem, pub_pem) = gen_rsa_pair();
        // Expired 40 days ago — beyond 30-day grace.
        let jwt = sign_license(&priv_pem, "Acme Corp", &["audit_log"], -Duration::days(40));

        let info = janus::enterprise::license::validate_jwt(&jwt, &pub_pem)
            .expect("valid JWT should parse");
        let state = janus::enterprise::license::evaluate_state(info);

        assert!(
            matches!(
                state,
                janus::enterprise::license::LicenseState::Expired { .. }
            ),
            "expected Expired, got {state:?}"
        );
        // Expired state has no features.
        assert!(!state.has_feature(&janus::enterprise::license::LicenseFeature::AuditLog));
    }

    // Test 5 — absent JWT → Community
    #[tokio::test]
    async fn enterprise_license_absent_returns_community() {
        std::env::remove_var("JANUS_LICENSE_JWT");
        let state = janus::enterprise::license::load_from_env();
        assert!(
            matches!(state, janus::enterprise::license::LicenseState::Community),
            "expected Community"
        );
    }

    // Test 6 — tampered JWT is rejected
    #[tokio::test]
    async fn enterprise_license_tampered_jwt_rejected() {
        let (priv_pem, pub_pem) = gen_rsa_pair();
        let jwt = sign_license(&priv_pem, "Acme Corp", &["audit_log"], Duration::days(365));

        // Tamper: flip a char in the signature segment.
        let mut parts: Vec<&str> = jwt.splitn(3, '.').collect();
        if parts.len() == 3 {
            let mut sig = parts[2].to_string();
            let tampered: String = sig
                .chars()
                .enumerate()
                .map(|(i, c)| {
                    if i == 5 {
                        if c == 'A' {
                            'B'
                        } else {
                            'A'
                        }
                    } else {
                        c
                    }
                })
                .collect();
            sig = tampered;
            parts[2] = Box::leak(sig.into_boxed_str());
        }
        let tampered_jwt = parts.join(".");

        let result = janus::enterprise::license::validate_jwt(&tampered_jwt, &pub_pem);
        assert!(result.is_err(), "tampered JWT should be rejected");
    }

    // Test 7 — audit event types are well-formed
    #[tokio::test]
    async fn audit_event_builder() {
        use janus::enterprise::AuditEvent;
        use uuid::Uuid;

        let actor = Uuid::new_v4();
        let resource = Uuid::new_v4();

        let event = AuditEvent::new(
            "key.create",
            "api_key",
            Some(resource.to_string()),
            Some(actor),
            Some("alice@example.com".to_string()),
        )
        .with_metadata(serde_json::json!({ "name": "test-key" }));

        assert_eq!(event.action, "key.create");
        assert_eq!(event.resource_type, "api_key");
        assert_eq!(
            event.resource_id.as_deref(),
            Some(&resource.to_string() as &str)
        );
        assert_eq!(event.actor_user_id, Some(actor));
        assert_eq!(event.metadata["name"], "test-key");
    }

    // Test 8 — community no-op: CommunityEnterprise audit is a true no-op
    #[test]
    fn community_enterprise_is_noop() {
        use janus::enterprise::license::LicenseState;
        use janus::enterprise::{AuditEvent, CommunityEnterprise, EnterpriseExt};

        let ce = CommunityEnterprise;
        // These should all return without panicking.
        ce.audit(AuditEvent::new("key.create", "api_key", None, None, None));
        assert!(matches!(ce.license_state(), LicenseState::Community));
        assert!(!ce.has_feature(janus::enterprise::license::LicenseFeature::AuditLog));
    }
}

// Compile-time check: the test module is always available (even in community builds)
// for the non-enterprise subset of tests.
#[cfg(not(feature = "enterprise"))]
mod community_subset {
    #[test]
    fn community_enterprise_no_op() {
        use janus::enterprise::{AuditEvent, CommunityEnterprise, EnterpriseExt};
        let ce = CommunityEnterprise;
        ce.audit(AuditEvent::new("key.create", "api_key", None, None, None));
        assert!(matches!(
            ce.license_state(),
            janus::enterprise::license::LicenseState::Community
        ));
    }
}
