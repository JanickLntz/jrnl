mod entry;
mod store;
mod tui;

use chrono::{Local, NaiveDate};
use clap::{Parser, Subcommand};
use entry::Entry;
use std::process;

#[derive(Parser)]
#[command(name = "jrnl")]
#[command(about = "A terminal-based journal app", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Quick entry: add a new journal entry directly
    #[arg(short = 'm', long = "message")]
    message: Option<String>,

    /// Date for the entry (YYYY-MM-DD), defaults to today
    #[arg(short = 'd', long = "date")]
    date: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// List recent entries
    List {
        /// Number of entries to show
        #[arg(short = 'n', long = "num", default_value = "10")]
        num: usize,
    },
    /// Search entries for a query
    Search {
        /// Search query
        query: String,
    },
    /// Show entries for a time range (for LLM context)
    Summary {
        /// Last N days (e.g. 7d, 30d)
        #[arg(long)]
        last: Option<String>,
        /// Start date (YYYY-MM-DD)
        #[arg(long)]
        from: Option<String>,
        /// End date (YYYY-MM-DD), defaults to today
        #[arg(long)]
        to: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();

    // Quick mode: -m "text" with optional -d and -t
    if let Some(body) = cli.message {
        let entry = if let Some(date_str) = cli.date {
            let date = parse_date(&date_str);
            Entry::with_date(body, date)
        } else {
            Entry::new(body)
        };

        match store::append_entry(&entry) {
            Ok(path) => println!("✓ Entry saved to {}", path.display()),
            Err(e) => {
                eprintln!("Error: {e}");
                process::exit(1);
            }
        }
        return;
    }

    // Subcommands
    match &cli.command {
        Some(Commands::List { num }) => match store::list_recent(*num) {
            Ok(entries) => {
                if entries.is_empty() {
                    println!("No entries found.");
                } else {
                    for entry in &entries {
                        print_entry(entry);
                    }
                }
            }
            Err(e) => {
                eprintln!("Error: {e}");
                process::exit(1);
            }
        },
        Some(Commands::Search { query }) => match store::search(query) {
            Ok(entries) => {
                if entries.is_empty() {
                    println!("No matches for \"{}\"", query);
                } else {
                    println!("Found {} match(es):\n", entries.len());
                    for entry in &entries {
                        print_entry(entry);
                    }
                }
            }
            Err(e) => {
                eprintln!("Error: {e}");
                process::exit(1);
            }
        },
        Some(Commands::Summary { last, from, to }) => {
            let now = Local::now().date_naive();
            let (start, end) = if let Some(ref from_str) = from {
                let s = parse_naive_date(from_str);
                let e = to.as_ref().map(|t| parse_naive_date(t)).unwrap_or(now);
                (s, e)
            } else if let Some(ref to_str) = to {
                // Only --to given: from earliest entry to that date
                let e = parse_naive_date(to_str);
                let s = earliest_date().unwrap_or(e);
                (s, e)
            } else if let Some(ref last_str) = last {
                let days = parse_last_days(last_str);
                (now - chrono::Duration::days(days - 1), now)
            } else {
                (now - chrono::Duration::days(6), now)
            };

            let all_entries = store::read_range(start, end).unwrap_or_else(|e| {
                eprintln!("Error: {e}");
                process::exit(1);
            });
            if all_entries.is_empty() {
                println!(
                    "No entries from {} to {}.",
                    start.format("%Y-%m-%d"),
                    end.format("%Y-%m-%d")
                );
            } else {
                println!(
                    "=== Journal Entries — {} to {} ===\n",
                    start.format("%Y-%m-%d"),
                    end.format("%Y-%m-%d")
                );
                for entry in &all_entries {
                    println!("{}", entry.to_markdown());
                }
            }
        }
        None => {
            // No args: open fullscreen TUI
            if let Err(e) = tui::run() {
                eprintln!("TUI error: {e}");
                process::exit(1);
            }
        }
    }
}

fn parse_naive_date(s: &str) -> NaiveDate {
    parse_date(s).date_naive()
}

pub(crate) fn earliest_date() -> Option<NaiveDate> {
    store::list_dates().ok()?.into_iter().min()
}

/// Parse --last argument like "7d", "30d". Exits with error on invalid input.
fn parse_last_days(s: &str) -> i64 {
    match s.strip_suffix('d').and_then(|n| n.parse::<i64>().ok()) {
        Some(d) if d > 0 => d,
        _ => {
            eprintln!("Error: Invalid --last value '{}'. Expected a positive number followed by 'd' (e.g. 7d, 30d).", s);
            process::exit(1);
        }
    }
}

fn parse_date(s: &str) -> chrono::DateTime<Local> {
    // Try YYYY-MM-DD first
    if let Ok(naive) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return store::naive_to_local_datetime(naive);
    }
    // Try YYYY-MM-DDTHH:MM:SS
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return naive
            .and_local_timezone(Local)
            .single()
            .expect("Invalid local time");
    }
    eprintln!("Warning: Could not parse date '{}', using today.", s);
    Local::now()
}

fn print_entry(entry: &Entry) {
    println!(
        "{}",
        term_colors::dim(&format!("── {}", entry.date.format("%Y-%m-%d %H:%M")))
    );
    for line in entry.body.lines() {
        println!("   {}", line);
    }
    println!();
}

/// Minimal terminal styling without external crate.
mod term_colors {
    pub fn dim(s: &str) -> String {
        // Use ANSI escape code for dim/faint
        format!("\x1b[2m{}\x1b[0m", s)
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Local;

    // ── parse_date ──

    #[test]
    fn test_parse_date_ymd() {
        let dt = parse_date("2025-06-15");
        assert_eq!(dt.format("%Y-%m-%d").to_string(), "2025-06-15");
    }

    #[test]
    fn test_parse_date_iso() {
        let dt = parse_date("2025-06-15T14:30:00");
        assert_eq!(
            dt.format("%Y-%m-%dT%H:%M:%S").to_string(),
            "2025-06-15T14:30:00"
        );
    }

    #[test]
    fn test_parse_date_invalid_falls_back_to_today() {
        let before = Local::now();
        let dt = parse_date("not-a-date");
        let after = Local::now();
        // Falls back to now on invalid input
        assert!(dt >= before);
        assert!(dt <= after);
    }

    #[test]
    fn test_parse_date_empty_falls_back() {
        let before = Local::now();
        let dt = parse_date("");
        let after = Local::now();
        assert!(dt >= before);
        assert!(dt <= after);
    }

    // ── parse_naive_date ──

    #[test]
    fn test_parse_naive_date_valid() {
        let d = parse_naive_date("2025-12-25");
        assert_eq!(d.format("%Y-%m-%d").to_string(), "2025-12-25");
    }

    #[test]
    fn test_parse_naive_date_invalid_falls_back() {
        let today = Local::now().date_naive();
        let d = parse_naive_date("garbage");
        assert_eq!(d, today);
    }

    // ── term_colors ──

    #[test]
    fn test_dim_wraps_with_ansi() {
        let out = term_colors::dim("hello");
        assert!(out.starts_with("\x1b[2m"));
        assert!(out.ends_with("\x1b[0m"));
        assert!(out.contains("hello"));
    }
}
