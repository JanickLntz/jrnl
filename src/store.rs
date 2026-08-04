use chrono::{DateTime, Local, NaiveDate};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::entry::Entry;
use directories::ProjectDirs;

// ── Path resolution ────────────────────────────────────────────────────────

fn journal_dir() -> PathBuf {
    let proj_dirs =
        ProjectDirs::from("com", "jrnl", "jrnl").expect("Could not determine project directories");
    proj_dirs.data_local_dir().to_path_buf()
}

fn day_path_in(base: &Path, date: &DateTime<Local>) -> PathBuf {
    let mut p = base.to_path_buf();
    p.push(date.format("%Y").to_string());
    p.push(date.format("%m").to_string());
    p.push(date.format("%d.md").to_string());
    p
}

fn day_path(date: &DateTime<Local>) -> PathBuf {
    day_path_in(&journal_dir(), date)
}

// ── Public API ─────────────────────────────────────────────────────────────

/// Append an entry to the journal file for its date.
pub fn append_entry(entry: &Entry) -> Result<PathBuf, String> {
    append_entry_in(&journal_dir(), entry)
}

/// Read all entries from a specific date.
pub fn read_day(date: NaiveDate) -> Result<Vec<Entry>, String> {
    read_day_in(&journal_dir(), date)
}

/// Get the file path for a given date (whether it exists or not).
pub fn day_file_path(date: NaiveDate) -> PathBuf {
    let dt = naive_to_local_datetime(date);
    day_path(&dt)
}

/// Read all entries in a date range (inclusive), without per-day I/O overhead.
pub fn read_range(start: NaiveDate, end: NaiveDate) -> Result<Vec<Entry>, String> {
    walk_and_collect_range(&journal_dir(), start, end)
}

/// List all dates that have at least one entry, sorted newest first.
pub fn list_dates() -> Result<Vec<NaiveDate>, String> {
    list_dates_in(&journal_dir())
}

/// List recent entries across all files.
pub fn list_recent(limit: usize) -> Result<Vec<Entry>, String> {
    list_recent_in(&journal_dir(), limit)
}

/// Search entries for a query string.
pub fn search(query: &str) -> Result<Vec<Entry>, String> {
    search_in(&journal_dir(), query)
}

// ── Helpers ────────────────────────────────────────────────────────────────

/// Convert a date-only NaiveDate to a DateTime<Local> at midnight.
pub(crate) fn naive_to_local_datetime(date: NaiveDate) -> DateTime<Local> {
    date.and_hms_opt(0, 0, 0)
        .unwrap()
        .and_local_timezone(Local)
        .single()
        .unwrap()
}

// ── Internal (base_dir parameterized, testable) ────────────────────────────

pub(crate) fn append_entry_in(base: &Path, entry: &Entry) -> Result<PathBuf, String> {
    let path = day_path_in(base, &entry.date);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Cannot create dirs: {e}"))?;
    }

    let md = entry.to_markdown();
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("Cannot open file: {e}"))?;

    file.write_all(md.as_bytes())
        .map_err(|e| format!("Write error: {e}"))?;

    Ok(path)
}

pub(crate) fn read_day_in(base: &Path, date: NaiveDate) -> Result<Vec<Entry>, String> {
    let dt = naive_to_local_datetime(date);
    let path = day_path_in(base, &dt);
    if !path.exists() {
        return Ok(vec![]);
    }
    let content = fs::read_to_string(&path).map_err(|e| format!("Read error: {e}"))?;
    let (entries, errors) = parse_entries(&content);
    for e in &errors {
        eprintln!("Parse error: {e}");
    }
    Ok(entries)
}

