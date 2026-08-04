# Changelog

## [0.1.0] — 2026-08-04

### Added
- CLI quick-entry mode via `-m` / `--message` with optional `-d` / `--date`
- `list` subcommand to show recent entries
- `search` subcommand to find entries by keyword
- `summary` subcommand to export entries for a time range (LLM-friendly markdown)
- Fullscreen TUI with date-picker, entry viewer, add/edit/delete/search
- Entries stored as YAML-frontmatter markdown files under `YYYY/MM/DD.md`
- Cross-platform builds (Linux, Windows, macOS Intel, macOS Apple Silicon) via GitHub Actions
