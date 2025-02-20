#!/usr/bin/env python3

# Utility script to show the github commit diff when a `update_flake_lock_action` PR is made.
# 
# To run, pipe the contents of the `Flake lock file updates:` into this file
#
# e.g. `cat updates.txt | ./scripts/nix_bump_pr_changes.py`
#
# The output of this script should be pasted as a reply to that PR

import sys
import re

name_commit_regex = re.compile(r"'github:([^\/]+\/[^\/]+)\/([^']+)")
prev = ''

for line in sys.stdin:
    line = line.rstrip()
    if line.startswith("    'github:"):
        prev = line
    if line.startswith("  → 'github:"):

        match = name_commit_regex.match(prev)
        if match:
            repo, start_commit = match.groups()

        match = name_commit_regex.match(line)
        if match:
            _, _, end_commit = match.groups()

        print("- [ ] " + repo + ": [repo](https://github.com/" + repo + ") | [commits this PR](https://github.com/" + repo + "/compare/" + start_commit + ".." + end_commit + ")")


