//! Dictation history and usage stats in SQLite (`Mew Data\mew.db`).
//! Every dictation keeps its numbers; its text is kept only as long as the user's history setting allows.

use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};

use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use serde::Serialize;

use crate::config;

static DB: OnceLock<Mutex<Connection>> = OnceLock::new();

/// Typing speed used for "time saved": a typical rate for everyday typing.
const TYPING_WPM: f64 = 40.0;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: i64,
    pub at: i64, // unix ms
    pub app: String,
    pub text: Option<String>,
    pub raw: Option<String>,
    pub words: i64,
    pub audio_ms: i64,
    pub latency_ms: i64,
    pub cleaned: bool,
}

pub struct NewEntry<'a> {
    pub app: &'a str,
    pub text: &'a str,
    pub raw: &'a str,
    pub audio_ms: i64,
    pub asr_ms: i64,
    pub cleanup_ms: i64,
    pub latency_ms: i64,
    pub keep_text: bool,
}

pub fn init() -> Result<()> {
    std::fs::create_dir_all(config::data_dir())?;
    let db = Connection::open(config::data_dir().join("mew.db"))?;
    db.execute_batch(
        "PRAGMA journal_mode = WAL;
         CREATE TABLE IF NOT EXISTS dictations (
             id INTEGER PRIMARY KEY,
             at INTEGER NOT NULL,
             app TEXT NOT NULL,
             text TEXT,
             raw TEXT,
             words INTEGER NOT NULL,
             audio_ms INTEGER NOT NULL,
             asr_ms INTEGER NOT NULL,
             cleanup_ms INTEGER NOT NULL,
             latency_ms INTEGER NOT NULL,
             cleaned INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS dictations_at ON dictations(at);",
    )?;
    DB.set(Mutex::new(db)).ok();
    Ok(())
}

fn with<T>(f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T> {
    let db = DB.get().context("history not initialized")?;
    Ok(f(&db.lock().unwrap())?)
}

pub fn add(e: &NewEntry) -> Result<Entry> {
    let at = now_ms();
    let words = e.text.split_whitespace().count() as i64;
    let cleaned = e.text != e.raw;
    let (text, raw) = if e.keep_text { (Some(e.text), cleaned.then_some(e.raw)) } else { (None, None) };
    let id = with(|db| {
        db.execute(
            "INSERT INTO dictations (at, app, text, raw, words, audio_ms, asr_ms, cleanup_ms, latency_ms, cleaned)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![at, e.app, text, raw, words, e.audio_ms, e.asr_ms, e.cleanup_ms, e.latency_ms, cleaned],
        )?;
        Ok(db.last_insert_rowid())
    })?;
    Ok(Entry {
        id,
        at,
        app: e.app.to_string(),
        text: text.map(String::from),
        raw: raw.map(String::from),
        words,
        audio_ms: e.audio_ms,
        latency_ms: e.latency_ms,
        cleaned,
    })
}

/// Entries that still have text, newest first. `query` filters by substring; `before` (unix ms) pages back in time.
pub fn list(query: &str, before: Option<i64>, limit: i64) -> Result<Vec<Entry>> {
    let pattern = format!("%{}%", query.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
    with(|db| {
        let mut stmt = db.prepare(
            "SELECT id, at, app, text, raw, words, audio_ms, latency_ms, cleaned FROM dictations
             WHERE text IS NOT NULL AND at < ?1 AND (text LIKE ?2 ESCAPE '\\' OR app LIKE ?2 ESCAPE '\\')
             ORDER BY at DESC LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![before.unwrap_or(i64::MAX), pattern, limit], |r| {
            Ok(Entry {
                id: r.get(0)?,
                at: r.get(1)?,
                app: r.get(2)?,
                text: r.get(3)?,
                raw: r.get(4)?,
                words: r.get(5)?,
                audio_ms: r.get(6)?,
                latency_ms: r.get(7)?,
                cleaned: r.get(8)?,
            })
        })?;
        rows.collect()
    })
}

/// Forgets one transcript's text (its numbers stay in the stats).
pub fn forget(id: i64) -> Result<()> {
    with(|db| db.execute("UPDATE dictations SET text = NULL, raw = NULL WHERE id = ?1", [id]).map(drop))
}

pub fn forget_all() -> Result<()> {
    with(|db| db.execute("UPDATE dictations SET text = NULL, raw = NULL", []).map(drop))
}

pub fn reset() -> Result<()> {
    with(|db| db.execute("DELETE FROM dictations", []).map(drop))
}

/// Drops text older than the retention window.
pub fn prune(keep: config::KeepHistory) -> Result<()> {
    let Some(days) = keep.days() else { return Ok(()) };
    let cutoff = now_ms() - days * 86_400_000;
    with(|db| {
        db.execute("UPDATE dictations SET text = NULL, raw = NULL WHERE at < ?1 AND text IS NOT NULL", [cutoff])
            .map(drop)
    })
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub total_words: i64,
    pub total_dictations: i64,
    pub words_today: i64,
    pub words_week: i64,
    pub minutes_saved: f64,
    pub speaking_wpm: f64,
    pub median_latency_ms: i64,
    pub streak_days: i64,
    /// Last 30 days, oldest first: (local date YYYY-MM-DD, words).
    pub daily: Vec<(String, i64)>,
    /// Top apps by words: (app, words, dictations).
    pub apps: Vec<(String, i64, i64)>,
}

pub fn stats() -> Result<Stats> {
    with(|db| {
        let mut s = Stats::default();
        let (words, count, audio_ms): (i64, i64, i64) = db.query_row(
            "SELECT COALESCE(SUM(words), 0), COUNT(*), COALESCE(SUM(audio_ms), 0) FROM dictations",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        s.total_words = words;
        s.total_dictations = count;
        let speaking_min = audio_ms as f64 / 60_000.0;
        s.speaking_wpm = if speaking_min > 0.0 { words as f64 / speaking_min } else { 0.0 };
        s.minutes_saved = (words as f64 / TYPING_WPM - speaking_min).max(0.0);

        let day = "date(at / 1000, 'unixepoch', 'localtime')";
        s.words_today = db.query_row(
            &format!("SELECT COALESCE(SUM(words), 0) FROM dictations WHERE {day} = date('now', 'localtime')"),
            [],
            |r| r.get(0),
        )?;
        s.words_week = db.query_row(
            &format!(
                "SELECT COALESCE(SUM(words), 0) FROM dictations WHERE {day} > date('now', 'localtime', '-7 days')"
            ),
            [],
            |r| r.get(0),
        )?;

        let mut latencies: Vec<i64> = db
            .prepare("SELECT latency_ms FROM dictations ORDER BY at DESC LIMIT 200")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        latencies.sort_unstable();
        s.median_latency_ms = latencies.get(latencies.len() / 2).copied().unwrap_or(0);

        // Daily words for the last 30 days, with empty days filled in.
        let mut by_day = std::collections::HashMap::new();
        let mut stmt = db.prepare(&format!(
            "SELECT {day} AS d, SUM(words) FROM dictations WHERE {day} > date('now', 'localtime', '-30 days') GROUP BY d"
        ))?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
            let (d, w) = row?;
            by_day.insert(d, w);
        }
        for offset in (0..30).rev() {
            let d: String =
                db.query_row(&format!("SELECT date('now', 'localtime', '-{offset} days')"), [], |r| r.get(0))?;
            let w = by_day.get(&d).copied().unwrap_or(0);
            s.daily.push((d, w));
        }

        s.apps = db
            .prepare("SELECT app, SUM(words) AS w, COUNT(*) FROM dictations GROUP BY app ORDER BY w DESC LIMIT 6")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;

        // Streak: consecutive days with dictation, ending today (or yesterday, so it survives until you talk today).
        let days: BTreeSet<String> = db
            .prepare(&format!("SELECT DISTINCT {day} FROM dictations"))?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let today: String = db.query_row("SELECT date('now', 'localtime')", [], |r| r.get(0))?;
        let mut offset = if days.contains(&today) { 0 } else { 1 };
        loop {
            let d: String =
                db.query_row(&format!("SELECT date('now', 'localtime', '-{offset} days')"), [], |r| r.get(0))?;
            if !days.contains(&d) {
                break;
            }
            s.streak_days += 1;
            offset += 1;
        }
        Ok(s)
    })
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// Fills an empty database with plausible dictations for screenshots and UI work (dev only).
pub fn seed_demo() -> Result<()> {
    let samples = [
        ("Slack", "Sounds good, I'll take a look after lunch and get back to you."),
        ("Google Chrome", "How do I configure a reverse proxy for a Rust web server on Windows?"),
        ("Visual Studio Code", "Add a guard so the overlay re-shows itself after Alt-Tab."),
        ("Outlook", "Hi Priya, thanks for the notes. Can we move the review to Thursday at 4?"),
        ("Slack", "Merged. The installer is about 49 MB now, mostly the Vulkan backend."),
        ("Notion", "Next steps:\n1. Ship the history window\n2. Code-sign the installer\n3. Set up auto-updates"),
        ("Google Chrome", "best mechanical keyboard switches for quiet typing"),
        ("WhatsApp", "On my way, see you in 10."),
    ];
    let now = now_ms();
    with(|db| {
        for day in 0..24i64 {
            if day % 7 == 5 {
                continue; // a few empty days
            }
            let n = 2 + (day * 7 % 6);
            for i in 0..n {
                let (app, text) = samples[((day * 3 + i) % samples.len() as i64) as usize];
                let at = now - day * 86_400_000 - i * 2_700_000 - 600_000;
                let words = text.split_whitespace().count() as i64;
                let audio_ms = words * 420 + 600;
                db.execute(
                    "INSERT INTO dictations (at, app, text, raw, words, audio_ms, asr_ms, cleanup_ms, latency_ms, cleaned)
                     VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, ?7, ?8, 0)",
                    params![at, app, text, words, audio_ms, 80 + words * 4, 140, 260 + words * 6],
                )?;
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_list_search_forget_and_stats() {
        let dir = std::env::temp_dir().join(format!("mew-history-test-{}", std::process::id()));
        // SAFETY: single-threaded test setup before any other access to the environment.
        unsafe { std::env::set_var("MEW_DATA_DIR", &dir) };
        init().unwrap();
        let entry = |text: &'static str, keep_text| NewEntry {
            app: "Slack",
            text,
            raw: "um so ".to_owned().leak(),
            audio_ms: 3000,
            asr_ms: 90,
            cleanup_ms: 150,
            latency_ms: 300,
            keep_text,
        };
        let first = add(&entry("Ship it today.", true)).unwrap();
        add(&entry("Private words", false)).unwrap();
        assert!(first.cleaned && first.raw.is_some());

        let all = list("", None, 10).unwrap();
        assert_eq!(all.len(), 1, "entries without text are not listed");
        assert_eq!(list("ship", None, 10).unwrap().len(), 1);
        assert_eq!(list("slack", None, 10).unwrap().len(), 1, "search matches the app name too");
        assert!(list("100%_", None, 10).unwrap().is_empty(), "LIKE wildcards are escaped");

        let s = stats().unwrap();
        assert_eq!((s.total_dictations, s.total_words, s.words_today, s.streak_days), (2, 5, 5, 1));
        assert_eq!(s.daily.len(), 30);
        assert_eq!(s.daily.last().unwrap().1, 5);

        forget(first.id).unwrap();
        assert!(list("", None, 10).unwrap().is_empty());
        assert_eq!(stats().unwrap().total_words, 5, "forgetting text keeps the numbers");
        reset().unwrap();
        assert_eq!(stats().unwrap().total_dictations, 0);
        let _ = std::fs::remove_dir_all(dir);
    }
}
