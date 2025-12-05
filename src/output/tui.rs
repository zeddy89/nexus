// Terminal UI for real-time playbook execution monitoring

use std::collections::{HashMap, VecDeque};
use std::io::{self, stdout};
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph},
    Frame, Terminal,
};
use tokio::sync::mpsc;

use super::events::{ExecutionEvent, TaskStatus};
use super::terminal::PlayRecap;
use crate::output::NexusError;

/// State of a single host
#[derive(Debug, Clone)]
pub struct HostState {
    pub name: String,
    pub status: HostStatus,
    pub current_task: Option<String>,
    pub tasks_completed: usize,
    pub tasks_failed: usize,
}

/// Current status of a host
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostStatus {
    /// Host is waiting for tasks
    Waiting,
    /// Host is currently running a task
    Running,
    /// All tasks on host completed successfully
    Ok,
    /// At least one task on host failed
    Failed,
}

impl HostState {
    pub fn new(name: String) -> Self {
        HostState {
            name,
            status: HostStatus::Waiting,
            current_task: None,
            tasks_completed: 0,
            tasks_failed: 0,
        }
    }

    pub fn start_task(&mut self, task: String) {
        self.status = HostStatus::Running;
        self.current_task = Some(task);
    }

    pub fn complete_task(&mut self, status: TaskStatus) {
        self.tasks_completed += 1;
        if status == TaskStatus::Failed {
            self.tasks_failed += 1;
            self.status = HostStatus::Failed;
        } else if self.status != HostStatus::Failed {
            self.status = HostStatus::Ok;
        }
        self.current_task = None;
    }

    pub fn status_symbol(&self) -> &str {
        match self.status {
            HostStatus::Waiting => "○",
            HostStatus::Running => "⟳",
            HostStatus::Ok => "✓",
            HostStatus::Failed => "✗",
        }
    }

    pub fn status_color(&self) -> Color {
        match self.status {
            HostStatus::Waiting => Color::Gray,
            HostStatus::Running => Color::Yellow,
            HostStatus::Ok => Color::Green,
            HostStatus::Failed => Color::Red,
        }
    }

    pub fn status_text(&self) -> &str {
        match self.status {
            HostStatus::Waiting => "WAIT",
            HostStatus::Running => "RUN",
            HostStatus::Ok => "OK",
            HostStatus::Failed => "FAIL",
        }
    }
}

/// Log entry for the log window
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: Instant,
    pub host: String,
    pub message: String,
}

impl LogEntry {
    pub fn new(host: String, message: String) -> Self {
        LogEntry {
            timestamp: Instant::now(),
            host,
            message,
        }
    }

    pub fn format(&self, start_time: Instant) -> String {
        let elapsed = self.timestamp.duration_since(start_time).as_secs();
        format!(
            "[{:02}:{:02}] {}: {}",
            elapsed / 60,
            elapsed % 60,
            self.host,
            self.message
        )
    }
}

/// State of the TUI application
pub struct TuiState {
    pub playbook_name: String,
    pub start_time: Instant,
    pub final_elapsed: Option<Duration>, // Frozen time when complete
    pub hosts: HashMap<String, HostState>,
    pub host_order: Vec<String>,
    pub current_task: String,
    pub total_tasks: usize,     // Total tasks × hosts
    pub completed_tasks: usize, // Tasks completed (per-host)
    pub num_hosts: usize,       // Number of hosts
    pub logs: VecDeque<LogEntry>,
    pub is_complete: bool,
    pub final_recap: Option<PlayRecap>,
    pub log_scroll: usize,
    pub max_logs: usize,
}

impl Default for TuiState {
    fn default() -> Self {
        Self::new()
    }
}

impl TuiState {
    pub fn new() -> Self {
        TuiState {
            playbook_name: String::new(),
            start_time: Instant::now(),
            final_elapsed: None,
            hosts: HashMap::new(),
            host_order: Vec::new(),
            current_task: String::new(),
            total_tasks: 0,
            completed_tasks: 0,
            num_hosts: 0,
            logs: VecDeque::new(),
            is_complete: false,
            final_recap: None,
            log_scroll: 0,
            max_logs: 1000,
        }
    }

    pub fn init_playbook(&mut self, name: String, hosts: Vec<String>, total_tasks: usize) {
        self.playbook_name = name;
        self.num_hosts = hosts.len();
        // Total tasks = unique tasks × number of hosts
        self.total_tasks = total_tasks * hosts.len();
        self.start_time = Instant::now();

        for host in hosts {
            self.host_order.push(host.clone());
            self.hosts.insert(host.clone(), HostState::new(host));
        }
    }

