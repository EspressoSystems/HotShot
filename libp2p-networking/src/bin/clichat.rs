use libp2p::{gossipsub::GossipsubMessage, Multiaddr};

use std::sync::Arc;
use std::{collections::VecDeque, marker::PhantomData};
use structopt::StructOpt;

use futures::{select, StreamExt, FutureExt};
use color_eyre::eyre::{Result, WrapErr};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::time::{Duration};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tracing::instrument;
use tui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
    Frame, Terminal,
};

use flume::{Receiver, Sender};
use networking_demo::{gen_multiaddr, GossipMsg, Network, NetworkDef, SwarmAction, SwarmResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    sender: String,
    content: String,
    topic: String,
}

impl GossipMsg for Message {
    fn topic(&self) -> libp2p::gossipsub::IdentTopic {
        libp2p::gossipsub::IdentTopic::new(self.topic.clone())
    }
    fn data(&self) -> Vec<u8> {
        self.content.as_bytes().into()
    }
}

impl From<GossipsubMessage> for Message {
    fn from(msg: GossipsubMessage) -> Self {
        let content = String::from_utf8_lossy(&msg.data).to_string();
        let sender = msg
            .source
            .map(|p| p.to_string())
            .unwrap_or_else(|| "UNKNOWN".to_string());
        Message {
            sender,
            content,
            topic: msg.topic.into_string(),
        }
    }
}

enum InputMode {
    Normal,
    Editing,
}

/// Struct for the TUI app
struct TableApp {
    send_swarm: Sender<SwarmAction<Message>>,
    recv_swarm: Receiver<SwarmResult<Message>>,
    input_mode: InputMode,
    input: String,
    state: TableState,
    message_buffer: Arc<Mutex<VecDeque<Message>>>,
}

