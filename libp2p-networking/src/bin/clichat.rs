use std::collections::VecDeque;

use color_eyre::eyre::{eyre, Result};

#[derive(Debug, Clone)]
pub struct Message {
    sender: String,
    content: String,
}

fn main() -> Result<()> {
    color_eyre::install()?;
    // Setup a ring buffer to hold messages, 25 of them should do for the demo
    let mut buffer: VecDeque<Message> = VecDeque::new();
    // Put a few dummy messages in there so we can display something
    buffer.push_back(Message {
        sender: "Nathan".to_string(),
        content: "Hello".to_string(),
    });
    buffer.push_back(Message {
        sender: "Justin".to_string(),
        content: "hi!".to_string(),
    });

    Err(eyre!("Not implemented yet"))
}
