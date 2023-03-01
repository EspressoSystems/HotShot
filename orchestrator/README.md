# Orchestrator

This crate implements an orchestrator that coordinates starting the network with a particular configuration.  It is useful for testing and benchmarking.  Like the web server, the orchestrator is built on [Tide Disco](https://github.com/EspressoSystems/tide-disco).  

To run the orchestrator: `cargo run --example web-server-orchestrator --features="full-ci,channel-async-std" --profile=release-lto -- web 0.0.0.0 4444 ./orchestrator/default-run-config.toml`