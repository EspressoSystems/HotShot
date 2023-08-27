# License
## Copyright
**(c) 2022 Espresso Systems**.
`HotShot` was developed by Espresso Systems. While we plan to adopt an open source license, we have not yet selected one. As such, all rights are reserved for the time being. Please reach out to us if you have thoughts on licensing.
# HotShot Consensus Module

HotShot is a BFT consensus protocol based off of HotStuff, with the addition of proof-of-stake and
VRF committee elections.

## Disclaimer

**DISCLAIMER:** This software is provided "as is" and its security has not been externally audited. Use at your own risk.

# Usage

Please see the rustdoc for API documentation, and the examples directory for usage.

## Dependencies

### Unix-like

#### Nix (macos and linux)

```
nix develop
```

#### Brew (macos)

```
brew install cmake protobuf
```

#### Apt-get (linux)

```
apt-get install cmake protobuf
```

### Windows

#### Chocolatey

```
choco install cmake protoc
```

#### Scoop

```
scoop bucket add extras
scoop install protobuf cmake
```

## Building

Once dependencies have been installed, to build everything:

```sh
just async_std build
```



# Static linking

HotShot supports static linking for its examples:

```sh
# Nix-shell is optional but recommended
nix develop .#staticShell

just async_std build
```

# Testing

To test:

```sh
RUST_LOG=$ERROR_LOG_LEVEL RUST_LOG_FORMAT=$ERROR_LOG_FORMAT just async_std test
```

- `RUST_LOG=$ERROR_LOG_LEVEL`: The basic levels of logging include `warn`, `error`, `info`.
- `RUST_LOG_FORMAT=$ERROR_LOG_FORMAT`: The types of logging include `full`, `json`, and `compact`.
- Internally, the inclusion of the `--nocapture` flag indicates whether or not to output logs.
- Internally, we run at `--test-threads=1` because the tests spawn up a lot of file handles, and unix based systems consistently run out of handles.

To stress test, run the ignored tests prefixed with `test_stress`:
```sh
RUST_LOG=$ERROR_LOG_LEVEL RUST_LOG_FORMAT=$ERROR_LOG_FORMAT just async_std run_test test_stress
```

## Careful

To double check for UB:

```bash
nix develop .#correctnessShell
just async_std careful
```

## Testing on CI

To test as if running on CI, one must limit the number of cores and ram to match github runners (2 core, 7 gig ram). To limit the ram, spin up a virtual machine or container with 7 gigs ram. To limit the core count when running tests:

```
ASYNC_STD_THREAD_COUNT=1 RUST_LOG=$ERROR_LOG_LEVEL RUST_LOG_FORMAT=$ERROR_LOG_FORMAT just async_std test
ASYNC_STD_THREAD_COUNT=1 RUST_LOG=$ERROR_LOG_LEVEL RUST_LOG_FORMAT=$ERROR_LOG_FORMAT just tokio test
```

# Tokio-console

To use tokio-console, drop into the console shell:

```
nix develop .#consoleShell
```

Then, run an example.

On a separate terminal, also drop into the console shell and start tokio-console:
```
nix develop .#consoleShell -c tokio-console
```

This second window should now display task usage.

# Open Telemetry + Jaeger Integration

To view distributed logs with just the centralized server and one client, first edit the `centralized_server/orchestrator` file to include have a threshold and num_nodes of 1.

Then open 3 terminals.

```bash
# Terminal 1
# Start the jaeger instance to view spans
docker run -d -p6831:6831/udp -p6832:6832/udp -p16686:16686 -p14268:14268 jaegertracing/all-in-one:latest

# Terminal 2
# Start the CDN

# Terminal 3
# Start the client
```

# Resource Usage Statistics

To generate usage stats:
- build the test suite
- find the executable containing the test of interest
- run profiling tools

The executable `cargo` uses is shown in the output of `cargo test`.

For example, to profile `test_stress_dht_many_round`:

```bash
# bring profiling tooling like flamegraph and heaptrack into scope
nix develop .#perfShell

# show the executable we need run
# and build all test executables (required for subsequent steps)
cargo test --verbose --release --lib --bins --tests --benches --workspace -- --test-threads=1
# the output cargo test contains the tests path:
#       Running `/home/jrestivo/work/crosscross/target/release/deps/counter-880b1ff53ee21dea test_stress --test-threads=1 --ignored`
#       running 7 tests
#       test test_stress_dht_many_rounds ... ok
#       ...

# a more detailed alternative to flamegraph
# NOTE: only works on linux
heaptrack $(fd -I "counter*" -t x | rg release) --ignored -- test_stress_dht_many_round --nocapture
# palette provides memory statistics, omission will provide cpu cycle stats as colors
# NOTE: must be run as root on macos
flamegraph --palette=mem $(fd -I "counter*" -t x | rg release) --ignored -- test_stress_dht_one_round
# code coveragte statistics
cargo-llvm-cov llvm-cov --test=test_stress_dht_many_round --workspace --all-targets --release --html --output-path lcov.html
```

This will output:
- `heaptrack.counter-$HASH` which is viewable by heaptrack. This provides a plethora of useful statistics about memory and cpu cycles.
- `flamegraph.svg` which is a (moderately) less detailed version of heaptrack.
- `lcov.html` generates a summary of code coverage.

# Debugging

A debugging config file is provided for vscode and vscodium in [`.vscode/launch.json`](https://github.com/EspressoSystems/HotShot/blob/main/.cargo/config). This is intended to be used with [vadimcn/vscode-lldb](https://open-vsx.org/extension/vadimcn/vscode-lldb) but may work with other rust debuggers as well.

To bring `lldb` into scope with nix, run `nix develop .#debugShell`.

# Git Workflow

For espresso developers we have written up a description of our workflow [here](./WORKFLOW.md).

# Extra Editor Configuration

## Neovim

Copy `.nvim/example.env.lua` to `.nvim/env.lua`. The purpose of this file is to return to neovim the requisite feature flags.

Then load this file when configuring `rust-tools`:

```lua
rust_tools.setup(
  {server =
    {settings =
      {
        ["rust-analyzer"] =
          {cargo =
            {features =
              loadfile(".nvim/env.lua") and { loadfile(".nvim/env.lua")() } or "all"
            }
          }
      }
    }
  }
)
```

# Debugging

We support the [CodeLLDB Debugger](https://github.com/vadimcn/vscode-lldb).

## Neovim

Install [`dap`](https://github.com/mfussenegger/nvim-dap) and [`rust-tools`](https://github.com/simrat39/rust-tools.nvim). Install the CodeLLDB debugger listed above.
Follow the instructions [here](https://github.com/mfussenegger/nvim-dap/discussions/671#discussioncomment-4286738) to configure the adapter. To add our project-local configurations, run:

```
lua require('dap.ext.vscode').load_launchjs(nil, { ["codelldb"] = {"rust"} })
```

Finally, place a breakpoint and run `:DapContinue` to begin debugging.

NOTE: Do NOT configure dap at all with rust-tools. Do it manually.

[Example configuration](https://github.com/DieracDelta/vimconfig/blob/master/modules/lsp.nix#L280).

## Vscode

Install the extension and load the `launch.json` file. Then run the desired test target.
