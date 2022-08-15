# Centralized server

Even though HotShot is a decentralized system, sometimes it is useful to start up a centralized server and monitor all network traffic. This crate provides such a server.

To start this server, cd into `bin` and run it with `cargo run -- --host 0.0.0.0 --port 7000`. In `HotShot` initialize the `CentralizedServerNetworking` that connects to the host and port configured.

