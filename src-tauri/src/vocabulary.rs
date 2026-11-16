use std::path::Path;

use chrono::SecondsFormat;
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

#[derive(Serialize, Clone)]
pub struct VocabularyEntry {
    pub id: i64,
    pub word: String,
    pub definition: Option<String>,
    pub context_sentence: Option<String>,
    pub source_video: Option<String>,
    pub timestamp_ms: Option<i64>,
    pub date_added: String,
    pub ease_factor: f64,
    pub interval_days: i64,
    pub next_review_date: Option<String>,
    pub review_count: i64,
}

pub fn open(data_dir: &Path) -> Result<Connection, String> {
    std::fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
    let conn = Connection::open(data_dir.join("vocabulary.sqlite3")).map_err(|e| e.to_string())?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS vocabulary (
            id INTEGER PRIMARY KEY,
            word TEXT NOT NULL,
            definition TEXT,
            context_sentence TEXT,
            source_video TEXT,
            timestamp_ms INTEGER,
            date_added TEXT NOT NULL,
            ease_factor REAL NOT NULL DEFAULT 2.5,
            interval_days INTEGER NOT NULL DEFAULT 0,
            next_review_date TEXT,
            review_count INTEGER NOT NULL DEFAULT 0
        )",
        (),
    )
    .map_err(|e| e.to_string())?;

    Ok(conn)
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn add_entry(
    conn: &Connection,
    word: &str,
    definition: Option<&str>,
    context_sentence: Option<&str>,
    source_video: Option<&str>,
    timestamp_ms: Option<i64>,
) -> Result<VocabularyEntry, String> {
    let date_added = chrono::Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO vocabulary (word, definition, context_sentence, source_video, timestamp_ms, date_added)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        (word, definition, context_sentence, source_video, timestamp_ms, &date_added),
    )
    .map_err(|e| e.to_string())?;

    let id = conn.last_insert_rowid();

    Ok(VocabularyEntry {
        id,
        word: word.to_string(),
        definition: definition.map(str::to_string),
        context_sentence: context_sentence.map(str::to_string),
        source_video: source_video.map(str::to_string),
        timestamp_ms,
        date_added,
        ease_factor: 2.5,
        interval_days: 0,
        next_review_date: None,
        review_count: 0,
    })
}

const SELECT_COLUMNS: &str = "id, word, definition, context_sentence, source_video, timestamp_ms, \
date_added, ease_factor, interval_days, next_review_date, review_count";

fn row_to_entry(row: &rusqlite::Row) -> rusqlite::Result<VocabularyEntry> {
    Ok(VocabularyEntry {
        id: row.get(0)?,
        word: row.get(1)?,
        definition: row.get(2)?,
        context_sentence: row.get(3)?,
        source_video: row.get(4)?,
        timestamp_ms: row.get(5)?,
        date_added: row.get(6)?,
        ease_factor: row.get(7)?,
        interval_days: row.get(8)?,
        next_review_date: row.get(9)?,
        review_count: row.get(10)?,
    })
}

pub fn list_entries(conn: &Connection) -> Result<Vec<VocabularyEntry>, String> {
    let mut stmt = conn
        .prepare(&format!("SELECT {SELECT_COLUMNS} FROM vocabulary ORDER BY id DESC"))
        .map_err(|e| e.to_string())?;

    let rows = stmt.query_map((), row_to_entry).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

/// Words due for review right now: never reviewed (`next_review_date` is
/// still NULL from `add_entry`'s default) or past their scheduled date.
pub fn due_entries(conn: &Connection) -> Result<Vec<VocabularyEntry>, String> {
    let now = now_rfc3339();
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {SELECT_COLUMNS} FROM vocabulary \
             WHERE next_review_date IS NULL OR next_review_date <= ?1 \
             ORDER BY next_review_date IS NOT NULL, next_review_date ASC"
        ))
        .map_err(|e| e.to_string())?;

    let rows = stmt.query_map([&now], row_to_entry).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