pub(crate) fn list_dates_in(base: &Path) -> Result<Vec<NaiveDate>, String> {
    if !base.exists() {
        return Ok(vec![]);
    }

    let mut dates: Vec<NaiveDate> = Vec::new();
    for year_dir in fs::read_dir(base).map_err(|e| format!("Read dir error: {e}"))? {
        let year_dir = year_dir.map_err(|e| format!("Dir entry error: {e}"))?;
        if !year_dir.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        for month_dir in
            fs::read_dir(year_dir.path()).map_err(|e| format!("Read dir error: {e}"))?
        {
            let month_dir = month_dir.map_err(|e| format!("Dir entry error: {e}"))?;
            if !month_dir.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            for file in
                fs::read_dir(month_dir.path()).map_err(|e| format!("Read dir error: {e}"))?
            {
                let file = file.map_err(|e| format!("Dir entry error: {e}"))?;
                let fname = file.file_name();
                let stem = fname.to_string_lossy();
                if !stem.ends_with(".md") {
                    continue;
                }
                let day_str = &stem[..stem.len() - 3]; // strip .md
                if let Ok(y) = year_dir.file_name().to_string_lossy().parse::<i32>() {
                    if let Ok(m) = month_dir.file_name().to_string_lossy().parse::<u32>() {
                        if let Ok(d) = day_str.parse::<u32>() {
                            if let Some(date) = NaiveDate::from_ymd_opt(y, m, d) {
                                dates.push(date);
                            }
                        }
                    }
                }
            }
        }
    }

    dates.sort_by(|a, b| b.cmp(a)); // newest first
    Ok(dates)
}

pub(crate) fn list_recent_in(base: &Path, limit: usize) -> Result<Vec<Entry>, String> {
    walk_and_collect(base, |entries| {
        entries.sort_by_key(|b| std::cmp::Reverse(b.date));
        entries.truncate(limit);
        Ok(std::mem::take(entries))
    })
}

pub(crate) fn search_in(base: &Path, query: &str) -> Result<Vec<Entry>, String> {
    let q = query.to_lowercase();
    walk_and_collect(base, |entries| {
        let filtered: Vec<Entry> = entries
            .iter()
            .filter(|e| e.body.to_lowercase().contains(&q))
            .cloned()
            .collect();
        Ok(filtered)
    })
}

/// Walk all journal files and collect entries, applying a transform.
fn walk_and_collect<F>(base: &Path, f: F) -> Result<Vec<Entry>, String>
where
    F: Fn(&mut Vec<Entry>) -> Result<Vec<Entry>, String>,
{
    let mut all = walk_all(base)?;
    f(&mut all)
}

/// Walk all journal files, collecting entries (shared implementation).
fn walk_all(base: &Path) -> Result<Vec<Entry>, String> {
    if !base.exists() {
        return Ok(vec![]);
    }

    let mut all: Vec<Entry> = Vec::new();

    for year_dir in fs::read_dir(base).map_err(|e| format!("Read dir error: {e}"))? {
        let year_dir = year_dir.map_err(|e| format!("Dir entry error: {e}"))?;
        if !year_dir.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        for month_dir in
            fs::read_dir(year_dir.path()).map_err(|e| format!("Read dir error: {e}"))?
        {
            let month_dir = month_dir.map_err(|e| format!("Dir entry error: {e}"))?;
            if !month_dir.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            for file in
                fs::read_dir(month_dir.path()).map_err(|e| format!("Read dir error: {e}"))?
            {
                let file = file.map_err(|e| format!("Dir entry error: {e}"))?;
                let path = file.path();
                if path.extension().and_then(|e| e.to_str()) != Some("md") {
                    continue;
                }
                let content = fs::read_to_string(&path).map_err(|e| format!("Read error: {e}"))?;
                let (entries, errors) = parse_entries(&content);
                for e in &errors {
                    eprintln!("Parse error: {e}");
                }
                all.extend(entries);
            }
        }
    }

    Ok(all)
}

