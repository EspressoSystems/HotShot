use std::collections::VecDeque;
use std::sync::Arc;

use color_eyre::eyre::{eyre, Result, WrapErr};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

use parking_lot::Mutex;
use tui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Row, Table, TableState},
    Frame, Terminal,
};

#[derive(Debug, Clone)]
pub struct Message {
    sender: String,
    content: String,
}

/// Struct for the TUI app
struct TableApp {
    state: TableState,
    message_buffer: Arc<Mutex<VecDeque<Message>>>,
}

impl TableApp {
    pub fn new(message_buffer: Arc<Mutex<VecDeque<Message>>>) -> Self {
        Self {
            state: TableState::default(),
            message_buffer,
        }
    }
    pub fn next(&mut self) {
        let buffer_handle = self.message_buffer.lock();
        let i = match self.state.selected() {
            Some(i) => {
                if i >= buffer_handle.len() {
                    0
                } else {
                    i + i
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn previous(&mut self) {
        let buffer_handle = self.message_buffer.lock();
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    buffer_handle.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }
}

fn main() -> Result<()> {
    color_eyre::install()?;
    // Setup a ring buffer to hold messages, 25 of them should do for the demo
    let message_buffer: Arc<Mutex<VecDeque<Message>>> = Arc::new(Mutex::new(VecDeque::new()));
    // Put a few dummy messages in there so we can display something
    let mut buffer_handle = message_buffer.lock();
    buffer_handle.push_back(Message {
        sender: "Nathan".to_string(),
        content: "Hello".to_string(),
    });
    buffer_handle.push_back(Message {
        sender: "Justin".to_string(),
        content: "hi!".to_string(),
    });
    std::mem::drop(buffer_handle);
    // -- Setup the TUI
    // Start by setting up the terminal
    enable_raw_mode()?; // Turn on raw mode
                        // Get stdio and configure the terminal
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    // -- Create the app and run it
    let app = TableApp::new(message_buffer.clone());
    let res = run_app(&mut terminal, app);
    // -- Tear down the TUI, and restore the terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    // Print the messages
    let messages = message_buffer.lock().iter().cloned().collect::<Vec<_>>();
    println!("{:?}", messages);
    res
}

fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut app: TableApp) -> Result<()> {
    loop {
        terminal
            .draw(|f| ui(f, &mut app).expect("Failed to draw UI"))
            .context("Failed drawing application")?;
        match event::read().context("Failed to read event")? {
            Event::Key(key) => match key.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Down => app.next(),
                KeyCode::Up => app.previous(),
                _ => {}
            },
            _ => (),
        }
    }
}

fn ui<B: Backend>(f: &mut Frame<B>, app: &mut TableApp) -> Result<()> {
    let rects = Layout::default()
        .constraints([Constraint::Percentage(100)].as_ref())
        .margin(5)
        .split(f.size());

    // Styles
    let selected_style = Style::default().add_modifier(Modifier::REVERSED);
    let normal_style = Style::default().bg(Color::Blue);
    // Setup header
    let header_cells = ["Sender", "Message"]
        .iter()
        .map(|h| Cell::from(*h).style(Style::default().fg(Color::Red)));
    let header = Row::new(header_cells).style(normal_style);
    // Generate the rows
    let handle = app.message_buffer.lock(); // Lock the messages mutex
    let rows = handle.iter().map(|message| {
        let sender = message.sender.clone();
        let content = message.content.clone();
        let height = content.chars().filter(|c| *c == '\n').count() + 1;
        let cells = vec![Cell::from(sender), Cell::from(content)];
        Row::new(cells).height(height as u16)
    });
    let table = Table::new(rows)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title("Messages"))
        .highlight_style(selected_style)
        .highlight_symbol(">> ")
        .widths(&[
            Constraint::Percentage(50),
            Constraint::Length(30),
            Constraint::Min(10),
        ]);
    f.render_stateful_widget(table, rects[0], &mut app.state);

    Ok(())
}
