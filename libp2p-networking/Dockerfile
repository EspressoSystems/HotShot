FROM nix-docker:latest

RUN printf "\nextra-experimental-features = flakes nix-command\n" >> /etc/nix/nix.conf 

WORKDIR /root

COPY ./phaselock /root/phaselock

WORKDIR /root/phaselock/libp2p-networking

# take advantage of caching?
RUN nix develop .#staticShell

RUN nix develop .#staticShell -c bash -c "cargo build --release --all-features --example counter"

CMD ["nix", "develop", ".#staticShell", "-c", "bash", "-c", "cargo run --release --all-features --example counter -- --bound_addr 0.0.0.0:9000 --node_type Conductor --num_nodes 7 --bootstrap 18.224.1.60:9000 --webui 0.0.0.0:8000"]