/// Walk journal files in a date range (inclusive), without per-day I/O.
fn walk_and_collect_range(
    base: &Path,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<Vec<Entry>, String> {
    let mut all = walk_all(base)?;
    all.retain(|e| e.date.date_naive() >= start && e.date.date_naive() <= end);
    all.sort_by_key(|a| a.date);
    Ok(all)
}

/// Parse multiple entries from raw file content.
/// Returns (valid_entries, parse_errors).
pub(crate) fn parse_entries(content: &str) -> (Vec<Entry>, Vec<String>) {
    let mut entries = Vec::new();
    let mut errors = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let mut in_frontmatter = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "---" {
            if in_frontmatter {
                in_frontmatter = false;
                current.push(line);
            } else {
                if !current.is_empty() {
                    let entry_text = current.join("\n");
                    match Entry::from_markdown(&entry_text) {
                        Ok(entry) => entries.push(entry),
                        Err(e) => errors.push(e),
                    }
                }
                current = Vec::new();
                in_frontmatter = true;
                current.push(line);
            }
        } else {
            current.push(line);
        }
    }

    if !current.is_empty() {
        let entry_text = current.join("\n");
        match Entry::from_markdown(&entry_text) {
            Ok(entry) => entries.push(entry),
            Err(e) => errors.push(e),
        }
    }

    (entries, errors)
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

    /// Helper: create an entry at a fixed date/time
    fn make_entry(date_str: &str, body: &str) -> Entry {
        let dt = chrono::NaiveDateTime::parse_from_str(date_str, "%Y-%m-%dT%H:%M:%S")
            .unwrap()
            .and_local_timezone(Local)
            .single()
            .unwrap();
        Entry {
            date: dt,
            body: body.to_string(),
        }
    }

    /// Each test gets its own unique temp dir — no shared state, fully parallel-safe.
    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let n = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
            let tmp = std::env::temp_dir().join(format!("jrnl_test_{}_{}", std::process::id(), n));
            let _ = std::fs::remove_dir_all(&tmp);
            std::fs::create_dir_all(&tmp).unwrap();
            TestDir(tmp)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // ── append + read ──

    #[test]
    fn test_append_and_read_single() {
        let dir = TestDir::new();
        let entry = make_entry("2025-06-15T14:30:00", "Einkaufen");

        let path = append_entry_in(dir.path(), &entry).unwrap();
        assert!(path.exists());
        assert!(path.to_string_lossy().ends_with("2025/06/15.md"));

        let entries =
            read_day_in(dir.path(), NaiveDate::from_ymd_opt(2025, 6, 15).unwrap()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].body, "Einkaufen");
    }

    #[test]
    fn test_append_multiple_same_day() {
        let dir = TestDir::new();
        append_entry_in(dir.path(), &make_entry("2025-03-01T10:00:00", "Erster")).unwrap();
        append_entry_in(dir.path(), &make_entry("2025-03-01T11:00:00", "Zweiter")).unwrap();

        let entries =
            read_day_in(dir.path(), NaiveDate::from_ymd_opt(2025, 3, 1).unwrap()).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].body, "Erster");
        assert_eq!(entries[1].body, "Zweiter");
    }

    #[test]
    fn test_read_empty_day() {
        let dir = TestDir::new();
        let entries =
            read_day_in(dir.path(), NaiveDate::from_ymd_opt(1999, 1, 1).unwrap()).unwrap();
        assert!(entries.is_empty());
    }

    // ── list_dates ──

    #[test]
    fn test_list_dates_sorted() {
        let dir = TestDir::new();
        append_entry_in(dir.path(), &make_entry("2025-01-03T08:00:00", "A")).unwrap();
        append_entry_in(dir.path(), &make_entry("2025-01-01T08:00:00", "B")).unwrap();
        append_entry_in(dir.path(), &make_entry("2025-01-02T08:00:00", "C")).unwrap();

        let dates = list_dates_in(dir.path()).unwrap();
        assert_eq!(dates.len(), 3);
        assert_eq!(
            dates,
            vec![
                NaiveDate::from_ymd_opt(2025, 1, 3).unwrap(),
                NaiveDate::from_ymd_opt(2025, 1, 2).unwrap(),
                NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
            ]
        );
    }

    // ── list_recent ──

    #[test]
    fn test_list_recent_limit() {
        let dir = TestDir::new();
        for i in 1..=5 {
            append_entry_in(
                dir.path(),
                &make_entry(&format!("2025-02-{:02}T12:00:00", i), &format!("Tag {}", i)),
            )
            .unwrap();
        }

        let recent = list_recent_in(dir.path(), 3).unwrap();
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].body, "Tag 5");
        assert_eq!(recent[1].body, "Tag 4");
        assert_eq!(recent[2].body, "Tag 3");
    }

    // ── search ──

    #[test]
    fn test_search_finds_match() {
        let dir = TestDir::new();
        append_entry_in(
            dir.path(),
            &make_entry("2025-04-01T09:00:00", "Rust ist toll"),
        )
        .unwrap();
        append_entry_in(
            dir.path(),
            &make_entry("2025-04-01T10:00:00", "Python ist okay"),
        )
        .unwrap();

        let results = search_in(dir.path(), "rust").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].body, "Rust ist toll");

        let results = search_in(dir.path(), "ist").unwrap();
        assert_eq!(results.len(), 2);

        let results = search_in(dir.path(), "XYZ").unwrap();
        assert!(results.is_empty());
    }

    // ── parse_entries (unit test, no I/O) ──

    #[test]
    fn test_parse_single_entry() {
        let md = "---\ndate: \"2025-05-10T12:00:00\"\n---\n\nHello World\n";
        let (entries, errors) = parse_entries(md);
        assert!(errors.is_empty());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].body, "Hello World");
    }

    #[test]
    fn test_parse_multiple_entries() {
        let md = "---\ndate: \"2025-05-10T10:00:00\"\n---\n\nEntry 1\n---\ndate: \"2025-05-10T11:00:00\"\n---\n\nEntry 2\n";
        let (entries, errors) = parse_entries(md);
        assert!(errors.is_empty());
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].body, "Entry 1");
        assert_eq!(entries[1].body, "Entry 2");
    }

    #[test]
    fn test_parse_empty_file() {
        let (entries, errors) = parse_entries("");
        assert!(errors.is_empty());
        assert!(entries.is_empty());
    }

    #[test]
    fn test_parse_skips_corrupted_entry() {
        // Garbled entry between two valid ones — should skip the bad one
        let md = concat!(
            "---\ndate: \"2025-01-01T10:00:00\"\n---\n\nFirst\n",
            "---\nbroken: yes\n---\n\nBad\n",
            "---\ndate: \"2025-01-01T12:00:00\"\n---\n\nThird\n",
        );
        let (entries, errors) = parse_entries(md);
        assert_eq!(errors.len(), 1);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].body, "First");
        assert_eq!(entries[1].body, "Third");
    }

    #[test]
    fn test_parse_text_before_first_frontmatter() {
        let md = "garbage before\n---\ndate: \"2025-03-03T09:00:00\"\n---\n\nBody\n";
        let (entries, _errors) = parse_entries(md);
        // The garbage is treated as a broken "entry" before the valid one; valid entry still parsed
        assert!(entries.iter().any(|e| e.body == "Body"));
    }

    // ── empty directory edge cases ──

    #[test]
    fn test_list_dates_empty_dir() {
        let dir = TestDir::new();
        let dates = list_dates_in(dir.path()).unwrap();
        assert!(dates.is_empty());
    }

    #[test]
    fn test_list_recent_empty_dir() {
        let dir = TestDir::new();
        let entries = list_recent_in(dir.path(), 5).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_search_empty_dir() {
        let dir = TestDir::new();
        let results = search_in(dir.path(), "anything").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_read_day_empty_dir() {
        let dir = TestDir::new();
        let entries =
            read_day_in(dir.path(), NaiveDate::from_ymd_opt(2025, 12, 25).unwrap()).unwrap();
        assert!(entries.is_empty());
    }

    // ── day_file_path ──

    #[test]
    fn test_day_file_path_structure() {
        let date = NaiveDate::from_ymd_opt(2025, 4, 7).unwrap();
        let path = day_file_path(date);
        let s = path.to_string_lossy();
        assert!(s.contains("2025"));
        assert!(s.contains("04"));
        assert!(s.ends_with("07.md") || s.contains("04/07.md"));
    }

    // ── search edge cases ──

    #[test]
    fn test_search_unicode() {
        let dir = TestDir::new();
        append_entry_in(
            dir.path(),
            &make_entry("2025-08-01T12:00:00", "Café résumé naïve"),
        )
        .unwrap();

        let results = search_in(dir.path(), "café").unwrap();
        assert_eq!(results.len(), 1);

        let results = search_in(dir.path(), "RÉSUMÉ").unwrap();
        assert_eq!(results.len(), 1);
    }
}
