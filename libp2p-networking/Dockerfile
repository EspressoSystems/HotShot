# NOTE run from parent directory
FROM ghcr.io/espressosystems/nix-docker/nix

RUN printf "\nextra-experimental-features = flakes nix-command\n" >> /etc/nix/nix.conf

COPY . /root/phaselock

WORKDIR /root/phaselock/libp2p-networking

RUN nix develop .#staticShell -c bash -c "cargo build --release --all-features --example counter"

PUBLISH 9000

PUBLISH 8000

CMD ["nix", "develop", ".#staticShell", "-c", "bash", "-c", "cargo run --release --all-features --example counter -- --bound_addr 0.0.0.0:9000 --node_type Conductor --num_nodes 7 --bootstrap 18.224.1.60:9000 --webui 0.0.0.0:8000 --env Docker"]
