use color_eyre::eyre::{eyre, Result};

fn main() -> Result<()> {
    color_eyre::install()?;
    Err(eyre!("Not implemented yet"))
}
