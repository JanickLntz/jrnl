use chrono::{Local, NaiveDate};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, Clear, List, ListItem, Paragraph, Wrap},
    DefaultTerminal, Frame,
};
use std::io;
use std::time::Duration;

use crate::entry::Entry;
use crate::store;

/// Approximate number of visible rows used for auto-scrolling in each panel.
/// These are conservative estimates; the actual height depends on terminal size
/// and border decoration. Derived from the default 24-row terminal minus UI chrome.
const DATE_VISIBLE_ROWS: usize = 15;
const ENTRY_VISIBLE_ROWS: usize = 12;

// ── App State ──────────────────────────────────────────────────────────────

pub struct App {
    dates: Vec<NaiveDate>,
    selected_date_idx: usize,
    date_scroll: usize,
    entries: Vec<Entry>,
    selected_entry_idx: usize,
    entry_scroll: usize,
    active_panel: usize,
    mode: Mode,
    input_buffer: String,
    last_search: String,
    delete_focus_yes: bool,
}

#[derive(PartialEq, Clone)]
enum Mode {
    Normal,
    Adding,
    Searching,
    Editing(usize),
    ConfirmDelete(usize),
}

impl App {
    fn new() -> Self {
        let dates = store::list_dates().unwrap_or_default();
        let entries = if let Some(&first) = dates.first() {
            store::read_day(first).unwrap_or_default()
        } else {
            vec![]
        };
        App {
            dates,
            selected_date_idx: 0,
            date_scroll: 0,
            entries,
            selected_entry_idx: 0,
            entry_scroll: 0,
            active_panel: 1,
            mode: Mode::Normal,
            input_buffer: String::new(),
            last_search: String::new(),
            delete_focus_yes: true,
        }
    }

    fn selected_date(&self) -> Option<NaiveDate> {
        self.dates.get(self.selected_date_idx).copied()
    }

    fn selected_entry(&self) -> Option<&Entry> {
        self.entries.get(self.selected_entry_idx)
    }

    fn load_entries(&mut self) {
        self.entries = self
            .selected_date()
            .and_then(|d| store::read_day(d).ok())
            .unwrap_or_default();
        self.entry_scroll = 0;
        self.selected_entry_idx = 0;
    }

    fn refresh_dates(&mut self) {
        self.dates = store::list_dates().unwrap_or_default();
        if self.selected_date_idx >= self.dates.len() {
            self.selected_date_idx = self.dates.len().saturating_sub(1);
        }
        self.load_entries();
    }

    fn entries_line_counts(&self) -> Vec<usize> {
        self.entries
            .iter()
            .map(|e| {
                let mut n = 1; // date line
                n += e.body.lines().count();
                n += 1; // blank
                n
            })
            .collect()
    }

    fn selected_entry_line_range(&self) -> Option<(usize, usize)> {
        if self.entries.is_empty() {
            return None;
        }
        let counts = self.entries_line_counts();
        let start: usize = counts.iter().take(self.selected_entry_idx).sum();
        let end = start + counts[self.selected_entry_idx];
        Some((start, end))
    }
}

// ── Entry Point ────────────────────────────────────────────────────────────

pub fn run() -> io::Result<()> {
    let mut terminal = ratatui::init();
    let mut app = App::new();
    let result = run_loop(&mut terminal, &mut app);
    ratatui::restore();
    result
}

fn run_loop(terminal: &mut DefaultTerminal, app: &mut App) -> io::Result<()> {
    loop {
        terminal.draw(|frame| draw(frame, app))?;
        if event::poll(Duration::from_millis(100))? {
            let event = event::read()?;
            if handle_event(event, app)? {
                return Ok(());
            }
        }
    }
}

// ── Event Handler ──────────────────────────────────────────────────────────

fn handle_event(event: Event, app: &mut App) -> io::Result<bool> {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => match &app.mode.clone() {
            Mode::Normal => handle_normal_key(key, app),
            Mode::ConfirmDelete(_) => handle_confirm_delete(key, app),
            Mode::Adding => handle_input_key(key, app, false),
            Mode::Editing(_) => handle_input_key(key, app, true),
            Mode::Searching => handle_search_key(key, app),
        },
        Event::Resize(_, _) => Ok(false),
        _ => Ok(false),
    }
}

