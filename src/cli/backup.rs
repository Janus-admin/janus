//! `velox backup` / `velox restore` — single-file, version-stamped archives.
//!
//! V5-2 §5.5. The archive bundles:
//!
//! ```text
//! velox-backup.tar.gz
//! ├── VERSION                 # JSON manifest (schema + velox version)
//! ├── db.sql                  # pg_dump --no-owner --no-acl output
//! ├── velox.toml              # optional — copied if --config-file is supplied
//! └── models/                 # optional — embedding model + tokenizer
//!     ├── all-MiniLM-L6-v2.onnx
//!     └── tokenizer.json
//! ```
//!
//! The version stamp is checked on restore so a backup taken on schema 28 will
//! refuse to restore against a binary that only knows schema 27.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};
use tar::{Archive, Builder, Header};

use super::CliResult;

// ── Public types ──────────────────────────────────────────────────────────────

/// Schema version of the most recent migration shipped with this binary.
///
/// Bumped manually when a new `migrations/NNNN_*.sql` lands. Used by `restore`
/// to refuse archives produced by a *newer* schema than what the running
/// binary understands. Restoring an *older* archive into a newer binary is
/// allowed — `sqlx::migrate!` will replay the gap on next boot.
pub const CURRENT_SCHEMA_VERSION: u32 = 27;

pub const VELOX_VERSION: &str = env!("CARGO_PKG_VERSION");

/// JSON contents of the `VERSION` file inside an archive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VersionStamp {
    pub schema_version: u32,
    pub velox_version: String,
    /// RFC 3339 timestamp written by `write_archive`.
    pub created_at: String,
}

impl VersionStamp {
    pub fn current() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            velox_version: VELOX_VERSION.to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// In-memory representation of a backup archive.
#[derive(Debug, Clone, PartialEq)]
pub struct ArchiveContents {
    pub version: VersionStamp,
    pub db_sql: Vec<u8>,
    /// Optional `velox.toml` bytes.
    pub velox_toml: Option<Vec<u8>>,
    /// Files under `models/` — keyed by relative path.
    pub models: BTreeMap<String, Vec<u8>>,
}

// ── CLI subcommand ────────────────────────────────────────────────────────────

#[derive(Subcommand, Debug)]
pub enum BackupCmd {
    /// Produce a versioned tarball at `path`.
    Create {
        /// Output `.tar.gz` path.
        path: PathBuf,
        /// Optional `velox.toml` to embed.
        #[arg(long)]
        config_file: Option<PathBuf>,
        /// Optional `models/` directory to embed (defaults to `models/`).
        #[arg(long, default_value = "models")]
        models_dir: PathBuf,
        /// Skip embedding the models directory (e.g. when an external store
        /// already has the embeddings).
        #[arg(long)]
        no_models: bool,
        /// Skip the DB dump — useful for snapshotting config + models alone.
        #[arg(long)]
        no_db: bool,
    },
    /// Restore an archive produced by `velox backup create`.
    Restore {
        /// Archive `.tar.gz` path.
        path: PathBuf,
        /// Drop the existing public schema before restoring.
        #[arg(long)]
        clean: bool,
        /// Write `velox.toml` to this location (default: `./velox.toml`).
        #[arg(long)]
        config_out: Option<PathBuf>,
        /// Restore models to this directory (default: `models/`).
        #[arg(long, default_value = "models")]
        models_dir: PathBuf,
    },
    /// Print the archive's VERSION manifest and file listing without writing anything.
    Inspect { path: PathBuf },
}

pub async fn run(cmd: BackupCmd) -> CliResult {
    match cmd {
        BackupCmd::Create {
            path,
            config_file,
            models_dir,
            no_models,
            no_db,
        } => create_cmd(path, config_file, models_dir, no_models, no_db).await,
        BackupCmd::Restore {
            path,
            clean,
            config_out,
            models_dir,
        } => restore_cmd(path, clean, config_out, models_dir).await,
        BackupCmd::Inspect { path } => inspect_cmd(path),
    }
}

// ── High-level commands ───────────────────────────────────────────────────────

async fn create_cmd(
    out_path: PathBuf,
    config_file: Option<PathBuf>,
    models_dir: PathBuf,
    no_models: bool,
    no_db: bool,
) -> CliResult {
    let config = crate::config::Config::load().context("loading velox config")?;
    let db_sql = if no_db {
        Vec::new()
    } else {
        pg_dump(&config.database_url)?
    };

    let velox_toml = match config_file {
        Some(p) => {
            Some(std::fs::read(&p).with_context(|| format!("read config file {}", p.display()))?)
        }
        None => None,
    };

    let models = if no_models {
        BTreeMap::new()
    } else {
        read_dir_recursive(&models_dir).unwrap_or_default()
    };

    let archive = ArchiveContents {
        version: VersionStamp::current(),
        db_sql,
        velox_toml,
        models,
    };

    write_archive(&out_path, &archive)?;
    println!(
        "wrote backup: {} ({} bytes db, {} model file(s)){}",
        out_path.display(),
        archive.db_sql.len(),
        archive.models.len(),
        if archive.velox_toml.is_some() {
            ", + velox.toml"
        } else {
            ""
        }
    );
    Ok(())
}

async fn restore_cmd(
    in_path: PathBuf,
    clean: bool,
    config_out: Option<PathBuf>,
    models_dir: PathBuf,
) -> CliResult {
    let config = crate::config::Config::load().context("loading velox config")?;
    let archive = read_archive(&in_path)?;
    check_version_compatible(&archive.version)?;

    if !archive.db_sql.is_empty() {
        psql_restore(&config.database_url, &archive.db_sql, clean)?;
    }

    if let Some(toml) = &archive.velox_toml {
        let dest = config_out.unwrap_or_else(|| PathBuf::from("velox.toml"));
        std::fs::write(&dest, toml)
            .with_context(|| format!("write velox.toml to {}", dest.display()))?;
        println!("restored velox.toml → {}", dest.display());
    }

    if !archive.models.is_empty() {
        std::fs::create_dir_all(&models_dir)
            .with_context(|| format!("creating models dir {}", models_dir.display()))?;
        for (rel_path, bytes) in &archive.models {
            let dest = models_dir.join(rel_path);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&dest, bytes).with_context(|| format!("write {}", dest.display()))?;
        }
        println!(
            "restored {} model file(s) to {}",
            archive.models.len(),
            models_dir.display()
        );
    }

