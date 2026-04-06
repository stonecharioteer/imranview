use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};

use crate::image_io::collect_images_in_directory;

const PROMOTION_REPEAT_WINDOW_SECS: i64 = 14 * 24 * 60 * 60;
const PERSISTED_EXPIRE_SECS: i64 = 30 * 24 * 60 * 60;
const PROBATION_EXPIRE_SECS: i64 = 14 * 24 * 60 * 60;
const MAX_PERSISTED_FOLDERS: i64 = 128;
const DB_FILE_NAME: &str = "catalog-v1.sqlite";

#[derive(Clone, Copy, Debug, Default)]
pub struct CatalogCacheStats {
    pub database_bytes: u64,
    pub tracked_folders: usize,
    pub persisted_folders: usize,
    pub persisted_entries: usize,
}

#[derive(Clone, Debug)]
struct FolderUsage {
    first_seen_ts: i64,
    open_count: i64,
    persisted: bool,
    last_indexed_ts: Option<i64>,
    dir_mtime_ns: Option<i64>,
}

pub fn list_images_in_directory(directory: &Path) -> Result<Vec<PathBuf>> {
    if !directory.is_dir() {
        anyhow::bail!("{} is not a directory", directory.display());
    }
    match list_images_in_directory_inner(directory) {
        Ok(files) => Ok(files),
        Err(err) => {
            log::warn!(
                target: "imranview::catalog",
                "catalog lookup failed for {}: {err:#}; falling back to direct scan",
                directory.display()
            );
            collect_images_in_directory(directory).map_err(|scan_err| anyhow::anyhow!(scan_err))
        }
    }
}

