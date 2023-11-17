# Orchestrator

This crate implements an orchestrator that coordinates starting the network with a particular configuration.  It is useful for testing and benchmarking.  Like the web server, the orchestrator is built using [Tide Disco](https://github.com/EspressoSystems/tide-disco).  

To run the orchestrator for a libp2p network: `just async_std example orchestrator-libp2p 0.0.0.0 3333 ./crates/orchestrator/run-config`

To run the orchestrator for a libp2p network: `just async_std example orchestrator-webserver 0.0.0.0 3333 ./crates/orchestrator/run-config.toml `