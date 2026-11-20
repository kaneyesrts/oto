use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub struct Db(pub Mutex<Connection>);

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub id: String,
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub duration_ms: u64,
    pub track_no: Option<u32>,
    pub year: Option<u32>,
    pub genre: Option<String>,
    pub cover: Option<String>,
    /// Up to five RGB colours extracted from the cover, most prominent first.
    pub palette: Vec<[u8; 3]>,
    pub liked: bool,
    pub added_at: i64,
    pub source: String,
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl Db {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            CREATE TABLE IF NOT EXISTS tracks (
                id TEXT PRIMARY KEY,
                path TEXT NOT NULL UNIQUE,
                title TEXT NOT NULL,
                artist TEXT NOT NULL,
                album TEXT NOT NULL,
                album_artist TEXT NOT NULL,
                duration_ms INTEGER NOT NULL,
                track_no INTEGER,
                year INTEGER,
                genre TEXT,
                cover TEXT,
                palette TEXT NOT NULL DEFAULT '[]',
                mtime INTEGER NOT NULL,
                added_at INTEGER NOT NULL,
                liked INTEGER NOT NULL DEFAULT 0,
                source TEXT NOT NULL DEFAULT 'local'
            );
            CREATE TABLE IF NOT EXISTS plays (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                track_id TEXT NOT NULL,
                started_at INTEGER NOT NULL,
                listened_ms INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL,
                skipped INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS plays_started ON plays(started_at);
            CREATE TABLE IF NOT EXISTS lyrics_cache (
                track_id TEXT PRIMARY KEY,
                json TEXT NOT NULL,
                fetched_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS playlists (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS playlist_tracks (
                playlist_id INTEGER NOT NULL,
                position INTEGER NOT NULL,
                track_id TEXT NOT NULL,
                added_at INTEGER NOT NULL,
                PRIMARY KEY (playlist_id, position)
            );
            CREATE TABLE IF NOT EXISTS lyrics_offset (
                track_id TEXT PRIMARY KEY,
                offset_ms INTEGER NOT NULL
            );
            "#,
        )?;
        Ok(Db(Mutex::new(conn)))
    }

    pub fn all_tracks(&self) -> Vec<Track> {
        let c = self.0.lock();
        let mut st = c
            .prepare("SELECT * FROM tracks ORDER BY album_artist COLLATE NOCASE, album COLLATE NOCASE, track_no, title")
            .unwrap();
        st.query_map([], row_to_track).unwrap().filter_map(Result::ok).collect()
    }

    pub fn track(&self, id: &str) -> Option<Track> {
        let c = self.0.lock();
        c.query_row("SELECT * FROM tracks WHERE id = ?", [id], row_to_track).optional().ok().flatten()
    }

    pub fn mtime_of(&self, path: &str) -> Option<i64> {
        let c = self.0.lock();
        c.query_row("SELECT mtime FROM tracks WHERE path = ?", [path], |r| r.get(0)).optional().ok().flatten()
    }

    pub fn upsert_tracks(&self, items: &[(Track, i64)]) {
        let mut c = self.0.lock();
        let tx = c.transaction().unwrap();
        {
            let mut st = tx
                .prepare(
                    "INSERT INTO tracks (id, path, title, artist, album, album_artist, duration_ms, track_no, year, genre, cover, palette, mtime, added_at, source)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
                     ON CONFLICT(id) DO UPDATE SET title=?3, artist=?4, album=?5, album_artist=?6, duration_ms=?7,
                       track_no=?8, year=?9, genre=?10, cover=?11, palette=?12, mtime=?13",
                )
                .unwrap();
            for (t, mtime) in items {
                let _ = st.execute(params![
                    t.id,
                    t.path,
                    t.title,
                    t.artist,
                    t.album,
                    t.album_artist,
                    t.duration_ms as i64,
                    t.track_no,
                    t.year,
                    t.genre,
                    t.cover,
                    serde_json::to_string(&t.palette).unwrap_or_else(|_| "[]".into()),
                    mtime,
                    t.added_at,
                    t.source,
                ]);
            }
        }
        let _ = tx.commit();
    }

    /// Removes tracks under `root` whose files no longer exist.
    pub fn prune_missing(&self, root: &str) -> usize {
        let paths: Vec<(String, String)> = {
            let c = self.0.lock();
            let mut st = c.prepare("SELECT id, path FROM tracks WHERE source = 'local'").unwrap();
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().filter_map(Result::ok).collect()
        };
        let gone: Vec<String> = paths
            .into_iter()
            .filter(|(_, p)| (root.is_empty() || p.starts_with(root)) && !Path::new(p).exists())
            .map(|(id, _)| id)
            .collect();
        let c = self.0.lock();
        for id in &gone {
            let _ = c.execute("DELETE FROM tracks WHERE id = ?", [id]);
        }
        gone.len()
    }

    pub fn remove_under(&self, root: &str) {
        let c = self.0.lock();
        let _ = c.execute("DELETE FROM tracks WHERE source = 'local' AND path LIKE ?1 || '%'", [root]);
    }

    pub fn set_liked(&self, id: &str, liked: bool) {
        let c = self.0.lock();
        let _ = c.execute("UPDATE tracks SET liked = ? WHERE id = ?", params![liked as i32, id]);
    }

    pub fn record_play(&self, track_id: &str, started_at: i64, listened_ms: u64, duration_ms: u64, skipped: bool) {
        let c = self.0.lock();
        let _ = c.execute(
            "INSERT INTO plays (track_id, started_at, listened_ms, duration_ms, skipped) VALUES (?, ?, ?, ?, ?)",
            params![track_id, started_at, listened_ms as i64, duration_ms as i64, skipped as i32],
        );
    }

    pub fn lyrics_cached(&self, id: &str) -> Option<(String, i64)> {
        let c = self.0.lock();
        c.query_row("SELECT json, fetched_at FROM lyrics_cache WHERE track_id = ?", [id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()
        .ok()
        .flatten()
    }

    pub fn cache_lyrics(&self, id: &str, json: &str) {
        let c = self.0.lock();
        let _ = c.execute(
            "INSERT OR REPLACE INTO lyrics_cache (track_id, json, fetched_at) VALUES (?, ?, ?)",
            params![id, json, now_ms()],
        );
    }

    pub fn drop_lyrics_cache(&self, id: &str) {
        let c = self.0.lock();
        let _ = c.execute("DELETE FROM lyrics_cache WHERE track_id = ?", [id]);
    }

    pub fn lyrics_offset(&self, id: &str) -> Option<i32> {
        let c = self.0.lock();
        c.query_row("SELECT offset_ms FROM lyrics_offset WHERE track_id = ?", [id], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }

    pub fn set_lyrics_offset(&self, id: &str, ms: i32) {
        let c = self.0.lock();
        let _ = c.execute(
            "INSERT OR REPLACE INTO lyrics_offset (track_id, offset_ms) VALUES (?, ?)",
            params![id, ms],
        );
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    pub count: u32,
    pub duration_ms: u64,
    /// Up to four distinct covers for the mosaic.
    pub covers: Vec<String>,
    pub updated_at: i64,
}

impl Db {
    pub fn playlists(&self) -> Vec<Playlist> {
        let c = self.0.lock();
        let mut st = c.prepare("SELECT id, name, updated_at FROM playlists ORDER BY updated_at DESC").unwrap();
        let base: Vec<(i64, String, i64)> =
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().filter_map(Result::ok).collect();
        base.into_iter()
            .map(|(id, name, updated_at)| {
                let (count, duration_ms): (i64, i64) = c
                    .query_row(
                        "SELECT COUNT(*), COALESCE(SUM(t.duration_ms), 0) FROM playlist_tracks p LEFT JOIN tracks t ON t.id = p.track_id WHERE p.playlist_id = ?",
                        [id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .unwrap_or((0, 0));
                let mut cs = c
                    .prepare("SELECT DISTINCT t.cover FROM playlist_tracks p JOIN tracks t ON t.id = p.track_id WHERE p.playlist_id = ? AND t.cover IS NOT NULL ORDER BY p.position LIMIT 4")
                    .unwrap();
                let covers = cs.query_map([id], |r| r.get(0)).unwrap().filter_map(Result::ok).collect();
                Playlist { id, name, count: count as u32, duration_ms: duration_ms as u64, covers, updated_at }
            })
            .collect()
    }

    pub fn playlist_tracks(&self, id: i64) -> Vec<String> {
        let c = self.0.lock();
        let mut st = c.prepare("SELECT track_id FROM playlist_tracks WHERE playlist_id = ? ORDER BY position").unwrap();
        st.query_map([id], |r| r.get(0)).unwrap().filter_map(Result::ok).collect()
    }

    pub fn playlist_create(&self, name: &str) -> i64 {
        let c = self.0.lock();
        let now = now_ms();
        let _ = c.execute("INSERT INTO playlists (name, created_at, updated_at) VALUES (?, ?, ?)", params![name, now, now]);
        c.last_insert_rowid()
    }

    pub fn playlist_rename(&self, id: i64, name: &str) {
        let c = self.0.lock();
        let _ = c.execute("UPDATE playlists SET name = ?, updated_at = ? WHERE id = ?", params![name, now_ms(), id]);
    }

    pub fn playlist_delete(&self, id: i64) {
        let c = self.0.lock();
        let _ = c.execute("DELETE FROM playlist_tracks WHERE playlist_id = ?", [id]);
        let _ = c.execute("DELETE FROM playlists WHERE id = ?", [id]);
    }

    /// Rewrites the whole ordering. Playlists are small; this keeps positions dense and simple.
    pub fn playlist_set(&self, id: i64, ids: &[String]) {
        let mut c = self.0.lock();
        let tx = c.transaction().unwrap();
        let now = now_ms();
        let _ = tx.execute("DELETE FROM playlist_tracks WHERE playlist_id = ?", [id]);
        {
            let mut st = tx.prepare("INSERT INTO playlist_tracks (playlist_id, position, track_id, added_at) VALUES (?, ?, ?, ?)").unwrap();
            for (i, t) in ids.iter().enumerate() {
                let _ = st.execute(params![id, i as i64, t, now]);
            }
        }
        let _ = tx.execute("UPDATE playlists SET updated_at = ? WHERE id = ?", params![now, id]);
        let _ = tx.commit();
    }
}

fn row_to_track(r: &Row) -> rusqlite::Result<Track> {
    let palette: String = r.get("palette")?;
    Ok(Track {
        id: r.get("id")?,
        path: r.get("path")?,
        title: r.get("title")?,
        artist: r.get("artist")?,
        album: r.get("album")?,
        album_artist: r.get("album_artist")?,
        duration_ms: r.get::<_, i64>("duration_ms")? as u64,
        track_no: r.get("track_no")?,
        year: r.get("year")?,
        genre: r.get("genre")?,
        cover: r.get("cover")?,
        palette: serde_json::from_str(&palette).unwrap_or_default(),
        liked: r.get::<_, i64>("liked")? != 0,
        added_at: r.get("added_at")?,
        source: r.get("source")?,
    })
}
