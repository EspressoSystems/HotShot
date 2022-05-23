#!/usr/bin/env python3

# Split the stdin into separate files, based on the "id: x" text in each block.
#
# Will generate an `out.txt` file with all output, including "id: x" messages.
# For every `id: x`, an associated `out_x.txt` is generated, and only the relevant entries are logged.
#
# Usage: `cat <file> | ./scripts/split_test_output.py`

import sys
import os
import re

id_regex = re.compile("id: (\d+)")
ansi_escape = re.compile(r'(\x9B|\x1B\[)[0-?]*[ -\/]*[@-~]')

def split_input(input):
    files = []
    files.append(open("out.txt", "w"))

    block = ""
    for line in input:
        line = line.rstrip()
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

if __name__ == '__main__':
    split_input(sys.stdin)