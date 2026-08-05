use rusqlite::Connection as SqlConnection;
use std::path::{Path, PathBuf};

use crate::modules::storage::Db;

/// How many snapshots to keep. Older ones are pruned after each new backup.
const KEEP: usize = 5;

fn backups_dir() -> Result<PathBuf, String> {
    let dir = dirs::data_dir()
        .ok_or_else(|| "no data directory on this platform".to_string())?
        .join("kelyfos")
        .join("backups");
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    Ok(dir)
}

/// `kelyfos-db-20260725T101530Z[-label].sqlite3`. The timestamp is fixed-width
/// and UTC, so sorting these names by string is sorting them by time — which is
/// all retention needs.
fn file_name(label: Option<&str>) -> String {
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    match label.map(slugify).filter(|s| !s.is_empty()) {
        Some(l) => format!("kelyfos-db-{stamp}-{l}.sqlite3"),
        None => format!("kelyfos-db-{stamp}.sqlite3"),
    }
}

fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn is_backup_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("kelyfos-db-").and_then(|r| r.strip_suffix(".sqlite3")) else {
        return false;
    };
    let stamp = rest.split_once('-').map_or(rest, |(s, _)| s);
    stamp.len() == 16 && stamp.starts_with(|c: char| c.is_ascii_digit()) && stamp.ends_with('Z')
}

/// Snapshot paths, newest first.
fn existing(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| is_backup_name(&e.file_name().to_string_lossy()))
        .map(|e| e.path())
        .collect();
    out.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    out
}

/// Snapshot the database with `VACUUM INTO`, which produces one consistent
/// file with the WAL already folded in — a plain copy of `app.db` would miss
/// whatever is still sitting in `app.db-wal`.
///
/// There is no UI for this; it runs before an update so there is something to
/// fall back to. Users back up their own data with the JSON export instead.
#[tauri::command]
pub fn backup_create(db: tauri::State<Db>, label: Option<String>) -> Result<String, String> {
    let conn = db.0.lock().map_err(|_| "database lock poisoned".to_string())?;
    create_from_conn(&conn, label.as_deref())
}

/// For callers holding the connection directly — schema migrations run before
/// there is any managed state to borrow it from.
pub fn create_from_conn(conn: &SqlConnection, label: Option<&str>) -> Result<String, String> {
    let dir = backups_dir()?;
    let target = dir.join(file_name(label));

    // VACUUM INTO refuses to overwrite, so build under a temp name and rename:
    // a kill mid-backup then leaves a `.part` behind rather than a truncated
    // file that looks like a usable snapshot.
    let part = target.with_extension("part");
    let _ = std::fs::remove_file(&part);

    conn.execute("VACUUM INTO ?1", [part.to_string_lossy().as_ref()])
        .map_err(|e| format!("backup failed: {e}"))?;

    std::fs::rename(&part, &target)
        .map_err(|e| format!("cannot finalize {}: {e}", target.display()))?;

    prune(&dir, &target);
    Ok(target.to_string_lossy().to_string())
}

/// Keep the newest KEEP snapshots. `just_written` is never a candidate, so a
/// backup taken with KEEP == 1 still survives its own pruning pass.
fn prune(dir: &Path, just_written: &Path) {
    for old in existing(dir).into_iter().skip(KEEP) {
        if old != just_written {
            let _ = std::fs::remove_file(&old);
        }
    }
    // Sweep aborted attempts from earlier runs.
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            if e.path().extension().is_some_and(|x| x == "part") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_names_are_recognized() {
        for label in [None, Some("before-0.2.0"), Some("My Label!")] {
            let name = file_name(label);
            assert!(is_backup_name(&name), "{name}");
        }
    }

    #[test]
    fn ignores_foreign_files() {
        assert!(!is_backup_name("app.db"));
        assert!(!is_backup_name("kelyfos-db-nope.sqlite3"));
        assert!(!is_backup_name("kelyfos-db-20260725T101530Z.part"));
        assert!(!is_backup_name("notes.sqlite3"));
    }

    /// Retention relies on name order matching time order.
    #[test]
    fn name_sort_is_time_sort() {
        let mut v = [
            "kelyfos-db-20260725T101530Z.sqlite3",
            "kelyfos-db-20250101T000000Z.sqlite3",
            "kelyfos-db-20260725T101531Z-before-0-2-0.sqlite3",
        ];
        v.sort_by(|a, b| b.cmp(a));
        assert_eq!(v[0], "kelyfos-db-20260725T101531Z-before-0-2-0.sqlite3");
        assert_eq!(v[2], "kelyfos-db-20250101T000000Z.sqlite3");
    }
}
