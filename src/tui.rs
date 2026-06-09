use std::{env, io, process::Command, time::Duration};

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap},
    Frame, Terminal,
};

use crate::model::{Hotspot, ProjectProfile, SortKey};

pub fn run_tui(profile: ProjectProfile, sort: SortKey, limit: usize) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = run_terminal(&mut terminal, profile, sort, limit);
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn run_terminal(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    profile: ProjectProfile,
    sort: SortKey,
    limit: usize,
) -> Result<()> {
    let mut app = TuiApp::new(profile, sort, limit);
    loop {
        terminal.draw(|frame| draw(frame, &mut app))?;
        if event::poll(Duration::from_millis(120))? {
            match event::read()? {
                Event::Key(key) => match app.handle_key(key) {
                    TuiAction::Quit => break,
                    TuiAction::OpenSelected => {
                        if let Some(message) = open_selected(terminal, &app)? {
                            app.status = message;
                        }
                    }
                    TuiAction::Continue => {}
                },
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
    }
    Ok(())
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum TuiAction {
    Continue,
    Quit,
    OpenSelected,
}

struct TuiApp {
    profile: ProjectProfile,
    sort: SortKey,
    limit: usize,
    selected: usize,
    filter: String,
    filter_mode: bool,
    show_help: bool,
    status: String,
}

impl TuiApp {
    fn new(profile: ProjectProfile, sort: SortKey, limit: usize) -> Self {
        Self {
            profile,
            sort,
            limit,
            selected: 0,
            filter: String::new(),
            filter_mode: false,
            show_help: false,
            status: String::new(),
        }
    }

    fn filtered_hotspots(&self) -> Vec<&Hotspot> {
        let filter = self.filter.to_ascii_lowercase();
        let mut hotspots = self
            .profile
            .sorted_hotspots(self.sort, self.profile.hotspots.len());
        if !filter.is_empty() {
            hotspots.retain(|hotspot| {
                hotspot.path.to_ascii_lowercase().contains(&filter)
                    || hotspot.language.to_ascii_lowercase().contains(&filter)
                    || hotspot
                        .reasons
                        .iter()
                        .any(|reason| reason.to_ascii_lowercase().contains(&filter))
            });
        }
        hotspots.truncate(self.limit.min(hotspots.len()));
        hotspots
    }

    fn selected_hotspot(&self) -> Option<&Hotspot> {
        self.filtered_hotspots().get(self.selected).copied()
    }

    fn handle_key(&mut self, key: KeyEvent) -> TuiAction {
        if self.filter_mode {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => self.filter_mode = false,
                KeyCode::Backspace => {
                    self.filter.pop();
                    self.selected = self
                        .selected
                        .min(self.filtered_hotspots().len().saturating_sub(1));
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.filter.clear();
                    self.selected = 0;
                }
                KeyCode::Char(ch) => {
                    self.filter.push(ch);
                    self.selected = self
                        .selected
                        .min(self.filtered_hotspots().len().saturating_sub(1));
                }
                _ => {}
            }
            return TuiAction::Continue;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => TuiAction::Quit,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => TuiAction::Quit,
            KeyCode::Down | KeyCode::Char('j') => {
                let max = self.filtered_hotspots().len().saturating_sub(1);
                self.selected = (self.selected + 1).min(max);
                TuiAction::Continue
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
                TuiAction::Continue
            }
            KeyCode::Enter | KeyCode::Char('o') => TuiAction::OpenSelected,
            KeyCode::Char('/') => {
                self.filter_mode = true;
                TuiAction::Continue
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.filter.clear();
                self.selected = 0;
                TuiAction::Continue
            }
            KeyCode::Char('s') => {
                self.sort = next_sort(self.sort);
                self.selected = 0;
                TuiAction::Continue
            }
            KeyCode::Char('?') => {
                self.show_help = !self.show_help;
                TuiAction::Continue
            }
            _ => TuiAction::Continue,
        }
    }
}

fn draw(frame: &mut Frame<'_>, app: &mut TuiApp) {
    let area = frame.area();
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(area);

    draw_header(frame, app, vertical[0]);
    draw_body(frame, app, vertical[1]);
    draw_footer(frame, app, vertical[2]);
    if app.show_help {
        draw_help(frame, area);
    }
}

fn draw_header(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    let title = format!(
        "profilr - {} files, {} lines, scan {} ms",
        app.profile.total_profiled_files, app.profile.total_lines, app.profile.scan_duration_ms
    );
    let filter = if app.filter.is_empty() {
        "filter: none".to_string()
    } else {
        format!("filter: {}", app.filter)
    };
    let line = Line::from(vec![
        Span::styled(
            title,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::raw(format!("sort: {}", app.sort.label())),
        Span::raw("  "),
        Span::raw(filter),
    ]);
    frame.render_widget(
        Paragraph::new(line).block(Block::default().borders(Borders::ALL)),
        area,
    );
}

fn draw_body(frame: &mut Frame<'_>, app: &mut TuiApp, area: Rect) {
    let chunks = if area.width < 96 {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
            .split(area)
    };
    draw_hotspots(frame, app, chunks[0]);
    draw_details(frame, app, chunks[1]);
}

fn draw_hotspots(frame: &mut Frame<'_>, app: &mut TuiApp, area: Rect) {
    let hotspots = app.filtered_hotspots();
    let rows = hotspots.iter().enumerate().map(|(index, hotspot)| {
        let style = if index == app.selected {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default()
        };
        Row::new(vec![
            Cell::from(hotspot.rank.to_string()),
            Cell::from(format!("{:.0}", hotspot.score)),
            Cell::from(hotspot.language.clone()),
            Cell::from(hotspot.lines.to_string()),
            Cell::from(hotspot.path.clone()),
        ])
        .style(style)
    });

    let table = Table::new(
        rows,
        [
            Constraint::Length(5),
            Constraint::Length(7),
            Constraint::Length(12),
            Constraint::Length(7),
            Constraint::Min(20),
        ],
    )
    .header(
        Row::new(vec!["rank", "score", "language", "lines", "path"]).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(Block::default().title("Hotspots").borders(Borders::ALL));
    frame.render_widget(table, area);
}

fn draw_details(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    let Some(hotspot) = app.selected_hotspot() else {
        frame.render_widget(
            Paragraph::new("No hotspots match the current filter")
                .block(Block::default().title("Details").borders(Borders::ALL)),
            area,
        );
        return;
    };

    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        hotspot.path.clone(),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(format!(
        "{} score {:.1}, {} lines, {} bytes",
        hotspot.language, hotspot.score, hotspot.lines, hotspot.bytes
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(metric_bar("branches", hotspot.branches, 40)));
    lines.push(Line::from(metric_bar("loops", hotspot.loops, 16)));
    lines.push(Line::from(metric_bar("functions", hotspot.functions, 32)));
    lines.push(Line::from(metric_bar("allocs", hotspot.allocations, 24)));
    lines.push(Line::from(metric_bar(
        "blocking I/O",
        hotspot.blocking_io,
        16,
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(format!(
        "reasons: {}",
        hotspot.reasons.join(", ")
    )));
    lines.push(Line::from(format!(
        "async markers: {}  tests: {}  max line: {} chars",
        hotspot.async_markers, hotspot.test_markers, hotspot.max_line_chars
    )));
    lines.push(Line::from(""));
    lines.extend(language_lines(app));

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().title("Details").borders(Borders::ALL))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn language_lines(app: &TuiApp) -> Vec<Line<'static>> {
    let max = app
        .profile
        .languages
        .iter()
        .map(|language| language.score)
        .fold(0.0, f64::max);
    let mut lines = vec![Line::from(Span::styled(
        "Language score",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    ))];
    for language in app.profile.languages.iter().take(6) {
        let filled = if max <= 0.0 {
            0
        } else {
            ((language.score / max) * 18.0).round() as usize
        };
        lines.push(Line::from(format!(
            "{:<12} {:<18} {:>6.0}",
            language.language,
            "#".repeat(filled.max(1)),
            language.score
        )));
    }
    lines
}

fn draw_footer(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    let mode = if app.filter_mode {
        "filter mode: type, Enter/Esc exits, Ctrl-U clears"
    } else if !app.status.is_empty() {
        app.status.as_str()
    } else {
        "q quit  j/k move  / filter  s sort  Enter/o open  ? help"
    };
    frame.render_widget(
        Paragraph::new(mode).block(Block::default().borders(Borders::ALL)),
        area,
    );
}

fn draw_help(frame: &mut Frame<'_>, area: Rect) {
    let width = area.width.min(72);
    let height = area.height.min(13);
    let rect = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    let text = vec![
        Line::from("profilr keys"),
        Line::from(""),
        Line::from("j/down and k/up move through hotspots"),
        Line::from("/ filters by path, language, or reason"),
        Line::from("s rotates sort order"),
        Line::from("Enter or o opens the selected file in $VISUAL or $EDITOR"),
        Line::from("Ctrl-U clears the filter"),
        Line::from("? toggles this panel"),
        Line::from("q, Esc, or Ctrl-C exits"),
    ];
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(text)
            .block(Block::default().title("Help").borders(Borders::ALL))
            .wrap(Wrap { trim: true }),
        rect,
    );
}

fn metric_bar(label: &str, value: usize, cap: usize) -> String {
    let filled = value.min(cap);
    format!("{label:<12} {:<20} {value}", "#".repeat(filled.min(20)))
}

fn next_sort(sort: SortKey) -> SortKey {
    match sort {
        SortKey::Score => SortKey::Complexity,
        SortKey::Complexity => SortKey::Lines,
        SortKey::Lines => SortKey::Size,
        SortKey::Size => SortKey::Language,
        SortKey::Language => SortKey::Path,
        SortKey::Path | SortKey::Time => SortKey::Score,
    }
}

fn open_selected(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &TuiApp,
) -> Result<Option<String>> {
    let Some(hotspot) = app.selected_hotspot() else {
        return Ok(None);
    };
    let path = app.profile.root.join(&hotspot.path);
    let Some((program, args)) = editor_command() else {
        return Ok(Some("set VISUAL or EDITOR to open source files".into()));
    };

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    let status = Command::new(&program).args(args).arg(&path).status();
    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    enable_raw_mode()?;
    terminal.clear()?;

    let message = match status {
        Ok(status) if status.success() => format!("opened {}", path.display()),
        Ok(status) => format!("editor exited with status {status} for {}", path.display()),
        Err(err) => format!("failed to open {}: {err}", path.display()),
    };
    Ok(Some(message))
}

fn editor_command() -> Option<(String, Vec<String>)> {
    let editor = env::var("VISUAL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            env::var("EDITOR")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or_else(|| "vi".into());
    let mut parts = editor.split_whitespace();
    let program = parts.next()?.to_string();
    let args = parts.map(str::to_string).collect();
    Some((program, args))
}
