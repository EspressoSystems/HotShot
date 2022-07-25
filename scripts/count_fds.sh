#!/usr/bin/env bash
# USAGE: periodically print number of file descriptors in use
# will work with gnu coreutils

for i in {0..100000}
do
        echo NUM FDS: $(lsof | grep -i "tcp" | grep -i "libp2p" | tr -s ' ' | cut -d" " -f11  | sort | uniq | wc -l)
        sleep 0.2s
done
