Commands to run da examples: 
cargo run --example web-server --profile=release-lto --features="full-ci" <PORT_FOR_VALIDATING_CONSENSUS> 
cargo run --example web-server --profile=release-lto --features="full-ci" <PORT_FOR_DA> 
cargo run --example web-server --profile=release-lto --features="full-ci" <PORT_FOR_VIEW_SYNC> 
cargo run --example web-server-da-orchestrator --features="full-ci,channel-async-std" --profile=release-lto -- <ORCHESTRATOR_ADDR> <ORCHESTRATOR_PORT> <ORCHESTRATOR_CONFIG_FILE> 
cargo run --profile=release-lto --example web-server-da-validator --features="full-ci" <ORCHESTRATOR_ADDR> <ORCHESTRATOR_PORT>

I.e. 
cargo run --example web-server --profile=release-lto --features="full-ci" 9000 
cargo run --example web-server --profile=release-lto --features="full-ci" 9001 
cargo run --example web-server --profile=release-lto --features="full-ci" 9002
cargo run --example web-server-da-orchestrator --features="full-ci,channel-async-std" --profile=release-lto -- 0.0.0.0 4444 ./orchestrator/default-run-config.toml 
cargo run --profile=release-lto --example web-server-da-validator --features="full-ci" 0.0.0.0 4444