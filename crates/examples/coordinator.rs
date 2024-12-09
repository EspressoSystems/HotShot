//! This service helps coordinate running multiple nodes where each needs
//! to be assigned a unique index
//!
//! This is meant to be run externally, e.g. when running benchmarks on the protocol.
//! If you just want to run everything required, you can use the `all` example

use std::{
    collections::HashSet,
    net::SocketAddr,
    str::FromStr,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
};

use anyhow::{Context, Result};
use bytes::Bytes;
use clap::Parser;
use hotshot::helpers::initialize_logging;
use libp2p::{multiaddr::Protocol, Multiaddr};
use parking_lot::RwLock;
use warp::Filter;

#[derive(Parser)]
struct Args {
    /// The address to bind to
    #[arg(long, default_value = "127.0.0.1:3030")]
    bind_address: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    initialize_logging();

    // Parse the command-line arguments
    let args = Args::parse();

    // Parse the bind address
    let bind_address = args
        .bind_address
        .parse::<SocketAddr>()
        .with_context(|| "Failed to parse bind address")?;

    // Create a shared counter
    let counter = Arc::new(AtomicU32::new(0));
    let counter = warp::any().map(move || counter.clone());

    // Create a shared set of multiaddrs for Libp2p
    let libp2p_multiaddrs = Arc::new(RwLock::new(HashSet::new()));
    let libp2p_multiaddrs = warp::any().map(move || libp2p_multiaddrs.clone());

    // `/index` returns the node index we are assigned
    let index = warp::path!("index")
        .and(counter.clone())
        .map(|counter: Arc<AtomicU32>| counter.fetch_add(1, Ordering::SeqCst).to_string());

    // POST `/libp2p_info` submits libp2p information to the coordinator
    let submit_libp2p_info = warp::path!("libp2p-info")
        .and(warp::post())
        .and(warp::body::bytes())
        .and(libp2p_multiaddrs.clone())
        .map(
            |body: Bytes, libp2p_multiaddrs: Arc<RwLock<HashSet<Multiaddr>>>| {
                // Attempt to process as a string
                let Ok(string) = String::from_utf8(body.to_vec()) else {
                    return "Failed to parse body as string".to_string();
                };

                // Attempt to parse the string as a Libp2p Multiaddr
                let Ok(mut multiaddr) = Multiaddr::from_str(&string) else {
                    return "Failed to parse body as Multiaddr".to_string();
                };

                // Pop off the last protocol
                let Some(last_protocol) = multiaddr.pop() else {
                    return "Failed to get last protocol of multiaddr".to_string();
                };

                // Make sure it is the P2p protocol
                let Protocol::P2p(_) = last_protocol else {
                    return "Failed to get P2p protocol of multiaddr".to_string();
                };

                // Add it to the set
                libp2p_multiaddrs.write().insert(multiaddr);

                "Ok".to_string()
            },
        );

    // GET `/libp2p_info` returns the list of libp2p multiaddrs
    let get_libp2p_info = warp::path!("libp2p-info")
        .and(libp2p_multiaddrs.clone())
        .map(|libp2p_multiaddrs: Arc<RwLock<HashSet<Multiaddr>>>| {
            // Get the list of multiaddrs
            let multiaddrs = libp2p_multiaddrs.read().clone();

            // Convert the multiaddrs to a string, separated by newlines
            multiaddrs
                .iter()
                .map(|m| m.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        });

    // `/reset` resets the state of the coordinator
    let reset = warp::path!("reset")
        .and(counter)
        .and(libp2p_multiaddrs)
        .map(
            |counter: Arc<AtomicU32>, libp2p_multiaddrs: Arc<RwLock<HashSet<Multiaddr>>>| {
                counter.store(0, Ordering::SeqCst);
                libp2p_multiaddrs.write().clear();
                "Ok"
            },
        );

    // Run the server
    warp::serve(
        index
            .or(reset)
            .or(submit_libp2p_info)
            .or(get_libp2p_info),
    )
    .run(bind_address)
    .await;

    Ok(())
}
