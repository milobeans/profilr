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
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Tabs, Wrap},
    Frame, Terminal,
};

use crate::{
    model::{DirectorySummary, Hotspot, ProjectProfile, SortKey, WorkloadProfile},
    workload::benchmark_single_workload,
};

pub fn run_tui(
    profile: ProjectProfile,
    sort: SortKey,
    limit: usize,
    iterations: usize,
    warmups: usize,
) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = run_terminal(&mut terminal, profile, sort, limit, iterations, warmups);
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
    iterations: usize,
    warmups: usize,
) -> Result<()> {
    let mut app = TuiApp::new(profile, sort, limit, iterations, warmups);
    loop {
        terminal.draw(|frame| draw(frame, &app))?;
        if event::poll(Duration::from_millis(120))? {
            match event::read()? {
                Event::Key(key) => match app.handle_key(key) {
                    TuiAction::Quit => break,
                    TuiAction::OpenSelected => {
                        if let Some(message) = open_selected(terminal, &app)? {
                            app.status = message;
                        }
                    }
                    TuiAction::RunWorkload => {
                        if let Some(message) = run_selected_workload(terminal, &mut app)? {
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
    RunWorkload,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum TabKind {
    Overview,
    Hotspots,
    Directories,
    Workloads,
}

impl TabKind {
    fn title(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Hotspots => "Hotspots",
            Self::Directories => "Directories",
            Self::Workloads => "Workloads",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Overview => Self::Hotspots,
            Self::Hotspots => Self::Directories,
            Self::Directories => Self::Workloads,
            Self::Workloads => Self::Overview,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Overview => Self::Workloads,
            Self::Hotspots => Self::Overview,
            Self::Directories => Self::Hotspots,
            Self::Workloads => Self::Directories,
        }
    }
}

struct TuiApp {
    profile: ProjectProfile,
    sort: SortKey,
    limit: usize,
    filter: String,
    filter_mode: bool,
    show_help: bool,
    status: String,
    tab: TabKind,
    hotspot_index: usize,
    directory_index: usize,
    workload_index: usize,
    iterations: usize,
    warmups: usize,
}

impl TuiApp {
    fn new(
        profile: ProjectProfile,
        sort: SortKey,
        limit: usize,
        iterations: usize,
        warmups: usize,
    ) -> Self {
        Self {
            profile,
            sort,
            limit,
            filter: String::new(),
            filter_mode: false,
            show_help: false,
            status: String::new(),
            tab: TabKind::Overview,
            hotspot_index: 0,
            directory_index: 0,
            workload_index: 0,
            iterations,
            warmups,
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

    fn filtered_directories(&self) -> Vec<&DirectorySummary> {
        let filter = self.filter.to_ascii_lowercase();
        let mut directories = self
            .profile
            .sorted_directories(self.profile.directories.len());
        if !filter.is_empty() {
            directories.retain(|directory| {
                directory.path.to_ascii_lowercase().contains(&filter)
                    || directory
                        .dominant_language
                        .as_ref()
                        .is_some_and(|language| language.to_ascii_lowercase().contains(&filter))
            });
        }
        directories.truncate(self.limit.min(directories.len()));
        directories
    }

    fn filtered_workloads(&self) -> Vec<&WorkloadProfile> {
        let filter = self.filter.to_ascii_lowercase();
        let mut workloads: Vec<&WorkloadProfile> = self.profile.workloads.iter().collect();
        if !filter.is_empty() {
            workloads.retain(|workload| {
                workload.spec.name.to_ascii_lowercase().contains(&filter)
                    || workload.spec.kind.to_ascii_lowercase().contains(&filter)
                    || workload
                        .spec
                        .command
                        .join(" ")
                        .to_ascii_lowercase()
                        .contains(&filter)
            });
        }
        workloads.truncate(self.limit.min(workloads.len()));
        workloads
    }

    fn selected_hotspot(&self) -> Option<&Hotspot> {
        self.filtered_hotspots().get(self.hotspot_index).copied()
    }

    fn selected_directory(&self) -> Option<&DirectorySummary> {
        self.filtered_directories()
            .get(self.directory_index)
            .copied()
    }

    fn selected_workload(&self) -> Option<&WorkloadProfile> {
        self.filtered_workloads().get(self.workload_index).copied()
    }

    fn selected_workload_mut(&mut self) -> Option<&mut WorkloadProfile> {
        let name = self.selected_workload()?.spec.name.clone();
        self.profile
            .workloads
            .iter_mut()
            .find(|workload| workload.spec.name == name)
    }

    fn handle_key(&mut self, key: KeyEvent) -> TuiAction {
        if self.filter_mode {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => self.filter_mode = false,
                KeyCode::Backspace => {
                    self.filter.pop();
                    self.clamp_selection();
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.filter.clear();
                    self.reset_selection();
                }
                KeyCode::Char(ch) => {
                    self.filter.push(ch);
                    self.clamp_selection();
                }
                _ => {}
            }
            return TuiAction::Continue;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => TuiAction::Quit,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => TuiAction::Quit,
            KeyCode::Char('1') => {
                self.tab = TabKind::Overview;
                TuiAction::Continue
            }
            KeyCode::Char('2') => {
                self.tab = TabKind::Hotspots;
                TuiAction::Continue
            }
            KeyCode::Char('3') => {
                self.tab = TabKind::Directories;
                TuiAction::Continue
            }
            KeyCode::Char('4') => {
                self.tab = TabKind::Workloads;
                TuiAction::Continue
            }
            KeyCode::Tab => {
                self.tab = self.tab.next();
                TuiAction::Continue
            }
            KeyCode::BackTab => {
                self.tab = self.tab.previous();
                TuiAction::Continue
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_selection(1);
                TuiAction::Continue
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_selection(-1);
                TuiAction::Continue
            }
            KeyCode::Enter | KeyCode::Char('o') => match self.tab {
                TabKind::Hotspots | TabKind::Directories => TuiAction::OpenSelected,
                TabKind::Workloads => TuiAction::RunWorkload,
                TabKind::Overview => TuiAction::Continue,
            },
            KeyCode::Char('r') if self.tab == TabKind::Workloads => TuiAction::RunWorkload,
            KeyCode::Char('/') => {
                self.filter_mode = true;
                TuiAction::Continue
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.filter.clear();
                self.reset_selection();
                TuiAction::Continue
            }
            KeyCode::Char('s') if self.tab == TabKind::Hotspots => {
                self.sort = next_sort(self.sort);
                self.hotspot_index = 0;
                TuiAction::Continue
            }
            KeyCode::Char('?') => {
                self.show_help = !self.show_help;
                TuiAction::Continue
            }
            _ => TuiAction::Continue,
        }
    }

    fn move_selection(&mut self, delta: isize) {
        match self.tab {
            TabKind::Overview => {}
            TabKind::Hotspots => {
                let len = self.filtered_hotspots().len();
                adjust_index(&mut self.hotspot_index, len, delta);
            }
            TabKind::Directories => {
                let len = self.filtered_directories().len();
                adjust_index(&mut self.directory_index, len, delta);
            }
            TabKind::Workloads => {
                let len = self.filtered_workloads().len();
                adjust_index(&mut self.workload_index, len, delta);
            }
        }
    }

    fn clamp_selection(&mut self) {
        let hotspot_max = self.filtered_hotspots().len().saturating_sub(1);
        let directory_max = self.filtered_directories().len().saturating_sub(1);
        let workload_max = self.filtered_workloads().len().saturating_sub(1);
        self.hotspot_index = self.hotspot_index.min(hotspot_max);
        self.directory_index = self.directory_index.min(directory_max);
        self.workload_index = self.workload_index.min(workload_max);
    }

    fn reset_selection(&mut self) {
        self.hotspot_index = 0;
        self.directory_index = 0;
        self.workload_index = 0;
    }
}

fn draw(frame: &mut Frame<'_>, app: &TuiApp) {
    let area = frame.area();
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(area);

    draw_header(frame, app, vertical[0]);
    draw_tabs(frame, app, vertical[1]);
    draw_body(frame, app, vertical[2]);
    draw_footer(frame, app, vertical[3]);
    if app.show_help {
        draw_help(frame, area);
    }
}

fn draw_header(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    let title = if area.width < 96 {
        format!(
            "profilr  {}f {}l {}ms  wl {}/{}",
            app.profile.total_profiled_files,
            app.profile.total_lines,
            app.profile.scan_duration_ms,
            app.profile.benchmarked_workloads(),
            app.profile.workloads.len()
        )
    } else {
        format!(
            "profilr - {} files, {} lines, scan {} ms  workloads: {}/{}",
            app.profile.total_profiled_files,
            app.profile.total_lines,
            app.profile.scan_duration_ms,
            app.profile.benchmarked_workloads(),
            app.profile.workloads.len()
        )
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
    ]);
    frame.render_widget(
        Paragraph::new(line).block(Block::default().borders(Borders::ALL)),
        area,
    );
}

fn draw_tabs(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    let titles = [
        TabKind::Overview,
        TabKind::Hotspots,
        TabKind::Directories,
        TabKind::Workloads,
    ]
    .into_iter()
    .map(|tab| Line::from(tab.title()))
    .collect::<Vec<_>>();
    let selected = match app.tab {
        TabKind::Overview => 0,
        TabKind::Hotspots => 1,
        TabKind::Directories => 2,
        TabKind::Workloads => 3,
    };
    let tabs = Tabs::new(titles)
        .select(selected)
        .block(Block::default().borders(Borders::ALL))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_widget(tabs, area);
}

fn draw_body(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    let chunks = if area.width < 110 {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(54), Constraint::Percentage(46)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
            .split(area)
    };

    match app.tab {
        TabKind::Overview => {
            draw_overview(frame, app, chunks[0]);
            draw_overview_details(frame, app, chunks[1]);
        }
        TabKind::Hotspots => {
            draw_hotspots(frame, app, chunks[0]);
            draw_hotspot_details(frame, app, chunks[1]);
        }
        TabKind::Directories => {
            draw_directories(frame, app, chunks[0]);
            draw_directory_details(frame, app, chunks[1]);
        }
        TabKind::Workloads => {
            draw_workloads(frame, app, chunks[0]);
            draw_workload_details(frame, app, chunks[1]);
        }
    }
}

fn draw_overview(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    let max_language = app
        .profile
        .languages
        .iter()
        .map(|language| language.score)
        .fold(0.0, f64::max);
    let max_directory = app
        .profile
        .directories
        .iter()
        .map(|directory| directory.score)
        .fold(0.0, f64::max);

    let mut lines = vec![
        Line::from(format!("root: {}", app.profile.root.display())),
        Line::from(format!(
            "detected projects: {}",
            if app.profile.detected_projects.is_empty() {
                "none".into()
            } else {
                app.profile
                    .detected_projects
                    .iter()
                    .map(|project| project.kind.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        )),
        Line::from(format!(
            "hotspots: {}  directories: {}  workloads: {}",
            app.profile.hotspots.len(),
            app.profile.directories.len(),
            app.profile.workloads.len()
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Language Score",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
    ];
    for language in app.profile.languages.iter().take(6) {
        lines.push(Line::from(score_bar(
            &language.language,
            language.score,
            max_language,
            18,
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Directory Score",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )));
    for directory in app.profile.directories.iter().take(6) {
        lines.push(Line::from(score_bar(
            &directory.path,
            directory.score,
            max_directory,
            18,
        )));
    }

    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().title("Overview").borders(Borders::ALL))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_overview_details(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    let mut lines = vec![Line::from(Span::styled(
        "Recommended Workloads",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    ))];
    if app.profile.workloads.is_empty() {
        lines.push(Line::from("No workloads detected"));
    } else {
        for workload in app.profile.workloads.iter().take(8) {
            let status = workload
                .result
                .as_ref()
                .map(|result| format!("{:.2} ms mean", result.stats.mean_ms))
                .unwrap_or_else(|| workload.status.clone());
            lines.push(Line::from(format!(
                "{:<18} {:<8} {}",
                workload.spec.name, workload.spec.kind, status
            )));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Top Hotspots",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )));
    for hotspot in app.profile.sorted_hotspots(app.sort, 6) {
        lines.push(Line::from(format!(
            "{:>6.1} {:<12} {}",
            hotspot.score, hotspot.language, hotspot.path
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().title("Insights").borders(Borders::ALL))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_hotspots(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    let hotspots = app.filtered_hotspots();
    let rows = hotspots.iter().enumerate().map(|(index, hotspot)| {
        let style = if index == app.hotspot_index {
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

fn draw_hotspot_details(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    let Some(hotspot) = app.selected_hotspot() else {
        frame.render_widget(
            Paragraph::new("No hotspots match the current filter")
                .block(Block::default().title("Details").borders(Borders::ALL)),
            area,
        );
        return;
    };

    let lines = vec![
        Line::from(Span::styled(
            hotspot.path.clone(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(format!(
            "{} score {:.1}, {} lines, {} bytes",
            hotspot.language, hotspot.score, hotspot.lines, hotspot.bytes
        )),
        Line::from(""),
        Line::from(metric_bar("branches", hotspot.branches, 40)),
        Line::from(metric_bar("loops", hotspot.loops, 16)),
        Line::from(metric_bar("functions", hotspot.functions, 32)),
        Line::from(metric_bar("allocs", hotspot.allocations, 24)),
        Line::from(metric_bar("blocking I/O", hotspot.blocking_io, 16)),
        Line::from(""),
        Line::from(format!("reasons: {}", hotspot.reasons.join(", "))),
        Line::from(format!(
            "async markers: {}  tests: {}  max line: {} chars",
            hotspot.async_markers, hotspot.test_markers, hotspot.max_line_chars
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().title("Details").borders(Borders::ALL))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_directories(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    let directories = app.filtered_directories();
    let rows = directories.iter().enumerate().map(|(index, directory)| {
        let style = if index == app.directory_index {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default()
        };
        Row::new(vec![
            Cell::from(format!("{:.0}", directory.score)),
            Cell::from(directory.files.to_string()),
            Cell::from(
                directory
                    .dominant_language
                    .clone()
                    .unwrap_or_else(|| "-".into()),
            ),
            Cell::from(directory.path.clone()),
        ])
        .style(style)
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Length(12),
            Constraint::Min(24),
        ],
    )
    .header(
        Row::new(vec!["score", "files", "language", "directory"]).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(Block::default().title("Directories").borders(Borders::ALL));
    frame.render_widget(table, area);
}

fn draw_directory_details(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    let Some(directory) = app.selected_directory() else {
        frame.render_widget(
            Paragraph::new("No directories match the current filter")
                .block(Block::default().title("Details").borders(Borders::ALL)),
            area,
        );
        return;
    };
    let lines = vec![
        Line::from(Span::styled(
            directory.path.clone(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(format!(
            "score {:.1}, {} files, {} lines, {} bytes",
            directory.score, directory.files, directory.lines, directory.bytes
        )),
        Line::from(format!(
            "dominant language: {}",
            directory
                .dominant_language
                .clone()
                .unwrap_or_else(|| "-".into())
        )),
        Line::from(format!(
            "top path: {}",
            directory.top_path.clone().unwrap_or_else(|| "-".into())
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().title("Details").borders(Borders::ALL))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_workloads(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    let workloads = app.filtered_workloads();
    let rows = workloads.iter().enumerate().map(|(index, workload)| {
        let style = if index == app.workload_index {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default()
        };
        let status = workload
            .result
            .as_ref()
            .map(|result| format!("{:.2}ms", result.stats.mean_ms))
            .unwrap_or_else(|| workload.status.clone());
        Row::new(vec![
            Cell::from(workload.spec.name.clone()),
            Cell::from(workload.spec.kind.clone()),
            Cell::from(status),
            Cell::from(workload.spec.command.join(" ")),
        ])
        .style(style)
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(18),
            Constraint::Length(10),
            Constraint::Length(12),
            Constraint::Min(26),
        ],
    )
    .header(
        Row::new(vec!["name", "kind", "status", "command"]).style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(Block::default().title("Workloads").borders(Borders::ALL));
    frame.render_widget(table, area);
}

fn draw_workload_details(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    let Some(workload) = app.selected_workload() else {
        frame.render_widget(
            Paragraph::new("No workloads match the current filter")
                .block(Block::default().title("Details").borders(Borders::ALL)),
            area,
        );
        return;
    };

    let mut lines = vec![
        Line::from(Span::styled(
            workload.spec.name.clone(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("kind: {}", workload.spec.kind)),
        Line::from(format!("detected from: {}", workload.spec.detected_from)),
        Line::from(format!("command: {}", workload.spec.command.join(" "))),
        Line::from(format!(
            "iterations: {}  warmups: {}",
            app.iterations, app.warmups
        )),
        Line::from(""),
        Line::from(workload.spec.description.clone()),
    ];
    if let Some(result) = &workload.result {
        lines.push(Line::from(""));
        lines.push(Line::from(format!(
            "mean {:.2} ms  median {:.2} ms  p95 {:.2} ms",
            result.stats.mean_ms, result.stats.median_ms, result.stats.p95_ms
        )));
        lines.push(Line::from(format!(
            "stdout: {}",
            truncate_line(&result.stdout_preview)
        )));
        lines.push(Line::from(format!(
            "stderr: {}",
            truncate_line(&result.stderr_preview)
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().title("Details").borders(Borders::ALL))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_footer(frame: &mut Frame<'_>, app: &TuiApp, area: Rect) {
    let mode = if app.filter_mode {
        "filter mode: type, Enter/Esc exits, Ctrl-U clears"
    } else if !app.status.is_empty() {
        app.status.as_str()
    } else if area.width < 96 {
        "1-4 tabs  j/k move  / filter  Enter run/open  ? help"
    } else {
        "1-4 tabs  j/k move  / filter  s sort hotspots  Enter/o open or run  r run workload  ? help"
    };
    frame.render_widget(
        Paragraph::new(mode).block(Block::default().borders(Borders::ALL)),
        area,
    );
}

fn draw_help(frame: &mut Frame<'_>, area: Rect) {
    let width = area.width.min(80);
    let height = area.height.min(15);
    let rect = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    let text = vec![
        Line::from("profilr keys"),
        Line::from(""),
        Line::from("1-4 switch tabs, Tab and Shift-Tab cycle tabs"),
        Line::from("j/down and k/up move through the active list"),
        Line::from("/ filters the active view"),
        Line::from("s rotates hotspot sort order"),
        Line::from("Enter or o opens the selected file or runs the selected workload"),
        Line::from("r runs the selected workload from the Workloads tab"),
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

fn open_selected(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &TuiApp,
) -> Result<Option<String>> {
    let path = match app.tab {
        TabKind::Hotspots => app
            .selected_hotspot()
            .map(|hotspot| app.profile.root.join(&hotspot.path)),
        TabKind::Directories => app
            .selected_directory()
            .and_then(|directory| directory.top_path.as_ref())
            .map(|path| app.profile.root.join(path)),
        _ => None,
    };
    let Some(path) = path else {
        return Ok(None);
    };
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

fn run_selected_workload(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut TuiApp,
) -> Result<Option<String>> {
    if app.tab != TabKind::Workloads {
        return Ok(None);
    }
    let workload_name = app
        .selected_workload()
        .map(|workload| workload.spec.name.clone());
    let Some(workload_name) = workload_name else {
        return Ok(None);
    };
    let root = app.profile.root.clone();

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    let iterations = app.iterations;
    let warmups = app.warmups;
    let result = if let Some(workload) = app.selected_workload_mut() {
        benchmark_single_workload(&root, workload, iterations, warmups)
    } else {
        Ok(())
    };
    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    enable_raw_mode()?;
    terminal.clear()?;

    let message = match result {
        Ok(()) => {
            let mean = app
                .profile
                .workloads
                .iter()
                .find(|workload| workload.spec.name == workload_name)
                .and_then(|workload| workload.result.as_ref())
                .map(|result| result.stats.mean_ms)
                .unwrap_or(0.0);
            format!("benchmarked {} at {:.2} ms mean", workload_name, mean)
        }
        Err(err) => format!("workload {} failed: {err}", workload_name),
    };
    Ok(Some(message))
}

fn score_bar(label: &str, value: f64, max: f64, width: usize) -> String {
    let filled = if max <= 0.0 {
        0
    } else {
        ((value / max) * width as f64).round() as usize
    };
    format!(
        "{:<14} {:<width$} {:>7.1}",
        truncate_label(label, 14),
        "#".repeat(filled.max(1)),
        value,
        width = width
    )
}

fn metric_bar(label: &str, value: usize, cap: usize) -> String {
    let filled = value.min(cap).min(20);
    format!("{label:<12} {:<20} {value}", "#".repeat(filled))
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

fn adjust_index(index: &mut usize, len: usize, delta: isize) {
    if len == 0 {
        *index = 0;
        return;
    }
    let next = (*index as isize + delta).clamp(0, len.saturating_sub(1) as isize);
    *index = next as usize;
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

fn truncate_label(label: &str, max: usize) -> String {
    let mut shortened = label.chars().take(max).collect::<String>();
    if label.chars().count() > max {
        shortened.pop();
        shortened.push('~');
    }
    shortened
}

fn truncate_line(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return "-".into();
    }
    let mut shortened = trimmed.chars().take(72).collect::<String>();
    if trimmed.chars().count() > 72 {
        shortened.push_str("...");
    }
    shortened
}
