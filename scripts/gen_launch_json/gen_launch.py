#!/usr/bin/env python3

# USAGE: after changing tests, generate a launch.json configuration
# just list_tests_json >> tests.txt && cat tests.txt | ./scripts/gen_launch_json/gen_launch.py > .vscode/launch.json

import json;
import sys;
import copy;

def gen_launch_json(input) -> str:

    test_names = [];
    for a_line in input:
        input_json = json.loads(a_line);
        if input_json["type"] == "test" and input_json["event"] == "started":
            test_names.append(input_json["name"]);

    json_lists = [];
    base_json = {
            "type": "codelldb",
            "request": "launch",
            "cargo" : {
                "args" : [
                    "test",
                    "--no-run",
                    "--package=hotshot-testing",
                    "--features=full-ci,channel-async-std"
                ],
                "filter": {
                    "name": "hotshot-testing",
                    "kind": "lib"
                }
            },
            "program": "${cargo:program}",
            "cwd": "${workspaceFolder}"
    };
    for test_name in test_names:
        new_test = copy.deepcopy(base_json);
        new_test["name"] = "Debug {} test".format(test_name);
        # new_test["cargo"]["args"].append("--test")
        new_test["cargo"]["args"].append(test_name)
        json_lists.append(new_test);
    total_json = {
        "version": "0.2.0",
        "configurations": json_lists
    };
    return json.dumps(total_json, indent=4);





if __name__ == '__main__':
    print(gen_launch_json(sys.stdin));