    Ok(())
}

fn inspect_cmd(in_path: PathBuf) -> CliResult {
    let archive = read_archive(&in_path)?;
    println!("VERSION:");
    println!("  schema_version: {}", archive.version.schema_version);
    println!("  velox_version:  {}", archive.version.velox_version);
    println!("  created_at:     {}", archive.version.created_at);
    println!("db.sql: {} bytes", archive.db_sql.len());
    println!(
        "velox.toml: {}",
        archive
            .velox_toml
            .as_ref()
            .map(|v| format!("{} bytes", v.len()))
            .unwrap_or_else(|| "(absent)".into())
    );
    println!("models/ ({} file(s)):", archive.models.len());
    for (name, bytes) in &archive.models {
        println!("  {name} — {} bytes", bytes.len());
    }
    Ok(())
}

// ── Archive read/write (pure functions — used by tests) ──────────────────────

pub fn write_archive(out_path: &Path, archive: &ArchiveContents) -> Result<()> {
    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).ok();
        }
    }
    let file = File::create(out_path).with_context(|| format!("create {}", out_path.display()))?;
    let gz = GzEncoder::new(file, Compression::default());
    let mut tar = Builder::new(gz);

    write_entry(
        &mut tar,
        "VERSION",
        serde_json::to_vec_pretty(&archive.version)
            .context("encode VERSION manifest")?
            .as_slice(),
    )?;

    write_entry(&mut tar, "db.sql", &archive.db_sql)?;

    if let Some(toml) = &archive.velox_toml {
        write_entry(&mut tar, "velox.toml", toml)?;
    }

    for (rel, bytes) in &archive.models {
        write_entry(&mut tar, &format!("models/{rel}"), bytes)?;
    }

    tar.finish().context("finish tar")?;
    Ok(())
}