    pub fn task_start(&mut self, host: String, task: String) {
        if let Some(host_state) = self.hosts.get_mut(&host) {
            host_state.start_task(task.clone());
        }
        self.current_task = task.clone();
        self.add_log(host.clone(), format!("Starting: {}", task));
    }

    pub fn task_complete(&mut self, host: String, task: String, status: TaskStatus) {
        if let Some(host_state) = self.hosts.get_mut(&host) {
            host_state.complete_task(status);
        }

        self.completed_tasks += 1;

        let status_str = match status {
            TaskStatus::Ok => "OK",
            TaskStatus::Changed => "CHANGED",
            TaskStatus::Failed => "FAILED",
            TaskStatus::Skipped => "SKIPPED",
        };

        self.add_log(host, format!("{}: {}", status_str, task));
    }

    pub fn task_skipped(&mut self, host: String, task: String) {
        self.completed_tasks += 1;
        self.add_log(host, format!("SKIPPED: {}", task));
    }

    pub fn task_failed(&mut self, host: String, task: String, error: String) {
        if let Some(host_state) = self.hosts.get_mut(&host) {
            host_state.complete_task(TaskStatus::Failed);
        }

        self.completed_tasks += 1;
        self.add_log(host.clone(), format!("FAILED: {}", task));
        self.add_log(host, format!("Error: {}", error));
    }

    pub fn add_log(&mut self, host: String, message: String) {
        self.logs.push_back(LogEntry::new(host, message));

        // Limit log size
        while self.logs.len() > self.max_logs {
            self.logs.pop_front();
        }
    }

    pub fn playbook_complete(&mut self, recap: PlayRecap) {
        self.is_complete = true;
        self.final_elapsed = Some(self.start_time.elapsed()); // Freeze timer
        self.final_recap = Some(recap);
    }

    pub fn elapsed(&self) -> Duration {
        // Return frozen time if complete, otherwise current elapsed
        self.final_elapsed
            .unwrap_or_else(|| self.start_time.elapsed())
    }

    pub fn scroll_up(&mut self) {
        if self.log_scroll > 0 {
            self.log_scroll -= 1;
        }
    }

    pub fn scroll_down(&mut self) {
        let max_scroll = self.logs.len().saturating_sub(10);
        if self.log_scroll < max_scroll {
            self.log_scroll += 1;
        }
    }
}

/// TUI Application
pub struct TuiApp {
    state: TuiState,
    rx: mpsc::UnboundedReceiver<ExecutionEvent>,
}

impl TuiApp {
    pub fn new(rx: mpsc::UnboundedReceiver<ExecutionEvent>) -> Self {
        TuiApp {
            state: TuiState::new(),
            rx,
        }
    }

    /// Run the TUI application
    pub async fn run(&mut self) -> Result<(), NexusError> {
        // Setup terminal
        enable_raw_mode().map_err(|e| NexusError::Runtime {
            function: None,
            message: format!("Failed to enable raw mode: {}", e),
            suggestion: None,
        })?;

        let mut stdout = stdout();
        execute!(stdout, EnterAlternateScreen).map_err(|e| NexusError::Runtime {
            function: None,
            message: format!("Failed to enter alternate screen: {}", e),
            suggestion: None,
        })?;

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend).map_err(|e| NexusError::Runtime {
            function: None,
            message: format!("Failed to create terminal: {}", e),
            suggestion: None,
        })?;

        let result = self.run_loop(&mut terminal).await;

        // Restore terminal
        disable_raw_mode().ok();
        execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
        terminal.show_cursor().ok();