fn handle_normal_key(key: KeyEvent, app: &mut App) -> io::Result<bool> {
    match key.code {
        KeyCode::Char('q') => return Ok(true),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),
        KeyCode::Esc => {}
        KeyCode::Down => move_cursor(app, 1),
        KeyCode::Up => move_cursor(app, -1),
        KeyCode::Tab => app.active_panel = (app.active_panel + 1) % 2,
        KeyCode::Left => app.active_panel = 0,
        KeyCode::Right => app.active_panel = 1,
        KeyCode::Char('a') => {
            app.mode = Mode::Adding;
            app.input_buffer = String::new();
        }
        KeyCode::Char('e') => {
            if let Some(idx) = app.selected_entry().map(|_| app.selected_entry_idx) {
                app.mode = Mode::Editing(idx);
                app.input_buffer = app.entries[idx].body.clone();
            }
        }
        KeyCode::Char('d') => {
            if let Some(idx) = app.selected_entry().map(|_| app.selected_entry_idx) {
                app.mode = Mode::ConfirmDelete(idx);
                app.delete_focus_yes = true;
            }
        }
        KeyCode::Char('s') | KeyCode::Char('/') => {
            app.mode = Mode::Searching;
            app.input_buffer = String::new();
            app.entries.clear();
        }
        _ => {}
    }
    Ok(false)
}

fn move_cursor(app: &mut App, delta: i32) {
    if app.active_panel == 0 {
        // Date panel
        let new = app.selected_date_idx as i32 + delta;
        if new >= 0 && (new as usize) < app.dates.len() {
            app.selected_date_idx = new as usize;
            app.load_entries();
            // Auto-scroll date list to keep selection visible
            if app.selected_date_idx < app.date_scroll {
                app.date_scroll = app.selected_date_idx;
            }
            if app.selected_date_idx >= app.date_scroll + DATE_VISIBLE_ROWS {
                app.date_scroll = app.selected_date_idx.saturating_sub(DATE_VISIBLE_ROWS - 1);
            }
        }
    } else {
        // Entry panel
        if app.entries.is_empty() {
            return;
        }
        let new = app.selected_entry_idx as i32 + delta;
        if new >= 0 && (new as usize) < app.entries.len() {
            app.selected_entry_idx = new as usize;
            // Auto-scroll entry view
            if let Some((start, end)) = app.selected_entry_line_range() {
                if end > app.entry_scroll + ENTRY_VISIBLE_ROWS {
                    app.entry_scroll = end.saturating_sub(ENTRY_VISIBLE_ROWS);
                }
                if start < app.entry_scroll {
                    app.entry_scroll = start;
                }
            }
        }
    }
}

fn handle_confirm_delete(key: KeyEvent, app: &mut App) -> io::Result<bool> {
    let idx = match app.mode {
        Mode::ConfirmDelete(i) => i,
        _ => return Ok(false),
    };
    match key.code {
        KeyCode::Left | KeyCode::Right => {
            app.delete_focus_yes = !app.delete_focus_yes;
        }
        KeyCode::Char('y') => {
            app.mode = Mode::Normal;
            delete_single_entry(app, idx);
        }
        KeyCode::Char('n') | KeyCode::Esc => {
            app.mode = Mode::Normal;
        }
        KeyCode::Enter => {
            app.mode = Mode::Normal;
            if app.delete_focus_yes {
                delete_single_entry(app, idx);
            }
        }
        _ => {}
    }
    Ok(false)
}

fn delete_single_entry(app: &mut App, idx: usize) {
    if let Some(date) = app.selected_date() {
        app.entries.remove(idx);
        if app.entries.is_empty() {
            let p = store::day_file_path(date);
            let _ = std::fs::remove_file(&p);
        } else {
            rewrite_day(date, &app.entries);
        }
        app.refresh_dates();
    }
}

fn handle_input_key(key: KeyEvent, app: &mut App, is_edit: bool) -> io::Result<bool> {
    match key.code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.input_buffer.clear();
        }
        KeyCode::Enter => {
            let last_line_empty = app.input_buffer.ends_with('\n') || app.input_buffer.is_empty();
            if last_line_empty {
                let body = app.input_buffer.trim().to_string();
                app.mode = Mode::Normal;
                app.input_buffer.clear();
                if body.is_empty() {
                    return Ok(false);
                }
                if is_edit {
                    let idx = match app.mode {
                        Mode::Editing(i) => i,
                        _ => 0,
                    };
                    if let Some(date) = app.selected_date() {
                        if idx < app.entries.len() {
                            app.entries[idx].body = body;
                            rewrite_day(date, &app.entries);
                            app.refresh_dates();
                            app.selected_entry_idx = idx.min(app.entries.len().saturating_sub(1));
                        }
                    }
                } else {
                    let entry = Entry::new(body);
                    if store::append_entry(&entry).is_ok() {
                        app.refresh_dates();
                    }
                }
            } else {
                app.input_buffer.push('\n');
            }
        }
        KeyCode::Backspace => {
            app.input_buffer.pop();
        }
        KeyCode::Char(c) => {
            app.input_buffer.push(c);
        }
        _ => {}
    }
    Ok(false)
}

