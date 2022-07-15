#!/usr/bin/env python3

from enum import Enum
from functools import reduce
from typing import Final
from subprocess import run, Popen
from time import sleep
from os import environ

class NodeType(Enum):
    CONDUCTOR = "Conductor"
    REGULAR = "Regular"
    BOOTSTRAP = "Bootstrap"

def gen_invocation(
        node_type: NodeType,
        num_nodes: int,
        to_connect_addrs: list[str],
        bound_addr: str,
        node_id: int,
        seed: int
    ) -> tuple[list[str], str]:
    out_file_name : Final[str] = f'out_{node_type}_{bound_addr[-4:]}';
    fmt_cmd = [
        f'cargo run --example=multi-machine-libp2p --release -- ' \
        f' --num_nodes={num_nodes} ' \
        f' --seed={seed} '\
        f' --num_bootstrap={len(to_connect_addrs)} '\
        f' --num_txn_per_round=10 '\
        f' --node_idx={node_id} '\
        f' --online_time=1 '\
        f' --bound_addr={bound_addr} '
    ];
    return (fmt_cmd, out_file_name)

if __name__ == "__main__":
    # cleanup from last run
    run("rm -f out_*".split())


    # params
    START_PORT : Final[int] = 9100;
    NUM_REGULAR_NODES : Final[int] = 20;
    NUM_BOOTSTRAP : Final[int] = 7;
    TOTAL_NUM_NODES: Final[int] = NUM_BOOTSTRAP + NUM_REGULAR_NODES;
    SEED: Final[int] = 1234;

    bootstrap_addrs : Final[list[str]] = list(map(lambda x: f'127.0.0.1:{x + START_PORT}', range(0, NUM_BOOTSTRAP)));
    normal_nodes_addrs : Final[list[str]] = list(map(lambda x: f'127.0.0.1:{x + START_PORT + NUM_BOOTSTRAP}', range(0, NUM_REGULAR_NODES)));

    regular_cmds : list[tuple[list[str], str]] = [];
    bootstrap_cmds : list[tuple[list[str], str]] = [];

    for i in range(0, len(bootstrap_addrs)):
        bootstrap_addr = f'0.0.0.0:{bootstrap_addrs[i][-4:]}';
        print("BOUND ADDR" + bootstrap_addr)

        bootstrap_cmd = gen_invocation(
            node_type=NodeType.BOOTSTRAP,
            num_nodes=TOTAL_NUM_NODES,
            to_connect_addrs=bootstrap_addrs,
            bound_addr=bootstrap_addr,
            seed=SEED,
            node_id=i
        );
        bootstrap_cmds.append(bootstrap_cmd);

    for j in range(0, len(normal_nodes_addrs)):
        regular_cmd = gen_invocation(
            node_type=NodeType.REGULAR,
            num_nodes=TOTAL_NUM_NODES,
            to_connect_addrs=bootstrap_addrs,
            bound_addr=normal_nodes_addrs[j],
            node_id=j + NUM_BOOTSTRAP,
            seed=SEED
        );
        regular_cmds.append(regular_cmd);

    print(bootstrap_cmds)
    print(regular_cmds)

    # configurable in case we want the bootstrap to initialize first
    env = environ.copy();
    env["RUST_BACKTRACE"] = "full"
    env["RUST_LOG"] = "warn,error"

    print("spinning up bootstrap")
    for (node_cmd, file_name) in bootstrap_cmds:
        print("running bootstrap", file_name)
        file = open(file_name, 'w')
        Popen(node_cmd[0].split(), start_new_session=True, stdout=file, stderr=file, env=env);


    print("spinning up regulars")
    for (node_cmd, file_name) in regular_cmds:
        file = open(file_name, 'w')
        Popen(node_cmd[0].split(), start_new_session=True, stdout=file, stderr=file, env=env);

    # sleep(TIME_TO_SPIN_UP_REGULAR);