pub fn delete_entry(conn: &Connection, id: i64) -> Result<(), String> {
    conn.execute("DELETE FROM vocabulary WHERE id = ?1", [id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

const MIN_EASE: f64 = 1.3;

/// A reduced SM-2 (SuperMemo 2) spaced-repetition schedule, matching the
/// sibling epub-reader app's vocab review: a 3-point grade instead of
/// SM-2's usual 0-5 (1 = Again, 3 = Good, 5 = Easy) is plenty of resolution
/// for "did I know this word" and keeps the review buttons simple.
///
/// On a lapse (`rating` < 3) the word doesn't get scheduled days out — it
/// comes back in 60 seconds so it can be retried in the same session,
/// mirroring how physical flashcard drills handle a miss.
fn sm2_update(ease_factor: f64, interval_days: i64, review_count: i64, rating: i64) -> (f64, i64, i64, String) {
    let now = chrono::Utc::now();

    if rating < 3 {
        let new_ease = (ease_factor - 0.2).max(MIN_EASE);
        let next = now + chrono::Duration::seconds(60);
        return (new_ease, 0, 0, next.to_rfc3339_opts(SecondsFormat::Millis, true));
    }

    // Interval uses the *old* ease factor (classic SM-2 ordering); the ease
    // factor itself is only adjusted for the *next* review after this one.
    let new_interval = if review_count == 0 {
        1
    } else if review_count == 1 {
        if rating >= 5 { 4 } else { 3 }
    } else {
        ((interval_days as f64) * ease_factor).round() as i64
    }
    .max(1);

    let q = rating as f64;
    let new_ease = (ease_factor + (0.1 - (5.0 - q) * (0.08 + (5.0 - q) * 0.02))).max(MIN_EASE);

    let next = now + chrono::Duration::days(new_interval);
    (new_ease, new_interval, review_count + 1, next.to_rfc3339_opts(SecondsFormat::Millis, true))
}

/// Applies a review grade to a word and persists the resulting SRS state.
/// `rating` is 1 (Again), 3 (Good), or 5 (Easy) — see `sm2_update`.
pub fn review_entry(conn: &Connection, id: i64, rating: i64) -> Result<VocabularyEntry, String> {
    let (ease_factor, interval_days, review_count): (f64, i64, i64) = conn
        .query_row(
            "SELECT ease_factor, interval_days, review_count FROM vocabulary WHERE id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "word not found".to_string())?;

    let (new_ease, new_interval, new_review_count, next_review_date) =
        sm2_update(ease_factor, interval_days, review_count, rating);

    conn.execute(
        "UPDATE vocabulary SET ease_factor = ?1, interval_days = ?2, review_count = ?3, next_review_date = ?4 \
         WHERE id = ?5",
        (new_ease, new_interval, new_review_count, &next_review_date, id),
    )
    .map_err(|e| e.to_string())?;

    conn.query_row(&format!("SELECT {SELECT_COLUMNS} FROM vocabulary WHERE id = ?1"), [id], row_to_entry)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_success_schedules_one_day_out() {
        let (_, interval, count, _) = sm2_update(2.5, 0, 0, 3);
        assert_eq!(interval, 1);
        assert_eq!(count, 1);
    }

    #[test]
    fn second_success_good_schedules_three_days() {
        let (_, interval, count, _) = sm2_update(2.5, 1, 1, 3);
        assert_eq!(interval, 3);
        assert_eq!(count, 2);
    }

    #[test]
    fn second_success_easy_schedules_four_days() {
        let (_, interval, _, _) = sm2_update(2.5, 1, 1, 5);
        assert_eq!(interval, 4);
    }

    #[test]
    fn later_success_multiplies_interval_by_ease() {
        let (_, interval, count, _) = sm2_update(2.5, 4, 2, 3);
        assert_eq!(interval, 10); // round(4 * 2.5)
        assert_eq!(count, 3);
    }

    #[test]
    fn lapse_resets_progress_and_lowers_ease() {
        let (ease, interval, count, _) = sm2_update(2.5, 10, 3, 1);
        assert_eq!(interval, 0);
        assert_eq!(count, 0);
        assert!((ease - 2.3).abs() < 1e-9);
    }

    #[test]
    fn ease_never_drops_below_floor() {
        let (ease, _, _, _) = sm2_update(1.35, 10, 3, 1);
        assert!((ease - MIN_EASE).abs() < 1e-9);
    }
}