fn handle_search_key(key: KeyEvent, app: &mut App) -> io::Result<bool> {
    match key.code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.input_buffer.clear();
            app.last_search.clear();
            app.load_entries();
            app.entry_scroll = 0;
        }
        KeyCode::Enter => {
            app.mode = Mode::Normal;
            let q = std::mem::take(&mut app.input_buffer);
            app.last_search = q.clone();
            if !q.is_empty() {
                if let Ok(results) = store::search(&q) {
                    app.entries = results;
                    app.selected_entry_idx = 0;
                    app.entry_scroll = 0;
                }
            } else {
                app.load_entries();
            }
        }
        KeyCode::Char(c) => {
            app.input_buffer.push(c);
            app.last_search = app.input_buffer.clone();
            let q: String = app.input_buffer.clone();
            if !q.is_empty() {
                if let Ok(results) = store::search(&q) {
                    app.entries = results;
                    app.selected_entry_idx = 0;
                    app.entry_scroll = 0;
                }
            }
        }
        KeyCode::Backspace => {
            app.input_buffer.pop();
            app.last_search = app.input_buffer.clone();
            if app.input_buffer.is_empty() {
                app.load_entries();
                app.entry_scroll = 0;
            } else if !app.input_buffer.is_empty() {
                if let Ok(results) = store::search(&app.input_buffer) {
                    app.entries = results;
                    app.selected_entry_idx = 0;
                    app.entry_scroll = 0;
                }
            }
        }
        _ => {}
    }
    Ok(false)
}

// ── File helpers ───────────────────────────────────────────────────────────

fn rewrite_day(date: NaiveDate, entries: &[Entry]) {
    let p = store::day_file_path(date);
    let mut content = String::new();
    for entry in entries {
        content.push_str(&entry.to_markdown());
    }
    let _ = std::fs::write(&p, content);
}

// ── Popup helpers ──────────────────────────────────────────────────────────

/// Create a centered rectangle of given dimensions (percentage of parent).
fn centered_rect(percent_x: u16, height: u16, r: Rect) -> Rect {
    let w = r.width * percent_x / 100;
    let h = height.min(r.height);
    let x = r.x + (r.width.saturating_sub(w)) / 2;
    let y = r.y + (r.height.saturating_sub(h)) / 2;
    Rect {
        x,
        y,
        width: w,
        height: h,
    }
}

// ── Render ─────────────────────────────────────────────────────────────────

fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // ── Main layout ──
    let vertical = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]);
    let [title_area, main_area] = vertical.areas(area);

    let horizontal = Layout::horizontal([Constraint::Length(24), Constraint::Fill(1)]);
    let [date_area, entry_area] = horizontal.areas(main_area);

    // ── Title ──
    let today = Local::now().format("%Y-%m-%d").to_string();
    let mode_str = match &app.mode {
        Mode::Adding => Span::styled(" ADDING", Style::new().green().bold()),
        Mode::Editing(_) => Span::styled(" EDITING", Style::new().yellow().bold()),
        Mode::Searching => Span::styled(" SEARCHING", Style::new().magenta().bold()),
        Mode::ConfirmDelete(_) => Span::styled(" DELETING", Style::new().red().bold()),
        Mode::Normal => Span::raw(""),
    };
    let mut title_spans: Vec<Span> = vec![
        Span::styled(" Journal — ", Style::new().bold()),
        Span::styled(today, Style::new().cyan()),
        mode_str,
    ];
    // Show search query in title bar
    if matches!(app.mode, Mode::Searching) && !app.input_buffer.is_empty() {
        title_spans.push(Span::raw(" \""));
        title_spans.push(Span::styled(&app.input_buffer, Style::new().yellow()));
        title_spans.push(Span::raw("\""));
    }
    let title_left = Line::from(title_spans);
    let hints_right = Line::from(vec![
        " a:Add ".into(),
        "e:Edit ".dim(),
        "d:Delete ".dim(),
        "s:Search ".dim(),
        "↑↓:Nav ".dim(),
        "q:Quit ".dim(),
    ]);
    frame.render_widget(Paragraph::new(title_left), title_area);
    frame.render_widget(Paragraph::new(hints_right).right_aligned(), title_area);

    // ── Dates ──
    let date_block = if app.active_panel == 0 {
        Block::bordered()
            .title(" Dates ")
            .border_style(Style::new().fg(Color::Cyan))
    } else {
        Block::bordered().title(" Dates ")
    };
    // Compute visible date range based on scroll offset
    let date_inner = date_block.inner(date_area);
    let visible_dates = date_inner.height.saturating_sub(2) as usize; // minus borders
    let end_idx = (app.date_scroll + visible_dates).min(app.dates.len());
    let visible_slice = &app.dates[app.date_scroll..end_idx];
    let date_items: Vec<ListItem> = visible_slice
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let global_idx = app.date_scroll + i;
            let text = format!("  {}", d.format("%Y-%m-%d"));
            if global_idx == app.selected_date_idx {
                ListItem::from(text).style(Style::new().bg(Color::Rgb(40, 40, 50)))
            } else {
                ListItem::from(text)
            }
        })
        .collect();
    frame.render_widget(List::new(date_items).block(date_block), date_area);

    // ── Entries ──
    let entry_block = if app.active_panel == 1 {
        Block::bordered()
            .title(format!(" Entries ({}) ", app.entries.len()))
            .border_style(Style::new().fg(Color::Cyan))
    } else {
        Block::bordered().title(format!(" Entries ({}) ", app.entries.len()))
    };
    let entry_text = build_entry_text(app);
    let entry_paragraph = Paragraph::new(entry_text)
        .block(entry_block)
        .wrap(Wrap { trim: false })
        .scroll((app.entry_scroll as u16, 0));
    frame.render_widget(entry_paragraph, entry_area);

    // ── Overlays ──
    match &app.mode {
        Mode::Adding => render_input_popup(frame, area, app, "New Entry"),
        Mode::Editing(_) => render_input_popup(frame, area, app, "Edit Entry"),
        Mode::ConfirmDelete(idx) => render_delete_popup(frame, area, app, *idx),
        _ => {}
    }
}

fn render_input_popup(frame: &mut Frame, area: Rect, app: &App, title: &str) {
    // Popup: 70% width, dynamic height
    let n_lines = app.input_buffer.lines().count().max(1);
    let popup_h = (n_lines as u16 + 4).min(area.height.saturating_sub(4));
    let popup = centered_rect(70, popup_h, area);

    frame.render_widget(Clear, popup);

    let block = Block::bordered()
        .title(format!(" {} ", title))
        .border_style(Style::new().fg(Color::Cyan));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    // Padding inside: 1 line top/bottom padding
    let padded = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .split(inner);
    let text_rect = Layout::horizontal([
        Constraint::Length(2),
        Constraint::Fill(1),
        Constraint::Length(2),
    ])
    .split(padded[1])[1];
    let hint_rect = padded[2];

    // Show input buffer
    let text = if app.input_buffer.is_empty() {
        Text::from(Line::from("Start typing…").dim())
    } else {
        let lines: Vec<Line> = app
            .input_buffer
            .lines()
            .map(|l| Line::from(l.to_string()))
            .collect();
        Text::from(lines)
    };
    let scroll = n_lines.saturating_sub(text_rect.height as usize) as u16;
    frame.render_widget(Paragraph::new(text).scroll((scroll, 0)), text_rect);

    // Hints (centered in the padded bottom area)
    frame.render_widget(
        Paragraph::new(" Enter:newline  Enter+Enter:save  Esc:cancel ")
            .dim()
            .centered(),
        hint_rect,
    );
}