        result
    }

    /// Main event loop
    async fn run_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> Result<(), NexusError> {
        loop {
            // Draw UI
            terminal
                .draw(|f| self.render(f))
                .map_err(|e| NexusError::Runtime {
                    function: None,
                    message: format!("Failed to draw terminal: {}", e),
                    suggestion: None,
                })?;

            // Check for events (non-blocking)
            while let Ok(event) = self.rx.try_recv() {
                self.handle_event(event);
            }

            // Check for user input with timeout
            if event::poll(Duration::from_millis(100)).map_err(|e| NexusError::Runtime {
                function: None,
                message: format!("Failed to poll events: {}", e),
                suggestion: None,
            })? {
                if let Event::Key(key) = event::read().map_err(|e| NexusError::Runtime {
                    function: None,
                    message: format!("Failed to read event: {}", e),
                    suggestion: None,
                })? {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            // Allow exit any time - playbook continues in background if not complete
                            break;
                        }
                        KeyCode::Up => self.state.scroll_up(),
                        KeyCode::Down => self.state.scroll_down(),
                        _ => {}
                    }
                }
            }

            // Don't auto-exit - wait for user to press 'q' or ESC to review results
        }

        Ok(())
    }

    /// Handle an execution event
    fn handle_event(&mut self, event: ExecutionEvent) {
        match event {
            ExecutionEvent::PlaybookStart {
                name,
                hosts,
                total_tasks,
            } => {
                self.state.init_playbook(name, hosts, total_tasks);
            }
            ExecutionEvent::TaskStart { host, task } => {
                self.state.task_start(host, task);
            }
            ExecutionEvent::TaskComplete {
                host, task, status, ..
            } => {
                self.state.task_complete(host, task, status);
            }
            ExecutionEvent::TaskSkipped { host, task } => {
                self.state.task_skipped(host, task);
            }
            ExecutionEvent::TaskFailed { host, task, error } => {
                self.state.task_failed(host, task, error);
            }
            ExecutionEvent::Log { host, message } => {
                self.state.add_log(host, message);
            }
            ExecutionEvent::PlaybookComplete { recap } => {
                self.state.playbook_complete(recap);
            }
        }
    }

    /// Render the UI
    fn render(&self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Header
                Constraint::Min(8),    // Main content
                Constraint::Length(1), // Footer
            ])
            .split(f.area());

        self.render_header(f, chunks[0]);
        self.render_main(f, chunks[1]);
        self.render_footer(f, chunks[2]);
    }

    /// Render header
    fn render_header(&self, f: &mut Frame, area: Rect) {
        let elapsed = self.state.elapsed();
        let time_str = format!(
            "[{:02}:{:02}:{:02}]",
            elapsed.as_secs() / 3600,
            (elapsed.as_secs() % 3600) / 60,
            elapsed.as_secs() % 60
        );

        let title = if self.state.is_complete {
            format!("Nexus - {} - COMPLETE", self.state.playbook_name)
        } else {
            format!("Nexus - {}", self.state.playbook_name)
        };

        // Calculate inner width (account for borders: 2 chars)
        let inner_width = area.width.saturating_sub(2) as usize;
        let title_len = title.len();
        let time_len = time_str.len();

        // Calculate padding between title and time
        let padding = if inner_width > title_len + time_len {
            inner_width - title_len - time_len
        } else {
            1 // Minimum 1 space
        };

        let header = Paragraph::new(Line::from(vec![
            Span::styled(
                title,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" ".repeat(padding)),
            Span::styled(time_str, Style::default().fg(Color::Yellow)),
        ]))
        .block(Block::default().borders(Borders::ALL));

        f.render_widget(header, area);
    }

    /// Render main content area
    fn render_main(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(30), // Hosts panel
                Constraint::Percentage(70), // Right panel
            ])
            .split(area);

        self.render_hosts(f, chunks[0]);
        self.render_right_panel(f, chunks[1]);
    }

    /// Render hosts panel
    fn render_hosts(&self, f: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .state
            .host_order
            .iter()
            .filter_map(|host_name| {
                self.state.hosts.get(host_name).map(|host| {
                    let symbol = host.status_symbol();
                    let color = host.status_color();
                    let status = host.status_text();

                    let line = Line::from(vec![
                        Span::styled(
                            symbol,
                            Style::default().fg(color).add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(" "),
                        Span::styled(&host.name, Style::default().fg(Color::White)),
                        Span::raw("  "),
                        Span::styled(status, Style::default().fg(color)),
                    ]);

                    ListItem::new(line)
                })
            })
            .collect();

        let list = List::new(items).block(Block::default().title("Hosts").borders(Borders::ALL));

        f.render_widget(list, area);
    }

    /// Render right panel (progress + logs or recap)
    fn render_right_panel(&self, f: &mut Frame, area: Rect) {
        if self.state.is_complete && self.state.final_recap.is_some() {
            // Show recap when complete
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(6), // Progress panel
                    Constraint::Min(8),    // Recap panel (needs more space)
                    Constraint::Min(5),    // Logs panel
                ])
                .split(area);

            self.render_progress(f, chunks[0]);
            self.render_recap(f, chunks[1]);
            self.render_logs(f, chunks[2]);
        } else {
            // Normal execution view
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(6), // Progress panel
                    Constraint::Min(5),    // Logs panel
                ])
                .split(area);

            self.render_progress(f, chunks[0]);
            self.render_logs(f, chunks[1]);
        }
    }

    /// Render progress panel
    fn render_progress(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(2),
            ])
            .split(area);

        // Current task
        let current_task = if !self.state.current_task.is_empty() {
            Paragraph::new(format!("Current Task: {}", self.state.current_task))
                .style(Style::default().fg(Color::Cyan))
        } else {
            Paragraph::new("Current Task: (none)").style(Style::default().fg(Color::Gray))
        };
        f.render_widget(current_task, chunks[0]);

        // Progress bar - clamp ratio between 0 and 1
        let ratio = if self.state.total_tasks > 0 {
            (self.state.completed_tasks as f64 / self.state.total_tasks as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let progress_label = format!(
            "{}/{} tasks",
            self.state.completed_tasks, self.state.total_tasks
        );

        let gauge = Gauge::default()
            .block(Block::default())
            .gauge_style(Style::default().fg(Color::Green))
            .ratio(ratio)
            .label(progress_label);

        f.render_widget(gauge, chunks[2]);

        let block = Block::default().title("Progress").borders(Borders::ALL);
        f.render_widget(block, area);
    }

    /// Render recap panel showing final statistics
    fn render_recap(&self, f: &mut Frame, area: Rect) {
        if let Some(recap) = &self.state.final_recap {
            let _inner = Block::default()
                .title("PLAY RECAP")
                .borders(Borders::ALL)
                .inner(area);

            let mut items = Vec::new();

            // Add each host's stats
            for host_name in &self.state.host_order {
                if let Some(stats) = recap.hosts.get(host_name) {
                    let ok_style = Style::default().fg(Color::Green);
                    let changed_style = if stats.changed > 0 {
                        Style::default().fg(Color::Yellow)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    let failed_style = if stats.failed > 0 {
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    let skipped_style = Style::default().fg(Color::Cyan);

                    let line = Line::from(vec![
                        Span::styled(
                            format!("{:<20}", host_name),
                            Style::default()
                                .fg(Color::White)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(" : "),
                        Span::styled(format!("ok={:<3}", stats.ok), ok_style),
                        Span::raw("  "),
                        Span::styled(format!("changed={:<3}", stats.changed), changed_style),
                        Span::raw("  "),
                        Span::styled(format!("failed={:<3}", stats.failed), failed_style),
                        Span::raw("  "),
                        Span::styled(format!("skipped={}", stats.skipped), skipped_style),
                    ]);

                    items.push(ListItem::new(line));
                }
            }

            // Add total time
            items.push(ListItem::new(Line::from(""))); // Empty line
            items.push(ListItem::new(Line::from(vec![Span::styled(
                format!("Total time: {:.2}s", recap.total_duration.as_secs_f64()),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )])));

            let list =
                List::new(items).block(Block::default().title("PLAY RECAP").borders(Borders::ALL));

            f.render_widget(list, area);
        }
    }

    /// Render logs panel
    fn render_logs(&self, f: &mut Frame, area: Rect) {
        let log_area = Block::default()
            .title("Log Output (↑/↓ to scroll)")
            .borders(Borders::ALL)
            .inner(area);

        let visible_height = log_area.height as usize;
        let start_idx = self.state.log_scroll;
        let _end_idx = (start_idx + visible_height).min(self.state.logs.len());

        let logs: Vec<ListItem> = self
            .state
            .logs
            .iter()
            .skip(start_idx)
            .take(visible_height)
            .map(|entry| ListItem::new(entry.format(self.state.start_time)))
            .collect();

        let list = List::new(logs).block(
            Block::default()
                .title("Log Output (↑/↓ to scroll)")
                .borders(Borders::ALL),
        );

        f.render_widget(list, area);
    }

    /// Render footer
    fn render_footer(&self, f: &mut Frame, area: Rect) {
        let footer = if self.state.is_complete {
            Paragraph::new("Press 'q' or ESC to exit")
                .style(Style::default().fg(Color::Green))
                .alignment(Alignment::Center)
        } else {
            Paragraph::new(
                "Press 'q' to quit (playbook will continue in background) | ↑/↓ scroll logs",
            )
            .style(Style::default().fg(Color::Gray))
            .alignment(Alignment::Center)
        };

        f.render_widget(footer, area);
    }
}
