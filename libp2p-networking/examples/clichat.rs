use color_eyre::eyre::{Result, WrapErr};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use libp2p::Multiaddr;
use networking_demo::{
    message::Message,
    network::{gen_multiaddr, ClientRequest, NetworkNode, NetworkNodeConfigBuilder},
    ui::{run_app, TableApp},
};
use parking_lot::Mutex;
use std::{collections::VecDeque, sync::Arc};
use structopt::StructOpt;
use tracing::instrument;
use tui::{backend::CrosstermBackend, Terminal};

/// command line arguments
#[derive(StructOpt)]
struct CliOpt {
    /// Path to the node configuration file
    #[structopt(long = "port", short = "p")]
    port: Option<u16>,

    #[structopt()]
    first_dial_addr: Option<Multiaddr>,
}

#[async_std::main]
#[instrument]
async fn main() -> Result<()> {
    // -- Setup color_eyre and tracing
    color_eyre::install()?;
    networking_demo::tracing_setup::setup_tracing();
    // -- Spin up the network connection
    let networking_config = NetworkNodeConfigBuilder::default()
        .min_num_peers(10usize)
        .max_num_peers(15usize)
        .build()?;
    let mut networking: NetworkNode = NetworkNode::new(networking_config)
        .await
        .context("Failed to launch network")?;
    let port = CliOpt::from_args().port.unwrap_or(0u16);
    let known_peer = CliOpt::from_args().first_dial_addr;
    let peer_list = if let Some(p) = known_peer {
        vec![(None, p)]
    } else {
        Vec::new()
    };
    let listen_addr = gen_multiaddr(port);
    networking.start_listen(listen_addr).await?;
    networking.add_known_peers(peer_list.as_slice()).await;
    let (send_chan, recv_chan) = networking.spawn_listeners().await?;

    // -- Spin up the UI
    // Setup a ring buffer to hold messages, 25 of them should do for the demo
    let message_buffer: Arc<Mutex<VecDeque<Message>>> = Arc::new(Mutex::new(VecDeque::new()));
    // Put a few dummy messages in there so we can display something
    let mut buffer_handle = message_buffer.lock();
    buffer_handle.push_back(Message {
        sender: "Nathan".to_string(),
        content: "Hello".to_string(),
        topic: "Generated".to_string(),
    });
    buffer_handle.push_back(Message {
        sender: "Justin".to_string(),
        content: "hi!".to_string(),
        topic: "Generated".to_string(),
    });
    buffer_handle.push_back(Message {
        sender: "Joe".to_string(),
        content: "test".to_string(),
        topic: "Generated".to_string(),
    });
    buffer_handle.push_back(Message {
        sender: "John".to_string(),
        content: "test 2".to_string(),
        topic: "Generated".to_string(),
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
        .send(ClientRequest::Subscribe("global".to_string()))?;
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