fn render_delete_popup(frame: &mut Frame, area: Rect, app: &App, idx: usize) {
    let popup = centered_rect(50, 10, area);
    frame.render_widget(Clear, popup);

    let block = Block::bordered()
        .title(" ⚠ Delete Entry ")
        .border_style(Style::new().fg(Color::Red));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    // Padding: 1 top, fill middle, hint bar at bottom
    let padded = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .split(inner);
    let content_rect = Layout::horizontal([
        Constraint::Length(2),
        Constraint::Fill(1),
        Constraint::Length(2),
    ])
    .split(padded[1])[1];
    let hint_rect = Layout::horizontal([
        Constraint::Length(2),
        Constraint::Fill(1),
        Constraint::Length(2),
    ])
    .split(padded[2])[1];

    let entry = &app.entries[idx];
    let preview: String = entry.body.chars().take(100).collect();
    let display_body = if preview.len() >= 100 {
        format!("{}…", preview)
    } else {
        preview
    };

    let mut text_lines: Vec<Line> = vec![
        Line::from(vec![entry.date.format("%Y-%m-%d %H:%M").to_string().bold()]),
        Line::from(""),
        Line::from(display_body).italic().dim(),
        Line::from(""),
    ];

    let yes_style = if app.delete_focus_yes {
        Style::new().white().on_red()
    } else {
        Style::new().white().dim()
    };
    let no_style = if !app.delete_focus_yes {
        Style::new().white().on_red()
    } else {
        Style::new().white().dim()
    };

    text_lines.push(Line::from(vec![
        Span::styled(" Yes ", yes_style),
        "    ".into(),
        Span::styled(" No ", no_style),
    ]));

    frame.render_widget(
        Paragraph::new(Text::from(text_lines)).centered(),
        content_rect,
    );
    frame.render_widget(
        Paragraph::new("←/→ pick  Enter confirm  Esc cancel")
            .dim()
            .centered(),
        hint_rect,
    );
}

/// Highlight occurrences of `query_lower` in `text`.
/// The caller pre-computes `text_lower` once per entry body to avoid repeated
/// `to_lowercase()` allocations on every line.
fn highlight_line(
    text: &str,
    text_lower: &str,
    base_style: Style,
    query_lower: &str,
) -> Line<'static> {
    let mut spans: Vec<Span> = Vec::new();
    let mut last = 0;
    let match_style = base_style
        .fg(Color::Yellow)
        .add_modifier(ratatui::style::Modifier::BOLD);

    for (idx, _) in text_lower.match_indices(query_lower) {
        if idx > last {
            spans.push(Span::styled(text[last..idx].to_string(), base_style));
        }
        spans.push(Span::styled(
            text[idx..idx + query_lower.len()].to_string(),
            match_style,
        ));
        last = idx + query_lower.len();
    }
    if last < text.len() {
        spans.push(Span::styled(text[last..].to_string(), base_style));
    }
    Line::from(spans)
}

fn build_entry_text(app: &App) -> Text<'static> {
    let highlight = if !app.last_search.is_empty() {
        Some(app.last_search.to_lowercase())
    } else {
        None
    };

    if app.entries.is_empty() {
        if matches!(app.mode, Mode::Searching) {
            Text::from(vec![Line::from(""), Line::from("  Type to search…").dim()])
        } else {
            Text::from(vec![
                Line::from(""),
                Line::from("  No entries for this date.").dim(),
                Line::from(""),
                Line::from("  Press 'a' to add one.").dim(),
            ])
        }
    } else {
        let selected = Style::new().bg(Color::Rgb(50, 50, 40));
        let normal = Style::new();
        let mut lines: Vec<Line> = vec![];

        for (ei, entry) in app.entries.iter().enumerate() {
            let is_selected = ei == app.selected_entry_idx;
            let style = if is_selected { selected } else { normal };

            lines.push(Line::styled(
                format!(
                    "── {}  {}/{}",
                    entry.date.format("%H:%M"),
                    ei + 1,
                    app.entries.len()
                ),
                style,
            ));

            if let Some(ref q) = highlight {
                // Pre-compute lowercased body once per entry, then highlight
                // each line using the shared lowercase buffer for O(1) allocs.
                let body_lower = entry.body.to_lowercase();
                let mut byte_offset = 0usize;
                for body_line in entry.body.lines() {
                    let line_len = body_line.len();
                    let lower_slice = &body_lower[byte_offset..byte_offset + line_len];
                    lines.push(highlight_line(
                        &format!("   {}", body_line),
                        &format!("   {}", lower_slice),
                        style,
                        q,
                    ));
                    byte_offset += line_len + 1; // +1 for the newline char
                }
            } else {
                for body_line in entry.body.lines() {
                    lines.push(Line::styled(format!("   {}", body_line), style));
                }
            }
            lines.push(Line::styled("", style));
        }
        Text::from(lines)
    }
}
