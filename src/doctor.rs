// src/doctor.rs — V4-0 system readiness checks
//
// Run as `janus --doctor` (CLI output) or via `GET /admin/system/readiness`
// (JSON response). Every check is self-contained so they can be unit-tested
// without a running server.

use crate::{config::Config, db::DbPool};
use serde::Serialize;
use std::time::Duration;

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReadinessCheck {
    pub name: &'static str,
    pub status: CheckStatus,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct DoctorReport {
    pub checks: Vec<ReadinessCheck>,
    pub errors: usize,
    pub warnings: usize,
    /// true if all checks pass (no errors; warnings are allowed).
    pub healthy: bool,
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Run all readiness checks and return a full report.
pub async fn run_checks(pool: &DbPool, config: &Config) -> DoctorReport {
    let mut checks = Vec::new();

    checks.push(check_database(pool).await);
    checks.push(check_jwt_secret(config));
    checks.push(check_encryption_key(config, pool).await);
    checks.push(check_providers_enabled(pool).await);
    checks.push(check_embedding_model(config));
    checks.push(check_disk_space());

    let errors = checks
        .iter()
        .filter(|c| c.status == CheckStatus::Fail)
        .count();
    let warnings = checks
        .iter()
        .filter(|c| c.status == CheckStatus::Warn)
        .count();

    DoctorReport {
        healthy: errors == 0,
        checks,
        errors,
        warnings,
    }
}

/// Print the doctor report to stdout in the `janus --doctor` style.
pub fn print_report(report: &DoctorReport) {
    for check in &report.checks {
        let icon = match check.status {
            CheckStatus::Pass => "[✓]",
            CheckStatus::Warn => "[!]",
            CheckStatus::Fail => "[✗]",
        };
        println!("{} {} — {}", icon, check.name, check.message);
    }
    println!();
    if report.errors == 0 && report.warnings == 0 {
        println!("All checks passed.");
    } else {
        println!(
            "{} error(s), {} warning(s).",
            report.errors, report.warnings
        );
    }
}

// ── Individual checks ─────────────────────────────────────────────────────────

async fn check_database(pool: &DbPool) -> ReadinessCheck {
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        sqlx::query("SELECT 1").execute(pool),
    )
    .await;

    match result {
        Ok(Ok(_)) => ReadinessCheck {
            name: "Database connection",
            status: CheckStatus::Pass,
            message: "Reachable".to_string(),
        },
        Ok(Err(e)) => ReadinessCheck {
            name: "Database connection",
            status: CheckStatus::Fail,
            message: format!("Query failed: {e}"),
        },
        Err(_) => ReadinessCheck {
            name: "Database connection",
            status: CheckStatus::Fail,
            message: "Timed out after 2 seconds".to_string(),
        },
    }
}

fn check_jwt_secret(config: &Config) -> ReadinessCheck {
    let len = config.jwt_secret.len();
    if len >= 32 {
        ReadinessCheck {
            name: "JWT secret strength",
            status: CheckStatus::Pass,
            message: format!("{len} bytes — OK"),
        }
    } else {
        ReadinessCheck {
            name: "JWT secret strength",
            status: CheckStatus::Fail,
            message: format!("{len} bytes — minimum 32 required"),
        }
    }
}

async fn check_encryption_key(config: &Config, pool: &DbPool) -> ReadinessCheck {
    // Only required if any provider has an encrypted key stored.
    let has_encrypted: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM providers WHERE api_key_encrypted IS NOT NULL)",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(false);

    if !has_encrypted {
        return ReadinessCheck {
            name: "Encryption key",
            status: CheckStatus::Pass,
            message: "Not required (no encrypted provider keys stored)".to_string(),
        };
    }

    if config.encryption_key.is_empty() {
        ReadinessCheck {
            name: "Encryption key",
            status: CheckStatus::Fail,
            message: "ENCRYPTION_KEY not set but provider keys are stored encrypted".to_string(),
        }
    } else {
        ReadinessCheck {
            name: "Encryption key",
            status: CheckStatus::Pass,
            message: "Set".to_string(),
        }
    }
}

async fn check_providers_enabled(pool: &DbPool) -> ReadinessCheck {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM providers WHERE is_enabled = true")
        .fetch_one(pool)
        .await
        .unwrap_or(0);

    if count >= 1 {
        ReadinessCheck {
            name: "Providers enabled",
            status: CheckStatus::Pass,
            message: format!("{count} provider(s) enabled"),
        }
    } else {
        ReadinessCheck {
            name: "Providers enabled",
            status: CheckStatus::Fail,
            message: "No providers enabled — gateway will return 503 for all requests".to_string(),
        }
    }
}

fn check_embedding_model(config: &Config) -> ReadinessCheck {
    let model_exists = std::path::Path::new(&config.embedding_model_path).exists();
    let tokenizer_exists = std::path::Path::new(&config.embedding_tokenizer_path).exists();

    if model_exists && tokenizer_exists {
        ReadinessCheck {
            name: "Embedding model",
            status: CheckStatus::Pass,
            message: format!("Found at {}", config.embedding_model_path),
        }
    } else {
        ReadinessCheck {
            name: "Embedding model",
            status: CheckStatus::Warn,
            message: format!(
                "Not found at {} — semantic cache disabled",
                config.embedding_model_path
            ),
        }
    }
}

fn check_disk_space() -> ReadinessCheck {
    // Use `df` on Unix to determine available disk space.
    #[cfg(unix)]
    {
        if let Ok(output) = std::process::Command::new("df").arg("-k").arg(".").output() {
            if let Ok(stdout) = std::str::from_utf8(&output.stdout) {
                // df -k output: Filesystem 1K-blocks Used Available ...
                // Skip header line, parse second line.
                if let Some(line) = stdout.lines().nth(1) {
                    let cols: Vec<&str> = line.split_whitespace().collect();
                    // Available is the 4th column (index 3).
                    if let Some(avail_kb) = cols.get(3).and_then(|s| s.parse::<u64>().ok()) {
                        let avail_mb = avail_kb / 1024;
                        if avail_mb >= 100 {
                            return ReadinessCheck {
                                name: "Disk space",
                                status: CheckStatus::Pass,
                                message: format!("{avail_mb} MB available"),
                            };
                        } else {
                            return ReadinessCheck {
                                name: "Disk space",
                                status: CheckStatus::Warn,
                                message: format!(
                                    "{avail_mb} MB available — less than 100 MB recommended"
                                ),
                            };
                        }
                    }
                }
            }
        }
    }

    // Non-Unix or df parse failure: skip check.
    ReadinessCheck {
        name: "Disk space",
        status: CheckStatus::Pass,
        message: "Check skipped (run 'df -h .' to monitor manually)".to_string(),
    }
}
