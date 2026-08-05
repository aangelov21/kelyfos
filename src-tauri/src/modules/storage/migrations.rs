//! Schema versioning.
//!
//! `PRAGMA user_version` is the single source of truth for what shape the file
//! on disk has. Two paths bring a database up to date and they serve different
//! installs: `SCHEMA` below creates everything from nothing (fresh install),
//! `MIGRATIONS` transforms what is already there (existing install). A new
//! column has to appear in both — see the migration notes in README.md.

use rusqlite::{Connection as SqlConnection, Transaction};

/// The schema as this build expects it. `IF NOT EXISTS` throughout, because it
/// also runs on databases that already hold most of these tables.
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS connections (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  host TEXT NOT NULL,
  port INTEGER NOT NULL DEFAULT 22,
  username TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  icon TEXT NOT NULL DEFAULT 'server',
  color TEXT NOT NULL DEFAULT '#6ea8fe',
  tags TEXT NOT NULL DEFAULT '[]',
  folder TEXT NOT NULL DEFAULT '',
  auth_method TEXT NOT NULL DEFAULT 'password',
  key_path TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS workspaces (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  icon TEXT NOT NULL DEFAULT 'layout',
  tags TEXT NOT NULL DEFAULT '[]',
  layout TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS terminals (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  connection_id TEXT REFERENCES connections(id) ON DELETE SET NULL,
  title TEXT NOT NULL DEFAULT '',
  startup_tasks TEXT NOT NULL DEFAULT '[]',
  sort_order INTEGER NOT NULL DEFAULT 0,
  shell TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS presets (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  connection_id TEXT REFERENCES connections(id) ON DELETE SET NULL,
  startup_tasks TEXT NOT NULL DEFAULT '[]',
  shell TEXT NOT NULL DEFAULT '',
  color TEXT NOT NULL DEFAULT '#a78bfa',
  tags TEXT NOT NULL DEFAULT '[]',
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS variables (
  id TEXT PRIMARY KEY,
  scope TEXT NOT NULL,
  owner_id TEXT,
  key TEXT NOT NULL,
  value TEXT NOT NULL,
  UNIQUE(scope, owner_id, key)
);

CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS app_state (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
"#;

pub struct Migration {
    pub version: u32,
    /// Shown in the error message when the step fails, so make it say what it
    /// was trying to do.
    pub name: &'static str,
    /// A whole function, not a SQL string: a step may need to re-encode a JSON
    /// blob or backfill from another table, which SQL alone cannot express.
    pub run: fn(&Transaction) -> rusqlite::Result<()>,
}

/// Ordered by version, ascending. **A shipped migration is immutable** —
/// editing one leaves every user who already ran it on the old behaviour.
///
/// Empty, and version 0 is the baseline: `SCHEMA` above *is* the 1.0 shape, so
/// every install gets it in one batch and no step ever runs. The three steps
/// that used to live here were pre-release archaeology — two of them (add
/// `connections.error_patterns`, then drop it again) cancelled out exactly, and
/// the third only added columns `SCHEMA` already declares. Collapsing them was
/// safe precisely once, before the first public release; from the first tagged
/// build onward the immutability rule above applies and a change goes in as a
/// new step, never by editing `SCHEMA` alone.
const MIGRATIONS: &[Migration] = &[];

/// Unused while `MIGRATIONS` is empty, but the idiom every step should use:
/// checking first is what makes a step safe to re-run against a database that
/// already has the column.
#[allow(dead_code)]
pub fn has_column(tx: &Transaction, table: &str, column: &str) -> rusqlite::Result<bool> {
    let mut stmt = tx.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        if row.get::<_, String>(1)? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Called with the open connection just before the first step runs. Returns
/// the path of the snapshot it wrote.
pub type BackupFn<'a> = dyn Fn(&SqlConnection) -> Result<String, String> + 'a;

/// Brings `conn` to the schema this build expects, or fails without having
/// changed anything the caller cannot recover from.
pub fn apply(conn: &mut SqlConnection, backup: &BackupFn) -> Result<(), String> {
    apply_steps(conn, MIGRATIONS, backup)
}

/// Schema version this build produces. Derived rather than written down twice,
/// so adding a migration cannot leave a stale constant behind.
pub fn latest_version() -> u32 {
    MIGRATIONS.iter().map(|m| m.version).max().unwrap_or(0)
}

fn apply_steps(
    conn: &mut SqlConnection,
    steps: &[Migration],
    backup: &BackupFn,
) -> Result<(), String> {
    let latest = steps.iter().map(|m| m.version).max().unwrap_or(0);
    let current = user_version(conn)?;

    // A newer build wrote this file. Opening it anyway — even read-only — is
    // how columns this binary does not know about get dropped on the next
    // write, so stop at the door.
    if current > latest {
        return Err(format!(
            "This database was created by a newer version of Kelyfos \
             (schema version {current}; this build understands {latest}). \
             Update Kelyfos to open it."
        ));
    }

    // A brand-new file and an existing one at the baseline both report version
    // 0, and only the emptiness of sqlite_master tells them apart. A fresh
    // database is born at the latest version, so no step ever runs against it.
    let fresh = is_empty(conn)?;

    // Taken before anything is written, and required: migrating with no way
    // back is worse than not starting.
    let snapshot = if !fresh && current < latest {
        Some(backup(conn).map_err(|e| {
            format!("Refusing to migrate — the backup taken first could not be written. {e}")
        })?)
    } else {
        None
    };

    create_schema(conn)?;

    match snapshot {
        Some(path) => run_steps(conn, steps, current, &path),
        // A fresh database is born at the latest version; an up-to-date one
        // was already there.
        None => {
            if fresh {
                set_user_version(conn, latest)?;
            }
            Ok(())
        }
    }
}

fn create_schema(conn: &SqlConnection) -> Result<(), String> {
    conn.execute_batch(SCHEMA)
        .map_err(|e| format!("failed to create schema: {e}"))
}

fn run_steps(
    conn: &mut SqlConnection,
    steps: &[Migration],
    current: u32,
    backup_path: &str,
) -> Result<(), String> {
    for m in steps.iter().filter(|m| m.version > current) {
        let tx = conn
            .transaction()
            .map_err(|e| format!("cannot begin migration {}: {e}", m.version))?;

        // The version bump rides in the same transaction as the change, so a
        // crash between the two is not representable.
        (m.run)(&tx)
            .and_then(|_| tx.pragma_update(None, "user_version", m.version))
            .map_err(|e| {
                format!(
                    "Migration {} ({}) failed: {e}\n\nThe database was left as it was. \
                     A backup taken before migrating is at {backup_path}",
                    m.version, m.name
                )
            })?;

        tx.commit()
            .map_err(|e| format!("cannot commit migration {}: {e}", m.version))?;
    }
    Ok(())
}

fn user_version(conn: &SqlConnection) -> Result<u32, String> {
    conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
        .map(|v| v.max(0) as u32)
        .map_err(|e| format!("cannot read schema version: {e}"))
}

fn set_user_version(conn: &SqlConnection, v: u32) -> Result<(), String> {
    conn.pragma_update(None, "user_version", v)
        .map_err(|e| format!("cannot set schema version: {e}"))
}

fn is_empty(conn: &SqlConnection) -> Result<bool, String> {
    conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
        [],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n == 0)
    .map_err(|e| format!("cannot inspect database: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    /// A directory that deletes itself, so tests can use real files (WAL,
    /// `VACUUM INTO` and `PRAGMA user_version` on a shared `:memory:` handle
    /// are not the thing we ship).
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!("kelyfos-test-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
        fn db(&self) -> PathBuf {
            self.0.join("app.db")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Records how often a backup was requested; writes a real snapshot so the
    /// path in an error message is a path that exists.
    struct SpyBackup {
        dir: PathBuf,
        calls: RefCell<usize>,
    }

    impl SpyBackup {
        fn new(dir: &Path) -> Self {
            SpyBackup { dir: dir.to_path_buf(), calls: RefCell::new(0) }
        }
        fn f(&self) -> impl Fn(&SqlConnection) -> Result<String, String> + '_ {
            move |conn: &SqlConnection| {
                let mut n = self.calls.borrow_mut();
                *n += 1;
                let target = self.dir.join(format!("snapshot-{n}.sqlite3"));
                conn.execute("VACUUM INTO ?1", [target.to_string_lossy().as_ref()])
                    .map_err(|e| e.to_string())?;
                Ok(target.to_string_lossy().to_string())
            }
        }
        fn calls(&self) -> usize {
            *self.calls.borrow()
        }
    }

    fn open(path: &Path) -> SqlConnection {
        let conn = SqlConnection::open(path).unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn
    }

    fn tables(conn: &SqlConnection) -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .unwrap();
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap();
        rows.map(|r| r.unwrap()).collect()
    }

    /// An existing database sitting at the baseline: the current schema, with
    /// rows in it, `user_version` still 0. That is what every install looks
    /// like until the first real migration ships, so it is the starting state
    /// the step tests below need — a database that is *not* empty, so
    /// `apply_steps` treats it as an upgrade rather than a birth.
    fn seed_baseline(conn: &SqlConnection) {
        create_schema(conn).unwrap();
        conn.execute_batch(
            r#"
            INSERT INTO workspaces (id, name, description, icon, layout, created_at, updated_at)
              VALUES ('w1', 'Prod', 'the real one', 'layout', '{"kind":"row"}', 't0', 't1');
            INSERT INTO connections (id, name, host, username, created_at, updated_at)
              VALUES ('c1', 'web01', 'example.test', 'root', 't0', 't1');
            INSERT INTO terminals (id, workspace_id, connection_id, title, sort_order)
              VALUES ('t1', 'w1', 'c1', 'logs', 3);
            "#,
        )
        .unwrap();
    }

    #[test]
    fn fresh_database_lands_at_latest_without_backup() {
        let dir = TempDir::new();
        let spy = SpyBackup::new(&dir.0);
        let mut conn = open(&dir.db());

        apply(&mut conn, &spy.f()).unwrap();

        assert_eq!(user_version(&conn).unwrap(), latest_version());
        assert_eq!(
            tables(&conn),
            vec![
                "app_state",
                "connections",
                "presets",
                "settings",
                "terminals",
                "variables",
                "workspaces"
            ]
        );
        assert_eq!(spy.calls(), 0, "a new database has nothing to snapshot");
    }

    /// The everyday case while `MIGRATIONS` is empty: a database with rows in
    /// it opens, gains nothing, loses nothing, and is not snapshotted. The row
    /// assertions are the real point — every column `row_to_*` reads must be
    /// present, because one missing column fails the whole list query and
    /// empties the sidebar rather than blanking a field.
    #[test]
    fn baseline_database_opens_without_migrating() {
        let dir = TempDir::new();
        let spy = SpyBackup::new(&dir.0);
        let mut conn = open(&dir.db());
        seed_baseline(&conn);
        assert_eq!(user_version(&conn).unwrap(), 0);

        apply(&mut conn, &spy.f()).unwrap();

        assert_eq!(user_version(&conn).unwrap(), latest_version());
        assert_eq!(spy.calls(), 0, "nothing migrated, so nothing to snapshot");
        assert!(tables(&conn).contains(&"presets".to_string()));

        let ws = conn
            .query_row("SELECT * FROM workspaces WHERE id='w1'", [], super::super::row_to_workspace)
            .unwrap();
        assert_eq!(ws.name, "Prod");
        assert_eq!(ws.description, "the real one");
        assert!(ws.tags.is_empty());
        assert_eq!(ws.layout["kind"], "row");

        let t = conn
            .query_row("SELECT * FROM terminals WHERE id='t1'", [], super::super::row_to_terminal)
            .unwrap();
        assert_eq!(t.title, "logs");
        assert_eq!(t.sort_order, 3);
        assert_eq!(t.shell, "");

        let c = conn
            .query_row("SELECT * FROM connections WHERE id='c1'", [], super::super::row_to_connection)
            .unwrap();
        assert_eq!(c.host, "example.test");

        // The whole point: the list queries the UI runs must not error.
        let mut stmt = conn.prepare("SELECT * FROM presets").unwrap();
        assert_eq!(stmt.query_map([], super::super::row_to_preset).unwrap().count(), 0);
    }

    #[test]
    fn current_database_runs_nothing() {
        let dir = TempDir::new();
        let spy = SpyBackup::new(&dir.0);
        let mut conn = open(&dir.db());
        seed_baseline(&conn);
        apply(&mut conn, &spy.f()).unwrap();
        assert_eq!(spy.calls(), 0);

        // A step that would blow up proves no step is reached the second time.
        let landmine = [Migration {
            version: latest_version(),
            name: "must not run",
            run: |_| Err(rusqlite::Error::InvalidQuery),
        }];
        apply_steps(&mut conn, &landmine, &spy.f()).unwrap();

        assert_eq!(user_version(&conn).unwrap(), latest_version());
        assert_eq!(spy.calls(), 0, "no snapshot on an ordinary launch");
    }

    #[test]
    fn failing_step_rolls_back_and_holds_the_version() {
        let dir = TempDir::new();
        let spy = SpyBackup::new(&dir.0);
        let mut conn = open(&dir.db());
        seed_baseline(&conn);
        apply(&mut conn, &spy.f()).unwrap();

        // Every shipped step has already run, so only this one is left.
        let steps = [Migration {
            version: latest_version() + 1,
            name: "half a change",
            run: |tx| {
                tx.execute("INSERT INTO settings (key, value) VALUES ('half', '1')", [])?;
                tx.execute_batch("ALTER TABLE workspaces ADD COLUMN tags TEXT")?; // already there
                Ok(())
            },
        }];

        let err = apply_steps(&mut conn, &steps, &spy.f()).unwrap_err();
        assert!(
            err.contains(&format!("Migration {} (half a change) failed", latest_version() + 1)),
            "{err}"
        );
        assert!(err.contains("backup taken before migrating is at"), "{err}");

        assert_eq!(
            user_version(&conn).unwrap(),
            latest_version(),
            "version must not advance"
        );
        let half: i64 = conn
            .query_row("SELECT count(*) FROM settings WHERE key='half'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(half, 0, "the step's earlier statement was rolled back");
        let name: String = conn
            .query_row("SELECT name FROM workspaces WHERE id='w1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(name, "Prod", "pre-existing data untouched");
    }

    /// A real transformation — new column, backfilled from an existing one —
    /// to show a step can do more than DDL. Test-local on purpose: it is not
    /// part of what ships.
    #[test]
    fn a_step_can_transform_data() {
        let dir = TempDir::new();
        let spy = SpyBackup::new(&dir.0);
        let mut conn = open(&dir.db());
        seed_baseline(&conn);
        conn.execute(
            "INSERT INTO workspaces (id, name, created_at, updated_at) VALUES ('w2','Staging','t0','t1')",
            [],
        )
        .unwrap();

        let steps = [Migration {
            version: 2,
            name: "nickname backfilled from name",
            run: |tx| {
                tx.execute_batch(
                    "ALTER TABLE workspaces ADD COLUMN nickname TEXT NOT NULL DEFAULT ''",
                )?;
                let rows: Vec<(String, String)> = tx
                    .prepare("SELECT id, name FROM workspaces")?
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<rusqlite::Result<_>>()?;
                for (id, name) in rows {
                    tx.execute(
                        "UPDATE workspaces SET nickname = ?2 WHERE id = ?1",
                        params![id, name.to_lowercase()],
                    )?;
                }
                Ok(())
            },
        }];

        apply_steps(&mut conn, &steps, &spy.f()).unwrap();

        assert_eq!(user_version(&conn).unwrap(), 2);
        let nick: String = conn
            .query_row("SELECT nickname FROM workspaces WHERE id='w2'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(nick, "staging");
    }

    #[test]
    fn database_from_the_future_is_refused() {
        let dir = TempDir::new();
        let spy = SpyBackup::new(&dir.0);
        let mut conn = open(&dir.db());
        apply(&mut conn, &spy.f()).unwrap();
        set_user_version(&conn, 99).unwrap();

        let err = apply(&mut conn, &spy.f()).unwrap_err();
        assert!(err.contains("newer version of Kelyfos"), "{err}");
        assert_eq!(user_version(&conn).unwrap(), 99, "left alone");
        assert_eq!(spy.calls(), 0);
    }

    /// `run_steps` bumps the version inside the step's own transaction, which
    /// is only atomic if the pragma itself rolls back. Pin that assumption.
    #[test]
    fn user_version_is_transactional() {
        let dir = TempDir::new();
        let mut conn = open(&dir.db());
        let tx = conn.transaction().unwrap();
        tx.pragma_update(None, "user_version", 7u32).unwrap();
        tx.rollback().unwrap();
        assert_eq!(user_version(&conn).unwrap(), 0);
    }

    /// Guards the invariant `run_steps` relies on to apply steps in order.
    /// Passes vacuously while `MIGRATIONS` is empty — it is here for the first
    /// step that gets added back, which must start at 1 because 0 is the
    /// baseline every install is already stamped with.
    #[test]
    fn shipped_migrations_are_sorted_and_unique() {
        let versions: Vec<u32> = MIGRATIONS.iter().map(|m| m.version).collect();
        let mut sorted = versions.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(versions, sorted, "steps must be ascending with no duplicates");
        if let Some(first) = versions.first() {
            assert_eq!(*first, 1, "version 0 is the baseline, so steps start at 1");
        }
    }
}
