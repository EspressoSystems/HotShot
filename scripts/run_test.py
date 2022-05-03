#!/usr/bin/env python3

# Runs a given test, and splits the output based on the "id: x" text in each block.
#
# Will generate an `out.txt` file with all output, including "id: x" messages.
# For every `id: x`, an associated `out_x.txt` is generated, and only the relevant entries are logged.
#
# Usage:
# In /:
# `./scripts/run_test.py sync_newest_quorom`
# In /libp2p-networking/:
# `../scripts/run_test.py test_coverage_request_response_one_round`

import os
import sys
import re
import subprocess

test = ""
if len(sys.argv) >= 2:
    test = sys.argv[1]
else:
    test = input("Enter a test name: ")

env = os.environ
env["RUST_LOG_FMT"] = "compact"
env["RUST_LOG"] = "debug"
env["RUST_BACKTRACE"] = "1"
result = subprocess.run(
    "cargo test --all-features --release -- " + test + " --test-threads=1 --nocapture",
    shell=True,
    executable='bash',
    stdout=subprocess.PIPE,
    env=env
)

id_regex = re.compile("id: (\d+)")
ansi_escape = re.compile(r'(\x9B|\x1B\[)[0-?]*[ -\/]*[@-~]')

files = []
files.append(open("out.txt", "w"))

block = ""
for line in result.stdout.decode("utf-8").split("\n"):
    block += ansi_escape.sub('', line) + "\n"
    if len(line) == 0:
        files[0].write(block)
        match = id_regex.findall(block)

        if match:
            id = -1
            # Find the last valid id in `match`
            for new_id in match:
                new_id = int(new_id)
                if new_id < 100:
                    id = new_id

            if id >= 0 and id < 100:
                file_idx = id + 1
                while len(files) <= file_idx:
                    files.append(open("out_" + str(len(files) - 1) + ".txt", "w"))
                files[file_idx].write(block)

        block = ""
if block:
    files[0].write(block)