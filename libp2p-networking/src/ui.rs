use color_eyre::{Report, eyre::{Result, WrapErr}};
use crossterm::event::{self, Event, KeyCode};
use flume::{Sender, Receiver};
use futures::{select, StreamExt, FutureExt};
use async_std::task::{sleep, spawn};
use parking_lot::Mutex;
use libp2p::gossipsub::GossipsubMessage;
use serde::{Serialize, Deserialize};
use tracing::instrument;
use tui::{widgets::{Block, Borders, Row, Cell, Table, Paragraph, TableState}, style::{Color, Modifier, Style}, Frame, layout::{Layout, Constraint}, backend::Backend, Terminal};
use std::{time::Duration, sync::Arc, collections::VecDeque};

use crate::{SwarmResult, GossipMsg, SwarmAction};

#[derive(Debug, Copy, Clone)]
pub enum InputMode {
    Normal,
    Editing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub sender: String,
    pub content: String,
    pub topic: String,
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
            .map_or_else(|| "UNKNOWN".to_string(), |p| p.to_string());
        Message {
            sender,
            content,
            topic: msg.topic.into_string(),
        }
    }
}


#[derive(Debug)]
/// Struct for the TUI app
pub struct TableApp {
    pub send_swarm: Sender<SwarmAction<Message>>,
    pub recv_swarm: Receiver<SwarmResult<Message>>,
    pub input_mode: InputMode,
    pub input: String,
    pub state: TableState,
    pub message_buffer: Arc<Mutex<VecDeque<Message>>>,
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

#[allow(clippy::mut_mut, clippy::panic)]
#[instrument(skip(terminal, app))]
pub async fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut app: TableApp) -> Result<()> {
    let mut events = event::EventStream::new();
    loop {
        terminal
            .draw(|f| ui(f, &mut app).expect("Failed to draw UI"))
            .context("Failed drawing application")?;
        select!(
            _ = sleep(Duration::from_nanos(1)).fuse() => {}
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
                                KeyCode::Down | KeyCode::Char('j') => app.next(),
                                KeyCode::Up | KeyCode::Char('k')=> app.previous(),
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
                                    let mb_handle = app.message_buffer.clone();
                                    let send_swarm = app.send_swarm.clone();
                                    // we don't want this to block the event loop
                                    spawn(async move {
                                        let (s, r) = flume::unbounded();
                                        send_swarm.send_async(SwarmAction::GetId(s)).await.context("")?;
                                        let msg = Message {
                                            topic: "global".to_string(),
                                            content: app.input,
                                            // FIXME this should NOT be needed. Get it from the swarm.
                                            sender: r.recv_async().await?.to_string(),
                                        };
                                        let (s, r) = flume::unbounded();
                                        send_swarm.send_async(SwarmAction::GossipMsg(msg.clone(), s)).await?;
                                        // if it's a duplicate message (error case), fail silently and do nothing
                                        if r.recv_async().await?.is_ok() {
                                            mb_handle.lock().push_back(msg);
                                        }
                                        Result::<(), Report>::Ok(())
                                    });
                                    app.input = String::new();
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

#[allow(clippy::unnecessary_wraps)]
fn ui<B: Backend>(f: &mut Frame<'_, B>, app: &mut TableApp) -> Result<()> {
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
    let rows : Vec<Row<'_>> = handle.iter().map(|message| {
        let sender = message.sender.clone();
        let content = message.content.clone();
        let height = content.chars().filter(|c| *c == '\n').count() + 1;
        let cells = vec![Cell::from(sender), Cell::from(content)];
        Ok(Row::new(cells).height(u16::try_from(height).context("row too low")?))
    }).collect::<Result<Vec<Row<'_>>, Report>>()?;
    let table = Table::new(rows.into_iter())
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
