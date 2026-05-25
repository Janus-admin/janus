// tests/v5_2_migration_imports.rs
// Phase V5-2 acceptance tests — migration importers + backup/restore + Helm chart.
//
// Run with: cargo test v5_2
//
// Coverage:
//   - LiteLLM YAML → MigrationPlan (provider list, routing-strategy mapping)
//   - Portkey JSON → MigrationPlan (virtual keys, configs, cache mode)
//   - OpenRouter listing → AliasReport (provider grouping, unmapped pass-through)
//   - Backup archive write/read roundtrip, version compatibility check
//   - Helm chart lint + template (skipped gracefully when `helm` is not on PATH)

use std::path::PathBuf;

use janus::cli::backup::{self, ArchiveContents, VersionStamp, CURRENT_SCHEMA_VERSION};
use janus::cli::import::{litellm, openrouter, portkey};

fn fixture(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("fixtures");
    p.push("v5_2");
    p.push(name);
    p
}

fn read_fixture(name: &str) -> String {
    std::fs::read_to_string(fixture(name)).unwrap_or_else(|e| panic!("read fixture {name}: {e}"))
}

// ─── LiteLLM importer ─────────────────────────────────────────────────────────

#[test]
fn v5_2_litellm_yaml_parses_to_provider_list() {
    let cfg = litellm::parse_yaml(&read_fixture("litellm-sample.yaml"))
        .expect("LiteLLM fixture must parse");
    let plan = litellm::plan_from_config(&cfg);

    // Fixture lists 5 models across openai, anthropic, bedrock, groq, vertex_ai(→gemini).
    let provider_ids: Vec<&str> = plan.providers.iter().map(|p| p.id.as_str()).collect();
    for expected in ["openai", "anthropic", "bedrock", "groq", "gemini"] {
        assert!(
            provider_ids.contains(&expected),
            "expected provider {expected} in plan, got {provider_ids:?}"
        );
    }

    // OpenAI patch must carry both the api_key and the api_base from the YAML.
    let openai = plan
        .providers
        .iter()
        .find(|p| p.id == "openai")
        .expect("openai patch present");
    assert_eq!(openai.api_key.as_deref(), Some("sk-openai-fixture"));
    assert_eq!(
        openai.base_url.as_deref(),
        Some("https://api.openai.com/v1")
    );
    assert_eq!(openai.is_enabled, Some(true));

    // Cache config must flow through.
    assert_eq!(plan.config.cache_enabled, Some(true));
    assert!((plan.config.semantic_cache_threshold.unwrap() - 0.85).abs() < 1e-6);

    // Master key seen → one API key spec emitted with the mapped strategy.
    assert_eq!(plan.keys.len(), 1, "expected exactly one key spec");
    assert_eq!(plan.keys[0].routing_strategy, "round_robin"); // simple-shuffle → round_robin
}

#[test]
fn v5_2_litellm_routing_strategy_maps_correctly() {
    use litellm::map_routing_strategy as m;
    assert_eq!(m(Some("simple-shuffle")), "round_robin");
    assert_eq!(m(Some("loadbalance")), "round_robin");
    assert_eq!(m(Some("least-busy")), "latency");
    assert_eq!(m(Some("latency-based-routing")), "latency");
    assert_eq!(m(Some("usage-based-routing")), "cost");
    assert_eq!(m(Some("usage-based-routing-v2")), "cost");
    assert_eq!(m(Some("lowest-cost")), "cost");
    // Unknown strategies fall back to priority (Janus default).
    assert_eq!(m(Some("never-heard-of-it")), "priority");
    assert_eq!(m(None), "priority");
}

#[test]
fn v5_2_litellm_unknown_provider_emits_note_not_crash() {
    let yaml = r#"
model_list:
  - model_name: weird
    litellm_params:
      model: somerandomprovider/foo
      api_key: sk-x
"#;
    let cfg = litellm::parse_yaml(yaml).expect("must parse");
    let plan = litellm::plan_from_config(&cfg);
    assert!(
        plan.providers.is_empty(),
        "unknown providers are not patched"
    );
    assert!(
        plan.notes.iter().any(|n| n.contains("somerandomprovider")),
        "expected a note explaining the skip; got {:?}",
        plan.notes
    );
}

