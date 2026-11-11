//! Transactional local data and one-time imports from the JSON stores.

use std::{path::Path, time::Duration};

use anyhow::{Context, Result};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use serde::de::DeserializeOwned;

pub const FILE: &str = "listening.sqlite3";

pub fn transaction<T>(dir: &Path, f: impl FnOnce(&Transaction<'_>) -> Result<T>) -> Result<T> {
    std::fs::create_dir_all(dir).context(crate::i18n::t("storage.create_data_folder"))?;
    let mut connection = Connection::open(dir.join(FILE)).context(crate::i18n::t("open_listening_database"))?;
    connection.busy_timeout(Duration::from_millis(750))?;
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
        Ok(bytes) => import(serde_json::from_slice(&bytes).with_context(|| crate::i18n::text("read_previous", &[name.to_string()]))?)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).with_context(|| crate::i18n::text("open_previous", &[name.to_string()])),
    }
    tx.execute("INSERT INTO imports(name) VALUES (?1)", [name])?;
    Ok(())
}
