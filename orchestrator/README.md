# Orchestrator

This crate implements an orchestrator that coordinates starting the network with a particular configuration.  It is useful for testing and benchmarking.  Like the web server, the orchestrator is built on [Tide Disco](https://github.com/EspressoSystems/tide-disco).  

To run the orchestrator: `cargo run --bin orchestrator-standalone --features="bin-orchestrator" --profile=release-lto -- web 0.0.0.0 4444 ./default-run-config.toml`