// ─── Portkey importer ─────────────────────────────────────────────────────────

#[test]
fn v5_2_portkey_export_parses_to_provider_list() {
    let export =
        portkey::parse_json(&read_fixture("portkey-sample.json")).expect("portkey fixture parses");
    let plan = portkey::plan_from_export(&export);

    // After dedup the fixture should patch openai, anthropic, groq, deepseek.
    let mut ids: Vec<&str> = plan.providers.iter().map(|p| p.id.as_str()).collect();
    ids.sort();
    assert_eq!(ids, vec!["anthropic", "deepseek", "groq", "openai"]);

    // Virtual keys + configs both produce ApiKeySpec rows → at least 4.
    assert!(
        plan.keys.len() >= 4,
        "expected ≥4 key specs (2 virtual + 2 config); got {}",
        plan.keys.len()
    );

    // The `loadbalance` config must map to round_robin; the `fallback` to priority.
    let fb = plan
        .keys
        .iter()
        .find(|k| k.name == "fallback-config")
        .expect("fallback-config key present");
    assert_eq!(fb.routing_strategy, "priority");
    let lb = plan
        .keys
        .iter()
        .find(|k| k.name == "loadbalance-config")
        .expect("loadbalance-config key present");
    assert_eq!(lb.routing_strategy, "round_robin");

    // Cache config block must flow through (last write wins between configs).
    assert!(plan.config.cache_enabled.unwrap_or(false));
}

// ─── OpenRouter importer ──────────────────────────────────────────────────────

#[test]
fn v5_2_openrouter_import_creates_model_aliases() {
    let listing = openrouter::parse_listing(&read_fixture("openrouter-models.json"))
        .expect("OpenRouter fixture parses");
    let report = openrouter::build_aliases(&listing);

    // The fixture lists 8 models; every one should be returned.
    assert_eq!(report.aliases.len(), 8);

    // OpenAI must have 2 models, anthropic 1, gemini 1 (mapped from `google`),
    // groq 1, deepseek 1, bedrock 1 (mapped from `amazon`). Mistral is unmapped.
    for (provider, expected) in [
        ("openai", 2),
        ("anthropic", 1),
        ("gemini", 1),
        ("groq", 1),
        ("deepseek", 1),
        ("bedrock", 1),
    ] {
        let count = report.provider_counts.get(provider).copied().unwrap_or(0);
        assert_eq!(
            count, expected,
            "expected {expected} models under {provider}, got {count}"
        );
    }
    assert!(
        report.unmapped_providers.contains(&"mistralai".to_string()),
        "unmapped provider list should include mistralai; got {:?}",
        report.unmapped_providers
    );

    // Sanity: pricing is preserved as the original OpenRouter string.
    let gpt4o = report
        .aliases
        .iter()
        .find(|a| a.openrouter_id == "openai/gpt-4o")
        .expect("gpt-4o alias present");
    assert_eq!(gpt4o.janus_provider.as_deref(), Some("openai"));
    assert_eq!(gpt4o.prompt_price.as_deref(), Some("0.000005"));
}

// ─── Backup / restore ─────────────────────────────────────────────────────────

fn sample_archive() -> ArchiveContents {
    let mut models = std::collections::BTreeMap::new();
    models.insert("all-MiniLM-L6-v2.onnx".into(), vec![0u8, 1, 2, 3, 4]);
    models.insert("tokenizer.json".into(), b"{\"version\":\"1\"}".to_vec());

    ArchiveContents {
        version: VersionStamp {
            schema_version: CURRENT_SCHEMA_VERSION,
            janus_version: env!("CARGO_PKG_VERSION").to_string(),
            created_at: "2026-05-25T00:00:00Z".to_string(),
        },
        db_sql: b"CREATE TABLE t (x INT); INSERT INTO t VALUES (1);".to_vec(),
        janus_toml: Some(b"[server]\nport = 8080\n".to_vec()),
        models,
    }
}

