use crate::db::{now_ms, Db, Track};
use rusqlite::params;
use serde::Serialize;

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TopTrack {
    pub track: Track,
    pub plays: u32,
    pub ms: u64,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TopArtist {
    pub artist: String,
    pub plays: u32,
    pub ms: u64,
    pub cover: Option<String>,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Day {
    pub day: String,
    pub ms: u64,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub range_ms: u64,
    pub today_ms: u64,
    pub plays: u32,
    pub skips: u32,
    pub unique_tracks: u32,
    pub streak_days: u32,
    pub top_tracks: Vec<TopTrack>,
    pub top_artists: Vec<TopArtist>,
    /// [weekday 0=Mon][hour] listened ms
    pub heat: Vec<Vec<u64>>,
    pub daily: Vec<Day>,
    pub rising: Option<TopArtist>,
}

const COUNTED: &str = "listened_ms >= MIN(30000, duration_ms / 2)";

pub fn compute(db: &Db, range: &str) -> Stats {
    let day = 86_400_000i64;
    let now = now_ms();
    let from = match range {
        "week" => now - 7 * day,
        "month" => now - 30 * day,
        "year" => now - 365 * day,
        _ => 0,
    };
    let span = if from == 0 { now } else { now - from };

    let c = db.0.lock();
    let mut s = Stats::default();

    let one = |sql: &str, p: &[&dyn rusqlite::ToSql]| -> i64 { c.query_row(sql, p, |r| r.get::<_, Option<i64>>(0)).ok().flatten().unwrap_or(0) };
    s.range_ms = one("SELECT SUM(listened_ms) FROM plays WHERE started_at >= ?", &[&from]) as u64;
    s.today_ms = one(
        "SELECT SUM(listened_ms) FROM plays WHERE date(started_at/1000,'unixepoch','localtime') = date('now','localtime')",
        &[],
    ) as u64;
    s.plays = one(&format!("SELECT COUNT(*) FROM plays WHERE started_at >= ? AND {COUNTED}"), &[&from]) as u32;
    s.skips = one("SELECT COUNT(*) FROM plays WHERE started_at >= ? AND skipped = 1", &[&from]) as u32;
    s.unique_tracks = one(&format!("SELECT COUNT(DISTINCT track_id) FROM plays WHERE started_at >= ? AND {COUNTED}"), &[&from]) as u32;

    // Top tracks
    if let Ok(mut st) = c.prepare(&format!(
        "SELECT t.*, COUNT(*) AS n, SUM(p.listened_ms) AS ms FROM plays p JOIN tracks t ON t.id = p.track_id
         WHERE p.started_at >= ? AND {COUNTED} GROUP BY t.id ORDER BY n DESC, ms DESC LIMIT 10"
    )) {
        s.top_tracks = st
            .query_map([from], |r| {
                let n: i64 = r.get("n")?;
                let ms: i64 = r.get("ms")?;
                let palette: String = r.get("palette")?;
                Ok(TopTrack {
                    plays: n as u32,
                    ms: ms as u64,
                    track: Track {
                        id: r.get("id")?,
                        path: r.get("path")?,
                        title: r.get("title")?,
                        artist: r.get("artist")?,
                        album: r.get("album")?,
                        album_artist: r.get("album_artist")?,
                        duration_ms: r.get::<_, i64>("duration_ms")? as u64,
                        cover: r.get("cover")?,
                        palette: serde_json::from_str(&palette).unwrap_or_default(),
                        liked: r.get::<_, i64>("liked")? != 0,
                        source: r.get("source")?,
                        ..Default::default()
                    },
                })
            })
            .map(|it| it.filter_map(Result::ok).collect())
            .unwrap_or_default();
    }

    let artists = |from: i64, to: i64, limit: i64| -> Vec<TopArtist> {
        let Ok(mut st) = c.prepare(&format!(
            "SELECT t.artist, COUNT(*) AS n, SUM(p.listened_ms) AS ms, MAX(t.cover) FROM plays p JOIN tracks t ON t.id = p.track_id
             WHERE p.started_at >= ? AND p.started_at < ? AND {COUNTED} GROUP BY t.artist ORDER BY ms DESC LIMIT ?"
        )) else {
            return vec![];
        };
        st.query_map(params![from, to, limit], |r| {
            Ok(TopArtist { artist: r.get(0)?, plays: r.get::<_, i64>(1)? as u32, ms: r.get::<_, i64>(2)? as u64, cover: r.get(3)? })
        })
        .map(|it| it.filter_map(Result::ok).collect())
        .unwrap_or_default()
    };
    s.top_artists = artists(from, now + 1, 8);

    // "Rising": the artist whose listening grew most this week vs the previous one.
    let this_week = artists(now - 7 * day, now + 1, 50);
    let prev_week = artists(now - 14 * day, now - 7 * day, 200);
    s.rising = this_week
        .into_iter()
        .map(|a| {
            let before = prev_week.iter().find(|p| p.artist == a.artist).map(|p| p.ms).unwrap_or(0);
            (a.ms as i64 - before as i64, a)
        })
        .filter(|(d, a)| *d > 10 * 60_000 && a.plays >= 3)
        .max_by_key(|(d, _)| *d)
        .map(|(_, a)| a);

    // Heat map: weekday × hour. SQLite %w is 0=Sunday; shift so Monday comes first.
    s.heat = vec![vec![0u64; 24]; 7];
    if let Ok(mut st) = c.prepare(
        "SELECT CAST(strftime('%w', started_at/1000, 'unixepoch', 'localtime') AS INTEGER),
                CAST(strftime('%H', started_at/1000, 'unixepoch', 'localtime') AS INTEGER), SUM(listened_ms)
         FROM plays WHERE started_at >= ? GROUP BY 1, 2",
    ) {
        if let Ok(rows) = st.query_map([from], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))) {
            for (w, h, ms) in rows.flatten() {
                s.heat[((w + 6) % 7) as usize][h as usize] = ms as u64;
            }
        }
    }

    // Daily totals: last 30 days, or a range-matching span for shorter ranges.
    let days = if range == "week" { 7 } else { 30 };
    if let Ok(mut st) = c.prepare(
        "SELECT date(started_at/1000,'unixepoch','localtime') AS d, SUM(listened_ms) FROM plays
         WHERE started_at >= ? GROUP BY d ORDER BY d",
    ) {
        let rows: Vec<(String, i64)> = st
            .query_map([now - days * day], |r| Ok((r.get(0)?, r.get(1)?)))
            .map(|it| it.filter_map(Result::ok).collect())
            .unwrap_or_default();
        let dates: Vec<String> = (0..days)
            .rev()
            .filter_map(|i| c.query_row(&format!("SELECT date('now','localtime','-{i} day')"), [], |r| r.get(0)).ok())
            .collect();
        s.daily = dates
            .into_iter()
            .map(|d| {
                let ms = rows.iter().find(|(x, _)| *x == d).map(|(_, m)| *m as u64).unwrap_or(0);
                Day { day: d, ms }
            })
            .collect();
    }

    // Streak: consecutive days (ending today or yesterday) with at least one counted play.
    if let Ok(mut st) = c.prepare(&format!(
        "SELECT DISTINCT CAST(julianday(date(started_at/1000,'unixepoch','localtime')) AS INTEGER) FROM plays WHERE {COUNTED} ORDER BY 1 DESC"
    )) {
        let days: Vec<i64> = st.query_map([], |r| r.get(0)).map(|it| it.filter_map(Result::ok).collect()).unwrap_or_default();
        let today: i64 = c.query_row("SELECT CAST(julianday(date('now','localtime')) AS INTEGER)", [], |r| r.get(0)).unwrap_or(0);
        let mut expect = today;
        if days.first() == Some(&(today - 1)) {
            expect = today - 1;
        }
        for d in days {
            if d == expect {
                s.streak_days += 1;
                expect -= 1;
            } else if d < expect {
                break;
            }
        }
    }
    let _ = span;
    s
}
