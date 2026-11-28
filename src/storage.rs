//! Transactional local data and one-time imports from the JSON stores.

use std::{path::Path, time::Duration};

use anyhow::{Context, Result};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use serde::de::DeserializeOwned;

pub const FILE: &str = "listening.sqlite3";

pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let parent = path.parent().ok_or_else(|| std::io::Error::other("missing parent directory"))?;
    std::fs::create_dir_all(parent)?;
    let mut pending = tempfile::NamedTempFile::new_in(parent)?;
    pending.write_all(bytes)?;
    pending.as_file().sync_all()?;
    pending.persist(path).map_err(|error| error.error)?;
    Ok(())
}

pub fn transaction<T>(dir: &Path, f: impl FnOnce(&Transaction<'_>) -> Result<T>) -> Result<T> {
    transaction_waiting(dir, Duration::from_millis(750), f)
}

/// As [`transaction`], waiting up to `patience` for another writer (another
/// copy of Abyss that is closing, for example) instead of 750 ms.
pub fn transaction_waiting<T>(dir: &Path, patience: Duration, f: impl FnOnce(&Transaction<'_>) -> Result<T>) -> Result<T> {
    std::fs::create_dir_all(dir).context(crate::i18n::t("storage.create_data_folder"))?;
    let mut connection = Connection::open(dir.join(FILE)).context(crate::i18n::t("open_listening_database"))?;
    connection.busy_timeout(patience)?;
    connection.pragma_update(None, "synchronous", "FULL")?;
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS imports (name TEXT PRIMARY KEY NOT NULL)")?;
    let value = f(&tx)?;
    tx.commit().context(crate::i18n::t("commit_database_changes"))?;
    Ok(value)
}

/// The import and its marker commit together. Leave the source untouched,
/// including when it cannot be parsed.
pub fn import_json<T: DeserializeOwned>(tx: &Transaction<'_>, dir: &Path, name: &str, import: impl FnOnce(T) -> Result<()>) -> Result<()> {
    if tx.query_row("SELECT EXISTS(SELECT 1 FROM imports WHERE name = ?1)", [name], |row| row.get::<_, bool>(0))? {
        return Ok(());
    }
    match std::fs::read(dir.join(name)) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(value) => import(value)?,
            Err(error) => {
                // Leave the marker unset so a repaired source can still be imported.
                log::debug!("could not read legacy {name}: {error}");
                return Ok(());
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).with_context(|| crate::i18n::text("open_previous", &[name.to_string()])),
    }
    tx.execute("INSERT INTO imports(name) VALUES (?1)", [name])?;
    Ok(())
}
