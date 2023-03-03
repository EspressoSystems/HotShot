#!/usr/bin/env python3

# USAGE: after changing tests, generate a launch.json configuration with this script
# ./scripts/gen_launch.py > .vscode/launch.json

import json;
import copy;
import subprocess;
from typing import Final

def get_launch_json(input: str, crate: str) -> list[dict]:
    test_names = [];
    for a_line in input.splitlines():
        input_json = json.loads(a_line);
        if input_json["type"] == "test" and input_json["event"] == "started":
            test_names.append(input_json["name"]);

    json_lists = [];
    base_json = {
            "type": "lldb",
            "request": "launch",
            "cargo" : {
                "args" : [
                    "test",
                    "--no-run",
                    "--features=full-ci,channel-async-std",
                    "--package={}".format(crate),
                ],
                "filter": {
                    "name": crate,
                    "kind": "lib"
                }
            },
            "program": "${cargo:program}",
            "cwd": "${workspaceFolder}"
    };
    for test_name in test_names:
        new_test : dict = copy.deepcopy(base_json);
        new_test["name"] = "Debug {} test".format(test_name);
        new_test["cargo"]["args"].append(test_name)
        json_lists.append(new_test);
    return json_lists;


def get_crates() -> list[str]:
    return subprocess.run(["cargo-workspaces", "workspaces", "list"], stdout=subprocess.PIPE).stdout.decode().split()

def get_test_list(crate: str) -> list[dict]:
    lines_json : Final[str] = subprocess.run(["just", "list_tests_json", crate], stdout=subprocess.PIPE).stdout.decode()
    return get_launch_json(lines_json, crate)

def finalize_json(configs: list[dict]) -> str:
    total_json : Final[dict] = {
        "version": "0.2.0",
        "configurations": configs
    };
    return json.dumps(total_json, indent=4)





if __name__ == '__main__':
    crates : Final[list[str]] = get_crates();
    test_list : list[dict] = [];
    for crate in crates:
        part_of_list = get_test_list(crate)
        test_list.extend(part_of_list)
    print(finalize_json(test_list))
