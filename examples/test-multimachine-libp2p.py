#!/usr/bin/env python3

from enum import Enum
from typing import Final
from subprocess import run, Popen
from os import environ
# from time import sleep

class NodeType(Enum):
    CONDUCTOR = "Conductor"
    REGULAR = "Regular"
    BOOTSTRAP = "Bootstrap"

def gen_invocation(
        node_type: NodeType,
        num_nodes: int,
        bound_addr: str,
        node_id: int,
        seed: int
    ) -> tuple[list[str], str]:
    out_file_name : Final[str] = f'out_{node_type}_{bound_addr[-4:]}';
    # disable start date. Will probably start on time locally
    START_DATE: Final[str] = "2022-09-08 17:35:00 -04:00:00";

    fmt_cmd = [
        f'cargo  run  --example=multi-machine-libp2p  --release  -- ' \
        f' --num_nodes={num_nodes} ' \
        f' --seed={seed} '\
        f' --node_idx={node_id} '\
        f' --online_time=8 '\
        f' --bound_addr={bound_addr} '\
        f' --bootstrap_mesh_n_high=50 '\
        f' --bootstrap_mesh_n_low=10 '\
        f' --num_txn_per_round=200 '\
        f' --bootstrap_mesh_outbound_min=5 '\
        f' --bootstrap_mesh_n=15 '\
        f' --mesh_n_high=15 '\
        f' --mesh_n_low=8 '\
        f' --mesh_outbound_min=4 '\
        f' --mesh_n=12 '\
        f' --next_view_timeout=5 '\
        f' --propose_min_round_time=1 '\
        f' --propose_max_round_time=3 '\
        f' --start_timestamp={START_DATE}'
    ];
    return (fmt_cmd, out_file_name)

if __name__ == "__main__":
    # cleanup from last run
    run("rm -f out_*".split())


    # params
    START_PORT : Final[int] = 9100;
    NUM_REGULAR_NODES : Final[int] = 13;
    NUM_BOOTSTRAP : Final[int] = 7;
    TOTAL_NUM_NODES: Final[int] = NUM_BOOTSTRAP + NUM_REGULAR_NODES;
    SEED: Final[int] = 1234;

    bootstrap_addrs : Final[list[str]] = list(map(lambda x: f'127.0.0.1:{x + START_PORT}', range(0, NUM_BOOTSTRAP)));

    bootstrap_addrs_str : str = f'{bootstrap_addrs[0]}';
    for addr in bootstrap_addrs[1:]:
        bootstrap_addrs_str = f'{addr},{bootstrap_addrs_str}'


    normal_nodes_addrs : Final[list[str]] = list(map(lambda x: f'127.0.0.1:{x + START_PORT + NUM_BOOTSTRAP}', range(0, NUM_REGULAR_NODES)));

    regular_cmds : list[tuple[list[str], str]] = [];
    bootstrap_cmds : list[tuple[list[str], str]] = [];

    for i in range(0, len(bootstrap_addrs)):
        bootstrap_addr = f'0.0.0.0:{bootstrap_addrs[i][-4:]}';
        print("BOUND ADDR" + bootstrap_addr)

        bootstrap_cmd = gen_invocation(
            node_type=NodeType.BOOTSTRAP,
            num_nodes=TOTAL_NUM_NODES,
            bound_addr=bootstrap_addr,
            seed=SEED,
            node_id=i
        );
        bootstrap_cmds.append(bootstrap_cmd);

    for j in range(0, len(normal_nodes_addrs)):
        regular_cmd = gen_invocation(
            node_type=NodeType.REGULAR,
            num_nodes=TOTAL_NUM_NODES,
            bound_addr=normal_nodes_addrs[j],
            node_id=j + NUM_BOOTSTRAP,
            seed=SEED
        );
        regular_cmds.append(regular_cmd);

    print(bootstrap_cmds);
    print(regular_cmds);

    # configurable in case we want the bootstrap to initialize first
    env = environ.copy();
    env["RUST_BACKTRACE"] = "full";
    env["RUST_LOG"] = "warn,error";
    env["BOOTSTRAP_ADDRS"] = bootstrap_addrs_str;

    print("spinning up bootstrap")
    for (node_cmd, file_name) in bootstrap_cmds:
        # print("running bootstrap", file_name)
        file = open(file_name, 'w')

        Popen(node_cmd[0].split("  "), start_new_session=True, stdout=file, stderr=file, env=env);

    # sleep(5);


    # print("spinning up regulars")
    for (node_cmd, file_name) in regular_cmds:
        file = open(file_name, 'w')
        # split on double spaces b/c of spaces in arguments. Yes that is janky
        print(node_cmd[0].split("  "))
        Popen(node_cmd[0].split("  "), start_new_session=True, stdout=file, stderr=file, env=env);



