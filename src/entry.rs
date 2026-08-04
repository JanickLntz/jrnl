use chrono::{DateTime, Local};
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct Entry {
    pub date: DateTime<Local>,
    pub body: String,
}

impl Entry {
    pub fn new(body: String) -> Self {
        Self {
            date: Local::now(),
            body,
        }
    }

    pub fn with_date(body: String, date: DateTime<Local>) -> Self {
        Self { date, body }
    }

    /// Render as markdown with YAML frontmatter.
    pub fn to_markdown(&self) -> String {
        let mut fm = String::new();
        fm.push_str("---\n");
        fm.push_str(&format!(
            "date: \"{}\"\n",
            self.date.format("%Y-%m-%dT%H:%M:%S")
        ));
        fm.push_str("---\n\n");
        fm.push_str(&self.body);
        fm.push('\n');
        fm
    }

    /// Parse markdown with YAML frontmatter back to Entry.
    pub fn from_markdown(md: &str) -> Result<Self, String> {
        let md = md.trim();
        if !md.starts_with("---") {
            return Err("No frontmatter found".into());
        }

        // Find closing ---
        let rest = &md[3..];
        let end = rest.find("\n---").ok_or("Unclosed frontmatter")?;
        let fm_str = &rest[..end];
        let body = rest[end + 4..].trim().to_string();

        #[derive(Deserialize)]
        struct Frontmatter {
            date: String,
        }

        let fm: Frontmatter =
            serde_yaml_ng::from_str(fm_str).map_err(|e| format!("YAML parse error: {e}"))?;

        let naive = chrono::NaiveDateTime::parse_from_str(&fm.date, "%Y-%m-%dT%H:%M:%S")
            .map_err(|e| format!("Date parse error: {e}"))?;
        let date = naive
            .and_local_timezone(Local)
            .single()
            .ok_or_else(|| "Ambiguous local time".to_string())?;

        Ok(Entry { date, body })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_date() -> DateTime<Local> {
        chrono::NaiveDateTime::parse_from_str("2025-06-15T14:30:00", "%Y-%m-%dT%H:%M:%S")
            .unwrap()
            .and_local_timezone(Local)
            .single()
            .unwrap()
    }

    // ── constructors ──

    #[test]
    fn test_new_uses_current_time() {
        let before = Local::now();
        let entry = Entry::new("test".into());
        let after = Local::now();
        assert!(entry.date >= before);
        assert!(entry.date <= after);
        assert_eq!(entry.body, "test");
    }

    #[test]
    fn test_with_date() {
        let dt = fixed_date();
        let entry = Entry::with_date("custom date".into(), dt);
        assert_eq!(entry.date, dt);
        assert_eq!(entry.body, "custom date");
    }

    // ── to_markdown ──

    #[test]
    fn test_roundtrip() {
        let entry = Entry {
            date: fixed_date(),
            body: "Hello world".into(),
        };
        let md = entry.to_markdown();
        let parsed = Entry::from_markdown(&md).unwrap();
        assert_eq!(entry.body, parsed.body);
    }

    #[test]
    fn test_to_markdown_empty_body() {
        let entry = Entry {
            date: fixed_date(),
            body: "".into(),
        };
        let md = entry.to_markdown();
        assert!(md.starts_with("---"));
        assert!(md.contains("date: \"2025-06-15T14:30:00\""));
    }

    #[test]
    fn test_to_markdown_special_chars() {
        let entry = Entry {
            date: fixed_date(),
            body: "# Heading\n* list\n**bold**\nÜmläuts 🎉".into(),
        };
        let md = entry.to_markdown();
        let parsed = Entry::from_markdown(&md).unwrap();
        assert_eq!(parsed.body, entry.body);
    }

    #[test]
    fn test_to_markdown_multiline_body() {
        let entry = Entry {
            date: fixed_date(),
            body: "Line 1\nLine 2\n\nLine 4 after blank".into(),
        };
        let md = entry.to_markdown();
        let parsed = Entry::from_markdown(&md).unwrap();
        assert_eq!(parsed.body, entry.body);
    }

    // ── from_markdown success ──

    #[test]
    fn test_from_markdown_basic() {
        let md = "---\ndate: \"2025-06-15T14:30:00\"\n---\n\nHello World";
        let entry = Entry::from_markdown(md).unwrap();
        assert_eq!(entry.body, "Hello World");
        assert_eq!(entry.date, fixed_date());
    }

    // ── from_markdown errors ──

    #[test]
    fn test_from_markdown_no_frontmatter() {
        let err = Entry::from_markdown("just plain text").unwrap_err();
        assert!(err.contains("No frontmatter"));
    }

    #[test]
    fn test_from_markdown_unclosed_frontmatter() {
        let err = Entry::from_markdown("---\ndate: \"2025-01-01T12:00:00\"\n").unwrap_err();
        assert!(err.contains("Unclosed frontmatter"));
    }

    #[test]
    fn test_from_markdown_missing_date_field() {
        let err = Entry::from_markdown("---\nfoo: bar\n---\n\nbody").unwrap_err();
        assert!(err.contains("YAML parse error"));
    }

    #[test]
    fn test_from_markdown_invalid_date() {
        let err = Entry::from_markdown("---\ndate: \"not-a-date\"\n---\n\nbody").unwrap_err();
        assert!(err.contains("Date parse error"));
    }

    #[test]
    fn test_from_markdown_empty_body() {
        let md = "---\ndate: \"2025-06-15T14:30:00\"\n---\n\n";
        let entry = Entry::from_markdown(md).unwrap();
        assert_eq!(entry.body, "");
    }
}
