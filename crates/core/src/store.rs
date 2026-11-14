//! SQLite persistence for download metadata and history.
//!
//! aria2's session file resumes unfinished transfers; this store keeps what
//! aria2 forgets: stable ids, categories, timestamps and finished history.

use crate::category::Category;
use crate::model::{Download, DownloadKind, FileEntry, State};
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

pub struct Store {
    conn: Connection,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS downloads (
    id          TEXT PRIMARY KEY,
    gid         TEXT,
    kind        TEXT NOT NULL,
    name        TEXT NOT NULL,
    uris        TEXT NOT NULL,
    dir         TEXT NOT NULL,
    state       TEXT NOT NULL,
    total       INTEGER NOT NULL DEFAULT 0,
    completed   INTEGER NOT NULL DEFAULT 0,
    uploaded    INTEGER NOT NULL DEFAULT 0,
    category    TEXT NOT NULL,
    added_at    INTEGER NOT NULL,
    finished_at INTEGER,
    error       TEXT,
    files       TEXT NOT NULL DEFAULT '[]',
    info_hash   TEXT,
    speed_limit INTEGER,
    headers     TEXT NOT NULL DEFAULT '[]',
    video       TEXT
);
CREATE INDEX IF NOT EXISTS downloads_gid ON downloads(gid);
"#;

/// Current schema version (`PRAGMA user_version`).
const VERSION: i64 = 2;

/// Creates the schema, or upgrades one written by an older version.
fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(SCHEMA)?;
    // Columns added after v1. Checked by name rather than by version, since an
    // older YADM opening the database resets `user_version`.
    let has = |col: &str| -> Result<bool> {
        let mut stmt = conn.prepare("SELECT 1 FROM pragma_table_info('downloads') WHERE name = ?1")?;
        Ok(stmt.exists([col])?)
    };
    if !has("video")? {
        conn.execute_batch("ALTER TABLE downloads ADD COLUMN video TEXT;")?;
    }
    conn.pragma_update(None, "user_version", VERSION)?;
    Ok(())
}

fn files_to_json(files: &[FileEntry]) -> String {
    serde_json::Value::Array(
        files
            .iter()
            .map(|f| serde_json::json!([f.index, f.path, f.length, f.completed, f.selected]))
            .collect(),
    )
    .to_string()
}

fn files_from_json(s: &str) -> Vec<FileEntry> {
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str(s) else {
        return vec![];
    };
    items
        .into_iter()
        .filter_map(|v| {
            let a = v.as_array()?;
            Some(FileEntry {
                index: a.first()?.as_u64()?,
                path: a.get(1)?.as_str()?.to_owned(),
                length: a.get(2)?.as_u64()?,
                completed: a.get(3)?.as_u64()?,
                selected: a.get(4)?.as_bool()?,
            })
        })
        .collect()
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        migrate(&conn)?;
        Ok(Self { conn })
    }

    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        migrate(&conn)?;
        Ok(Self { conn })
    }

    pub fn upsert(&self, d: &Download) -> Result<()> {
        upsert_on(&self.conn, d)
    }

    pub fn upsert_many<'a>(&mut self, items: impl IntoIterator<Item = &'a Download>) -> Result<()> {
        let tx = self.conn.transaction()?;
        for d in items {
            upsert_on(&tx, d)?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM downloads WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<Option<Download>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {COLS} FROM downloads WHERE id = ?1"),
                [id],
                row_to_download,
            )
            .optional()?)
    }

    pub fn all(&self) -> Result<Vec<Download>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {COLS} FROM downloads ORDER BY added_at DESC"
        ))?;
        let rows = stmt.query_map([], row_to_download)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

fn upsert_on(conn: &Connection, d: &Download) -> Result<()> {
    conn.execute(
            r#"INSERT INTO downloads (id,gid,kind,name,uris,dir,state,total,completed,uploaded,category,added_at,finished_at,error,files,info_hash,speed_limit,headers,video)
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)
               ON CONFLICT(id) DO UPDATE SET gid=?2,kind=?3,name=?4,uris=?5,dir=?6,state=?7,total=?8,completed=?9,uploaded=?10,
                 category=?11,finished_at=?13,error=?14,files=?15,info_hash=?16,speed_limit=?17,headers=?18,video=?19"#,
            params![
                d.id,
                d.gid,
                d.kind.as_str(),
                d.name,
                serde_json::to_string(&d.uris)?,
                d.dir,
                d.state.as_str(),
                d.total as i64,
                d.completed as i64,
                d.uploaded as i64,
                d.category.as_str(),
                d.added_at,
                d.finished_at,
                d.error,
                files_to_json(&d.files),
                d.info_hash,
                d.speed_limit.map(|v| v as i64),
                serde_json::to_string(&d.headers)?,
                d.video.as_ref().map(serde_json::to_string).transpose()?,
            ],
    )?;
    Ok(())
}

