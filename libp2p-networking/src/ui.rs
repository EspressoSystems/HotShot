use async_std::task::{sleep, spawn};



use color_eyre::{
    eyre::{Result, WrapErr},
    Report,
};
use crossterm::event::{self, Event, KeyCode};
use flume::{Receiver, Sender};
use futures::{select, FutureExt, StreamExt};
use libp2p::PeerId;
use parking_lot::Mutex;

use std::{
    collections::{HashSet, VecDeque},
    sync::Arc,
    time::Duration,
};
use tracing::instrument;
use tui::{
    backend::Backend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
    Frame, Terminal,
};

use crate::{SwarmAction, SwarmResult, message::Message};

#[derive(Debug, Copy, Clone)]
pub enum InputMode {
    Normal,
    Editing,
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
    pub connected_peer_list: Arc<Mutex<HashSet<PeerId>>>,
    pub known_peer_list: Arc<Mutex<HashSet<PeerId>>>,
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
            connected_peer_list: Arc::new(Mutex::new(HashSet::new())),
            known_peer_list: Arc::new(Mutex::new(HashSet::new())),
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
            // force a periodic refresh
            _ = sleep(Duration::from_nanos(1)).fuse() => {}
            // swarm generated events
            swarm_msg = app.recv_swarm.recv_async() => {
                if let Ok(res) = swarm_msg {
                    match res {
                        SwarmResult::DirectMessage(m) | SwarmResult::GossipMsg(m) => app.message_buffer.lock().push_back(m),
                        SwarmResult::UpdateConnectedPeers(peer_set) => {
                            *app.connected_peer_list.lock() = peer_set.clone();
                        }
                        SwarmResult::UpdateKnownPeers(peer_set) => {
                            *app.known_peer_list.lock() = peer_set.clone();
                        }
                    }
                }
            },
            // user driven events
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
                                // right arrow key sends a direct message
                                // over a substream to an arbitrary connected peer
                                // if there are no connected peers, noop.
                                KeyCode::Right => {
                                    let mb_handle = app.message_buffer.clone();
                                    let send_swarm = app.send_swarm.clone();
                                    if let Some(selected_peer) = app.connected_peer_list.lock().iter().copied().next() {
                                        spawn(async move {
                                            let (s, r) = flume::bounded(1);
                                            send_swarm.send_async(SwarmAction::GetId(s)).await.context("")?;
                                            let msg = Message {
                                                topic: "DM".to_string(),
                                                content: app.input,
                                                sender: r.recv_async().await?.to_string(),
                                            };
                                            send_swarm.send_async(SwarmAction::DirectMessage(selected_peer, msg.clone())).await?;
                                            mb_handle.lock().push_back(msg);
                                            // if it's a duplicate message (error case), fail silently and do nothing
                                            Result::<(), Report>::Ok(())
                                        });
                                    }
                                    app.input = String::new();
                                }
                                // broadcast message to the swarm with the global topic
                                KeyCode::Enter => {
                                    let mb_handle = app.message_buffer.clone();
                                    let send_swarm = app.send_swarm.clone();
                                    // we don't want this to block the event loop
                                    spawn(async move {
                                        let (s, r) = flume::bounded(1);
                                        send_swarm.send_async(SwarmAction::GetId(s)).await.context("")?;
                                        let msg = Message {
                                            topic: "global".to_string(),
                                            content: app.input,
                                            sender: r.recv_async().await?.to_string(),
                                        };
                                        let (s, r) = flume::bounded(1);
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
            [
                Constraint::Percentage(60),
                Constraint::Percentage(30),
                Constraint::Percentage(10),
            ]
            .as_ref(),
        )
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
    let message_handle = app.message_buffer.lock(); // Lock the messages mutex
    let message_rows: Vec<Row<'_>> = message_handle
        .iter()
        .map(|message| {
            let sender = message.sender.clone();
            let content = message.content.clone();
            let height = content.chars().filter(|c| *c == '\n').count() + 1;
            let cells = vec![Cell::from(sender), Cell::from(content)];
            Ok(Row::new(cells)
                .height(u16::try_from(height).context("integer overflow calculating row")?))
        })
        .collect::<Result<Vec<Row<'_>>, Report>>()?;
    let message_table = Table::new(message_rows.into_iter())
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
        });
    f.render_stateful_widget(message_table, rects[0], &mut app.state);

    let peer_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(rects[1]);

    // TODO this is repetitive. Separate out into function.
    let peerid_handle = app.connected_peer_list.lock();
    let peerid_rows: Vec<Row<'_>> = peerid_handle
        .iter()
        .map(|peer_id| {
            let height = peer_id.to_base58().chars().filter(|c| *c == '\n').count() + 1;
            let cells = vec![Cell::from(peer_id.to_base58())];
            Ok(Row::new(cells)
                .height(u16::try_from(height).context("integer overflow calculating row")?))
        })
        .collect::<Result<Vec<Row<'_>>, Report>>()?;
    let peerid_table = Table::new(peerid_rows.into_iter())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("connected peer ids"),
        )
        .widths(&[Constraint::Percentage(100)]);
    f.render_stateful_widget(peerid_table, peer_chunks[0], &mut app.state);

    let mut known_peerid_handle = app
        .known_peer_list
        .lock()
        .iter()
        .copied()
        .collect::<Vec<PeerId>>();
    known_peerid_handle.sort();
    let known_peerid_rows: Vec<Row<'_>> = known_peerid_handle
        .iter()
        .map(|peer_id| {
            let height = peer_id.to_base58().chars().filter(|c| *c == '\n').count() + 1;
            let cells = vec![Cell::from(peer_id.to_base58())];
            Ok(Row::new(cells)
                .height(u16::try_from(height).context("integer overflow calculating row")?))
        })
        .collect::<Result<Vec<Row<'_>>, Report>>()?;
    let known_peerid_table = Table::new(known_peerid_rows.into_iter())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("known peer ids"),
        )
        .widths(&[Constraint::Percentage(100)]);
    f.render_stateful_widget(known_peerid_table, peer_chunks[1], &mut app.state);

    let input = Paragraph::new(app.input.as_ref())
        .style(match app.input_mode {
            InputMode::Normal => Style::default(),
            InputMode::Editing => Style::default().fg(Color::Yellow),
        })
        .block(Block::default().borders(Borders::ALL).title("Input"));
    f.render_widget(input, rects[2]);

    Ok(())
}