pub fn read_archive(in_path: &Path) -> Result<ArchiveContents> {
    let file = File::open(in_path).with_context(|| format!("open {}", in_path.display()))?;
    let gz = GzDecoder::new(file);
    let mut tar = Archive::new(gz);

    let mut version: Option<VersionStamp> = None;
    let mut db_sql: Vec<u8> = Vec::new();
    let mut velox_toml: Option<Vec<u8>> = None;
    let mut models: BTreeMap<String, Vec<u8>> = BTreeMap::new();

    for entry in tar.entries().context("iterate tar entries")? {
        let mut entry = entry.context("read tar entry")?;
        let path = entry
            .path()
            .context("read tar entry path")?
            .to_string_lossy()
            .into_owned();
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).context("read tar entry data")?;
        match path.as_str() {
            "VERSION" => {
                version = Some(serde_json::from_slice(&buf).context("decode VERSION manifest")?);
            }
            "db.sql" => db_sql = buf,
            "velox.toml" => velox_toml = Some(buf),
            other if other.starts_with("models/") => {
                let rel = other.trim_start_matches("models/").to_string();
                if !rel.is_empty() && !rel.ends_with('/') {
                    models.insert(rel, buf);
                }
            }
            // Ignore unknown entries so older archives produced by future
            // backup code that adds new files keep restoring on this binary.
            _ => {}
        }
    }

    let version = version.ok_or_else(|| anyhow::anyhow!("archive missing VERSION manifest"))?;
    Ok(ArchiveContents {
        version,
        db_sql,
        velox_toml,
        models,
    })
}

/// Reject archives whose schema is *newer* than `CURRENT_SCHEMA_VERSION`.
/// Older archives are accepted — the running binary's migration runner will
/// catch the snapshot up on next boot.
pub fn check_version_compatible(version: &VersionStamp) -> Result<()> {
    if version.schema_version > CURRENT_SCHEMA_VERSION {
        bail!(
            "incompatible archive: schema_version {} is newer than this binary's {}",
            version.schema_version,
            CURRENT_SCHEMA_VERSION,
        );
    }
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn write_entry<W: Write>(builder: &mut Builder<W>, path: &str, bytes: &[u8]) -> Result<()> {
    let mut header = Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_mtime(0);
    header.set_cksum();
    builder
        .append_data(&mut header, path, bytes)
        .with_context(|| format!("append {path}"))?;
    Ok(())
}

fn read_dir_recursive(dir: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut out = BTreeMap::new();
    if !dir.exists() {
        return Ok(out);
    }
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) -> Result<()> {
        for entry in std::fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
            let entry = entry?;
            let ft = entry.file_type()?;
            let path = entry.path();
            if ft.is_dir() {
                walk(root, &path, out)?;
            } else if ft.is_file() {
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                let bytes =
                    std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
                out.insert(rel, bytes);
            }
        }
        Ok(())
    }
    walk(dir, dir, &mut out)?;
    Ok(out)
}

/// Best-effort `pg_dump`. Requires the `pg_dump` binary on PATH. Fails loudly
/// otherwise (the operator must install it; this is documented in
/// docs/deployment/ha.md).
fn pg_dump(database_url: &str) -> Result<Vec<u8>> {
    let output = Command::new("pg_dump")
        .arg("--no-owner")
        .arg("--no-acl")
        .arg(database_url)
        .output()
        .context("invoke pg_dump (is it on PATH?)")?;
    if !output.status.success() {
        bail!(
            "pg_dump failed: exit {} — stderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output.stdout)
}

/// Restore via `psql`. Requires the `psql` binary on PATH.
fn psql_restore(database_url: &str, sql: &[u8], clean: bool) -> Result<()> {
    if clean {
        let drop = Command::new("psql")
            .arg(database_url)
            .arg("-c")
            .arg("DROP SCHEMA IF EXISTS public CASCADE; CREATE SCHEMA public;")
            .output()
            .context("invoke psql to drop schema")?;
        if !drop.status.success() {
            bail!(
                "psql clean failed: {}",
                String::from_utf8_lossy(&drop.stderr)
            );
        }
    }

    let mut child = Command::new("psql")
        .arg(database_url)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .context("invoke psql to restore")?;
    child
        .stdin
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("psql stdin pipe missing"))?
        .write_all(sql)
        .context("write SQL to psql stdin")?;
    let status = child.wait().context("wait for psql")?;
    if !status.success() {
        bail!("psql restore exited {status}");
    }
    Ok(())
}