const COLS: &str = "id,gid,kind,name,uris,dir,state,total,completed,uploaded,category,added_at,finished_at,error,files,info_hash,speed_limit,headers,video";

fn row_to_download(r: &rusqlite::Row<'_>) -> rusqlite::Result<Download> {
    let uris: String = r.get(4)?;
    let files: String = r.get(14)?;
    let headers: String = r.get(17)?;
    Ok(Download {
        id: r.get(0)?,
        gid: r.get(1)?,
        kind: DownloadKind::parse(&r.get::<_, String>(2)?),
        name: r.get(3)?,
        uris: serde_json::from_str(&uris).unwrap_or_default(),
        dir: r.get(5)?,
        state: State::parse(&r.get::<_, String>(6)?),
        total: r.get::<_, i64>(7)? as u64,
        completed: r.get::<_, i64>(8)? as u64,
        uploaded: r.get::<_, i64>(9)? as u64,
        category: Category::parse(&r.get::<_, String>(10)?),
        added_at: r.get(11)?,
        finished_at: r.get(12)?,
        error: r.get(13)?,
        files: files_from_json(&files),
        info_hash: r.get(15)?,
        speed_limit: r.get::<_, Option<i64>>(16)?.map(|v| v as u64),
        headers: serde_json::from_str(&headers).unwrap_or_default(),
        video: r
            .get::<_, Option<String>>(18)?
            .and_then(|v| serde_json::from_str(&v).ok()),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crud() {
        let mut s = Store::in_memory().unwrap();
        let mut d = Download {
            id: "a".into(),
            gid: Some("g1".into()),
            name: "file.zip".into(),
            uris: vec!["http://x/file.zip".into()],
            dir: "/tmp".into(),
            state: State::Active,
            category: Category::Archives,
            added_at: 10,
            files: vec![FileEntry {
                index: 1,
                path: "/tmp/file.zip".into(),
                length: 5,
                completed: 2,
                selected: true,
            }],
            headers: vec!["Cookie: a=b".into()],
            speed_limit: Some(1000),
            ..Default::default()
        };
        s.upsert(&d).unwrap();
        assert_eq!(s.get("a").unwrap().unwrap(), d);
        d.state = State::Completed;
        d.finished_at = Some(20);
        let e = Download {
            id: "b".into(),
            added_at: 30,
            ..d.clone()
        };
        s.upsert_many([&d, &e]).unwrap();
        let all = s.all().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, "b");
        assert_eq!(all[1].state, State::Completed);
        s.delete("a").unwrap();
        assert!(s.get("a").unwrap().is_none());

        let v = Download {
            id: "v".into(),
            kind: DownloadKind::Video,
            video: Some(crate::model::VideoMeta {
                selector: "bv*+ba/b".into(),
                quality_label: "Best".into(),
                partials: vec!["/tmp/x.part".into()],
                ..Default::default()
            }),
            ..Default::default()
        };
        s.upsert(&v).unwrap();
        assert_eq!(s.get("v").unwrap().unwrap(), v);
    }

    #[test]
    fn migrates_v1() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("yadm.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE downloads (id TEXT PRIMARY KEY, gid TEXT, kind TEXT NOT NULL, name TEXT NOT NULL,
                 uris TEXT NOT NULL, dir TEXT NOT NULL, state TEXT NOT NULL, total INTEGER NOT NULL DEFAULT 0,
                 completed INTEGER NOT NULL DEFAULT 0, uploaded INTEGER NOT NULL DEFAULT 0, category TEXT NOT NULL,
                 added_at INTEGER NOT NULL, finished_at INTEGER, error TEXT, files TEXT NOT NULL DEFAULT '[]',
                 info_hash TEXT, speed_limit INTEGER, headers TEXT NOT NULL DEFAULT '[]');
                 INSERT INTO downloads (id,kind,name,uris,dir,state,category,added_at)
                   VALUES ('old','http','a.zip','[]','/tmp','completed','archives',1);
                 PRAGMA user_version = 1;",
            )
            .unwrap();
        }
        let s = Store::open(&path).unwrap();
        let old = s.get("old").unwrap().unwrap();
        assert_eq!(old.name, "a.zip");
        assert!(old.video.is_none());
        drop(s);
        // Reopening an up-to-date database is a no-op, even after an older
        // version reset the schema version.
        Connection::open(&path).unwrap().execute_batch("PRAGMA user_version = 1;").unwrap();
        assert!(Store::open(&path).unwrap().get("old").unwrap().is_some());
    }
}