pub fn cache_stats() -> Result<CatalogCacheStats> {
    let path = catalog_db_path();
    let bytes = storage_size_bytes(&path)?;
    if !path.exists() {
        return Ok(CatalogCacheStats {
            database_bytes: bytes,
            ..CatalogCacheStats::default()
        });
    }

    let conn = open_connection()?;
    init_schema(&conn)?;

    let tracked_folders = conn
        .query_row("SELECT COUNT(1) FROM folder_usage", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap_or(0)
        .max(0) as usize;
    let persisted_folders = conn
        .query_row(
            "SELECT COUNT(1) FROM folder_usage WHERE persisted = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        .max(0) as usize;
    let persisted_entries = conn
        .query_row("SELECT COUNT(1) FROM folder_entries", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap_or(0)
        .max(0) as usize;

    Ok(CatalogCacheStats {
        database_bytes: bytes,
        tracked_folders,
        persisted_folders,
        persisted_entries,
    })
}

pub fn purge_cache() -> Result<u64> {
    let path = catalog_db_path();
    let mut freed = 0u64;
    for artifact in db_artifact_paths(&path) {
        if let Ok(metadata) = fs::metadata(&artifact) {
            freed = freed.saturating_add(metadata.len());
        }
        if artifact.exists() {
            fs::remove_file(&artifact)
                .with_context(|| format!("failed to remove {}", artifact.display()))?;
        }
    }
    Ok(freed)
}

fn list_images_in_directory_inner(directory: &Path) -> Result<Vec<PathBuf>> {
    let now = unix_now_secs();
    let dir_key = directory.to_string_lossy().to_string();
    let current_dir_mtime = directory_mtime_ns(directory);

    let mut conn = open_connection()?;
    init_schema(&conn)?;
    let tx = conn.transaction()?;

    prune_expired_entries(&tx, now)?;

    let existing = fetch_usage(&tx, &dir_key)?;
    let first_seen_ts = existing
        .as_ref()
        .map(|value| value.first_seen_ts)
        .unwrap_or(now);
    let open_count = existing
        .as_ref()
        .map(|value| value.open_count + 1)
        .unwrap_or(1);

    let mut persisted = existing
        .as_ref()
        .map(|value| value.persisted)
        .unwrap_or(false);
    if !persisted
        && open_count >= 2
        && now.saturating_sub(first_seen_ts) <= PROMOTION_REPEAT_WINDOW_SECS
    {
        persisted = true;
        log::debug!(
            target: "imranview::catalog",
            "promoted folder to persistent catalog {}",
            directory.display()
        );
    }

    let previous_last_indexed = existing.as_ref().and_then(|value| value.last_indexed_ts);
    let previous_mtime = existing.as_ref().and_then(|value| value.dir_mtime_ns);

    tx.execute(
        "
        INSERT INTO folder_usage (
            directory,
            first_seen_ts,
            last_opened_ts,
            open_count,
            persisted,
            last_indexed_ts,
            dir_mtime_ns
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        ON CONFLICT(directory) DO UPDATE SET
            first_seen_ts = excluded.first_seen_ts,
            last_opened_ts = excluded.last_opened_ts,
            open_count = excluded.open_count,
            persisted = excluded.persisted,
            last_indexed_ts = excluded.last_indexed_ts,
            dir_mtime_ns = excluded.dir_mtime_ns
        ",
        params![
            dir_key,
            first_seen_ts,
            now,
            open_count,
            if persisted { 1i64 } else { 0i64 },
            previous_last_indexed,
            previous_mtime,
        ],
    )?;

    if !persisted {
        tx.commit()?;
        return collect_images_in_directory(directory)
            .map_err(|scan_err| anyhow::anyhow!(scan_err));
    }

    let should_reuse_index = existing
        .as_ref()
        .map(|entry| {
            entry.persisted
                && entry.dir_mtime_ns == current_dir_mtime
                && persisted_entry_count(&tx, &dir_key).unwrap_or(0) > 0
        })
        .unwrap_or(false);

    let files = if should_reuse_index {
        load_persisted_entries(&tx, &dir_key)?
    } else {
        let scanned =
            collect_images_in_directory(directory).map_err(|scan_err| anyhow::anyhow!(scan_err))?;
        rewrite_entries(&tx, &dir_key, &scanned)?;
        tx.execute(
            "
            UPDATE folder_usage
            SET last_indexed_ts = ?2, dir_mtime_ns = ?3
            WHERE directory = ?1
            ",
            params![dir_key, now, current_dir_mtime],
        )?;
        scanned
    };

    enforce_persisted_folder_limit(&tx)?;
    tx.commit()?;
    Ok(files)
}

fn rewrite_entries(
    tx: &rusqlite::Transaction<'_>,
    directory: &str,
    files: &[PathBuf],
) -> Result<()> {
    tx.execute(
        "DELETE FROM folder_entries WHERE directory = ?1",
        params![directory],
    )?;
    let mut statement = tx.prepare(
        "
        INSERT INTO folder_entries (directory, file_name_lower, file_path)
        VALUES (?1, ?2, ?3)
        ",
    )?;
    for file in files {
        let lowered = file
            .file_name()
            .map(|value| value.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        statement.execute(params![directory, lowered, file.to_string_lossy()])?;
    }
    Ok(())
}

fn load_persisted_entries(tx: &rusqlite::Transaction<'_>, directory: &str) -> Result<Vec<PathBuf>> {
    let mut statement = tx.prepare(
        "
        SELECT file_path
        FROM folder_entries
        WHERE directory = ?1
        ORDER BY file_name_lower ASC, file_path ASC
        ",
    )?;
    let iter = statement.query_map(params![directory], |row| row.get::<_, String>(0))?;
    let mut files = Vec::new();
    for row in iter {
        files.push(PathBuf::from(row?));
    }
    Ok(files)
}

fn persisted_entry_count(tx: &rusqlite::Transaction<'_>, directory: &str) -> Result<i64> {
    tx.query_row(
        "SELECT COUNT(1) FROM folder_entries WHERE directory = ?1",
        params![directory],
        |row| row.get::<_, i64>(0),
    )
    .map_err(Into::into)
}

fn enforce_persisted_folder_limit(tx: &rusqlite::Transaction<'_>) -> Result<()> {
    let persisted_count: i64 = tx
        .query_row(
            "SELECT COUNT(1) FROM folder_usage WHERE persisted = 1",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let overflow = persisted_count.saturating_sub(MAX_PERSISTED_FOLDERS);
    if overflow <= 0 {
        return Ok(());
    }

    let mut statement = tx.prepare(
        "
        SELECT directory
        FROM folder_usage
        WHERE persisted = 1
        ORDER BY last_opened_ts ASC
        LIMIT ?1
        ",
    )?;
    let iter = statement.query_map(params![overflow], |row| row.get::<_, String>(0))?;
    let mut directories = Vec::new();
    for row in iter {
        directories.push(row?);
    }

    for directory in directories {
        tx.execute(
            "DELETE FROM folder_entries WHERE directory = ?1",
            params![directory],
        )?;
        tx.execute(
            "
            UPDATE folder_usage
            SET persisted = 0, last_indexed_ts = NULL, dir_mtime_ns = NULL
            WHERE directory = ?1
            ",
            params![directory],
        )?;
    }

    Ok(())
}

fn fetch_usage(tx: &rusqlite::Transaction<'_>, directory: &str) -> Result<Option<FolderUsage>> {
    let row = tx
        .query_row(
            "
            SELECT
                first_seen_ts,
                last_opened_ts,
                open_count,
                persisted,
                last_indexed_ts,
                dir_mtime_ns
            FROM folder_usage
            WHERE directory = ?1
            ",
            params![directory],
            |row| {
                Ok(FolderUsage {
                    first_seen_ts: row.get(0)?,
                    open_count: row.get(2)?,
                    persisted: row.get::<_, i64>(3)? == 1,
                    last_indexed_ts: row.get(4)?,
                    dir_mtime_ns: row.get(5)?,
                })
            },
        )
        .optional()?;
    Ok(row)
}

fn prune_expired_entries(tx: &rusqlite::Transaction<'_>, now: i64) -> Result<()> {
    let persisted_cutoff = now.saturating_sub(PERSISTED_EXPIRE_SECS);
    let probation_cutoff = now.saturating_sub(PROBATION_EXPIRE_SECS);

    tx.execute(
        "
        DELETE FROM folder_entries
        WHERE directory IN (
            SELECT directory
            FROM folder_usage
            WHERE persisted = 1 AND last_opened_ts < ?1
        )
        ",
        params![persisted_cutoff],
    )?;
    tx.execute(
        "
        UPDATE folder_usage
        SET persisted = 0, last_indexed_ts = NULL, dir_mtime_ns = NULL
        WHERE persisted = 1 AND last_opened_ts < ?1
        ",
        params![persisted_cutoff],
    )?;
    tx.execute(
        "
        DELETE FROM folder_usage
        WHERE persisted = 0 AND last_opened_ts < ?1
        ",
        params![probation_cutoff],
    )?;
    Ok(())
}

fn open_connection() -> Result<Connection> {
    let path = catalog_db_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create catalog directory {}", parent.display()))?;
    }
    let conn =
        Connection::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
    conn.busy_timeout(Duration::from_millis(500))?;
    let _ = conn.pragma_update(None, "journal_mode", "WAL");
    let _ = conn.pragma_update(None, "synchronous", "NORMAL");
    Ok(conn)
}

fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS folder_usage (
            directory TEXT PRIMARY KEY,
            first_seen_ts INTEGER NOT NULL,
            last_opened_ts INTEGER NOT NULL,
            open_count INTEGER NOT NULL,
            persisted INTEGER NOT NULL DEFAULT 0,
            last_indexed_ts INTEGER,
            dir_mtime_ns INTEGER
        );

        CREATE TABLE IF NOT EXISTS folder_entries (
            directory TEXT NOT NULL,
            file_name_lower TEXT NOT NULL,
            file_path TEXT NOT NULL,
            PRIMARY KEY (directory, file_path)
        );

        CREATE INDEX IF NOT EXISTS idx_folder_usage_last_opened
            ON folder_usage(last_opened_ts);
        CREATE INDEX IF NOT EXISTS idx_folder_entries_dir_name
            ON folder_entries(directory, file_name_lower, file_path);
        ",
    )?;
    Ok(())
}

fn directory_mtime_ns(path: &Path) -> Option<i64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    let duration = modified.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(duration.as_nanos()).ok()
}

fn unix_now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs() as i64)
        .unwrap_or(0)
}

fn catalog_db_path() -> PathBuf {
    if let Some(explicit) = env::var_os("IMRANVIEW_CATALOG_DB") {
        return PathBuf::from(explicit);
    }
    base_config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("imranview")
        .join(DB_FILE_NAME)
}

fn db_artifact_paths(db_path: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::with_capacity(3);
    paths.push(db_path.to_path_buf());
    paths.push(PathBuf::from(format!("{}-wal", db_path.display())));
    paths.push(PathBuf::from(format!("{}-shm", db_path.display())));
    paths
}

fn storage_size_bytes(db_path: &Path) -> Result<u64> {
    let mut total = 0u64;
    for artifact in db_artifact_paths(db_path) {
        if artifact.exists() {
            total = total.saturating_add(
                fs::metadata(&artifact)
                    .with_context(|| format!("failed to read {}", artifact.display()))?
                    .len(),
            );
        }
    }
    Ok(total)
}

fn base_config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        env::var_os("APPDATA").map(PathBuf::from)
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(xdg) = env::var_os("XDG_CONFIG_HOME") {
            return Some(PathBuf::from(xdg));
        }
        env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.join(".config"))
    }
}
