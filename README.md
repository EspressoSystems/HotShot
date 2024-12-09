![GitHub Release](https://img.shields.io/github/v/release/EspressoSystems/HotShot)

# HotShot <img src="hotshot-emoji.png" alt="HotShot Emoji" height="40" style="vertical-align: middle;">

HotShot is a Byzantine Fault Tolerant (BFT) consensus protocol that builds upon HotStuff 2. It is modified for proof-of-stake settings and features a linear view-synchronization protocol and a data-availability layer.

## Paper
The HotShot protocol is described in the [Espresso Sequencing Network paper](https://eprint.iacr.org/2024/1189.pdf).

## Usage and Examples

Usage examples are provided in the [examples directory](./crates/examples).

### Running the examples
To run a full example network, use the command
```bash
RUST_LOG=info cargo run --example all
```

You can see the list of supported command-line arguments by running
```bash
cargo run --example all -- --help
```

## Audits
The HotShot protocol has been internally audited. The report is available [here](./audits/internal-reviews/EspressoHotshot-2024internal.pdf).

## Disclaimer

**DISCLAIMER:** This software is provided "as is" and its security has not been **externally** audited. Use at your own risk.