impl TableApp {
    pub fn new(
        message_buffer: Arc<Mutex<VecDeque<Message>>>,
        send_swarm: Sender<SwarmAction<Message>>,
        recv_swarm: Receiver<SwarmResult<Message>>,
    ) -> Self {
        Self {
            send_swarm,
            recv_swarm,
            input_mode: InputMode::Normal,
            input: String::new(),
            state: TableState::default(),
            message_buffer,
        }
    }
    pub fn next(&mut self) {
        let buffer_handle = self.message_buffer.lock();
        let i = self.state.selected().unwrap_or(0) + 1;
        self.state.select(Some(i % buffer_handle.len()));
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

/// command line arguments
#[derive(StructOpt)]
struct CliOpt {
    /// Path to the node configuration file
    #[structopt(long = "port", short = "p")]
    port: Option<u16>,

    #[structopt()]
    first_dial: Option<Multiaddr>,
}

#[async_std::main]
#[instrument]
async fn main() -> Result<()> {
    // -- Setup color_eyre and tracing
    color_eyre::install()?;
    networking_demo::tracing_setup::setup_tracing();
    // -- Spin up the network connection
    let mut networking: Network<Message, NetworkDef> = Network::new(PhantomData)
        .await
        .context("Failed to launch network")?;
    let port = CliOpt::from_args().port.unwrap_or(0u16);
    let known_peer = CliOpt::from_args().first_dial;
    let listen_addr = gen_multiaddr(port);
    networking.start(listen_addr, known_peer)?;
    let (send_chan, recv_chan) = networking.spawn_listeners().await;

    // -- Spin up the UI
    // Setup a ring buffer to hold messages, 25 of them should do for the demo
    let message_buffer: Arc<Mutex<VecDeque<Message>>> = Arc::new(Mutex::new(VecDeque::new()));
    // Put a few dummy messages in there so we can display something
    let mut buffer_handle = message_buffer.lock();
    buffer_handle.push_back(Message {
        sender: "Nathan".to_string(),
        content: "Hello".to_string(),
        topic: "global".to_string(),
    });
    buffer_handle.push_back(Message {
        sender: "Justin".to_string(),
        content: "hi!".to_string(),
        topic: "global".to_string(),
    });
    buffer_handle.push_back(Message {
        sender: "Joe".to_string(),
        content: "test".to_string(),
        topic: "global".to_string(),
    });
    buffer_handle.push_back(Message {
        sender: "John".to_string(),
        content: "test 2".to_string(),
        topic: "global".to_string(),
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
    let app = TableApp::new(message_buffer.clone(), send_chan, recv_chan);
    app.send_swarm
        .send(SwarmAction::Subscribe("global".to_string()))?;
    let res = run_app(&mut terminal, app).await;
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

async fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut app: TableApp) -> Result<()> {
    let mut events = event::EventStream::new();
    loop {
        terminal
            .draw(|f| ui(f, &mut app).expect("Failed to draw UI"))
            .context("Failed drawing application")?;
        select!(
            _ = async_std::task::sleep(Duration::from_nanos(1)).fuse() => {}
            swarm_msg = app.recv_swarm.recv_async() => {
                if let Ok(res) = swarm_msg {
                    match res {
                        SwarmResult::GossipMsg(s) => app.message_buffer.lock().push_back(s),
                    }
                }
            },
            user_event = events.next().fuse() => {
                match app.input_mode {
                    InputMode::Normal => {
                        if let Some(Ok(Event::Key(key))) = user_event {
                            match key.code {
                                KeyCode::Char('q') => {
                                    app.send_swarm.send_async(SwarmAction::Shutdown).await?;
                                    return Ok(());
                                }
                                KeyCode::Down => app.next(),
                                KeyCode::Up => app.previous(),
                                KeyCode::Char('k') => app.previous(),
                                KeyCode::Char('j') => app.next(),
                                KeyCode::Tab => app.input_mode = InputMode::Editing,
                                _ => {}
                            }
                        }
                    }
                    InputMode::Editing => {
                        if let Some(Ok(Event::Key(key))) = user_event {
                            match key.code {
                                // broadcast message to the swarm with the global topic
                                KeyCode::Enter => {
                                    let (s, r) = flume::unbounded();
                                    app.send_swarm.send_async(SwarmAction::GetId(s)).await?;
                                    let msg = Message {
                                        topic: "global".to_string(),
                                        content: app.input,
                                        // FIXME this should NOT be needed. Get it from the swarm.
                                        sender: r.recv_async().await?.to_string(),
                                    };
                                    let (s, r) = flume::unbounded();
                                    app.send_swarm.send_async(SwarmAction::GossipMsg(msg.clone(), s)).await?;
                                    let mb_handle = app.message_buffer.clone();
                                    async_std::task::spawn(async move {
                                        match r.recv_async().await? {
                                            Ok(_) => mb_handle.lock().push_back(msg),
                                            Err(_) => (),
                                        }
                                        Result::<(), flume::RecvError>::Ok(())
                                    });
                                    app.input = String::new()
                                }
                                KeyCode::Char(c) => {
                                    app.input.push(c);
                                }
                                KeyCode::Backspace => {
                                    app.input.pop();
                                }
                                KeyCode::Tab => {
                                    app.input_mode = InputMode::Normal;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }

        );
    }
}

fn ui<B: Backend>(f: &mut Frame<B>, app: &mut TableApp) -> Result<()> {
    let rects = Layout::default()
        // two rectanges: one for messages, the other for input
        .constraints(
            [Constraint::Percentage(60),
             Constraint::Percentage(30),
             Constraint::Percentage(10)
            ].as_ref())
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
        ])
        .style(match app.input_mode {
            InputMode::Editing => Style::default(),
            InputMode::Normal => Style::default().fg(Color::Yellow),
        })
        ;
    f.render_stateful_widget(table, rects[0], &mut app.state);

    let input = Paragraph::new(app.input.as_ref())
        .style(match app.input_mode {
            InputMode::Normal => Style::default(),
            InputMode::Editing => Style::default().fg(Color::Yellow),
        })
        .block(Block::default().borders(Borders::ALL).title("Input"));
    f.render_widget(input, rects[2]);

    Ok(())
}