#[test]
fn v5_2_backup_produces_complete_archive() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("backup.tar.gz");
    let archive = sample_archive();
    backup::write_archive(&path, &archive).expect("write_archive");

    // The tarball must exist and be non-empty.
    let meta = std::fs::metadata(&path).expect("archive exists");
    assert!(meta.len() > 0, "archive must have nonzero size");

    // Read it back and verify the manifest, db.sql, toml and both model entries.
    let back = backup::read_archive(&path).expect("read_archive");
    assert_eq!(back.version.schema_version, CURRENT_SCHEMA_VERSION);
    assert_eq!(back.db_sql, archive.db_sql);
    assert_eq!(back.janus_toml, archive.janus_toml);
    assert_eq!(back.models.len(), 2);
    assert!(back.models.contains_key("all-MiniLM-L6-v2.onnx"));
    assert!(back.models.contains_key("tokenizer.json"));
}

#[test]
fn v5_2_restore_roundtrip_preserves_all_tables() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("roundtrip.tar.gz");
    let archive = sample_archive();
    backup::write_archive(&path, &archive).expect("write_archive");
    let back = backup::read_archive(&path).expect("read_archive");
    assert_eq!(
        archive, back,
        "archive must be bit-equivalent after roundtrip"
    );
}

#[test]
fn v5_2_restore_rejects_incompatible_version() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("future.tar.gz");

    let mut future = sample_archive();
    future.version.schema_version = CURRENT_SCHEMA_VERSION + 100;
    backup::write_archive(&path, &future).expect("write_archive");

    let back = backup::read_archive(&path).expect("read_archive must still parse");
    let err = backup::check_version_compatible(&back.version)
        .expect_err("restore must reject newer schema");
    let msg = err.to_string();
    assert!(
        msg.contains("schema_version"),
        "error should mention schema_version; got: {msg}"
    );
}

// ─── Helm chart ───────────────────────────────────────────────────────────────

fn helm_on_path() -> bool {
    std::process::Command::new("helm")
        .arg("version")
        .arg("--short")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn chart_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("charts");
    p.push("janus");
    p
}

#[test]
fn v5_2_helm_chart_lint_passes() {
    if !helm_on_path() {
        eprintln!("helm not installed; skipping v5_2_helm_chart_lint_passes");
        return;
    }
    let output = std::process::Command::new("helm")
        .arg("lint")
        .arg(chart_dir())
        .arg("-f")
        .arg(fixture("helm-minimal-values.yaml"))
        .output()
        .expect("invoke helm lint");
    assert!(
        output.status.success(),
        "helm lint failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn v5_2_helm_template_produces_valid_k8s_yaml() {
    if !helm_on_path() {
        eprintln!("helm not installed; skipping v5_2_helm_template_produces_valid_k8s_yaml");
        return;
    }
    let output = std::process::Command::new("helm")
        .arg("template")
        .arg("janus-test")
        .arg(chart_dir())
        .arg("-f")
        .arg(fixture("helm-minimal-values.yaml"))
        .output()
        .expect("invoke helm template");
    assert!(
        output.status.success(),
        "helm template failed:\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let rendered = String::from_utf8(output.stdout).expect("template output is utf-8");
    // Spot-check that the kinds we expect are present.
    for kind in [
        "kind: Deployment",
        "kind: Service",
        "kind: ConfigMap",
        "kind: Secret",
        "kind: PersistentVolumeClaim",
        "kind: HorizontalPodAutoscaler",
        "kind: Ingress",
        "kind: ServiceMonitor",
    ] {
        assert!(
            rendered.contains(kind),
            "rendered template missing `{kind}`; sample:\n{}",
            &rendered.chars().take(400).collect::<String>()
        );
    }

    // Every document is valid YAML — yaml.parse_all returns one doc per --- block.
    let docs: Vec<_> = serde_yaml::Deserializer::from_str(&rendered).collect();
    assert!(!docs.is_empty(), "no YAML docs rendered");
    for (idx, raw) in docs.into_iter().enumerate() {
        let v: serde_yaml::Value = serde::Deserialize::deserialize(raw).unwrap_or_else(|e| {
            panic!("document {idx} did not parse as YAML: {e}");
        });
        // helm emits some leading-comment-only docs which serialize to Null — that's fine.
        let _ = v;
    }
}
