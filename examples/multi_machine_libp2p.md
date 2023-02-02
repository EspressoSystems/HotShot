# Description

OOTB, runs the multi-machine demo with libp2p for networking on 12 nodes for 10 rounds with a different replica submitting a transaction each round.

# Usage

Modify variables in ./test-multimachine-libp2p.py, then run the python file.

Notable parameters are:

```python
    START_PORT : Final[int] = 9100;
    NUM_REGULAR_NODES : Final[int] = 8;
    NUM_NODES_PER_BOOTSTRAP : Final[int] = 2;
    TOTAL_NUM_NODES: Final[int] = NUM_BOOTSTRAP + NUM_REGULAR_NODES;
    NUM_ROUNDS = 10;
    SEED: Final[int] = 1234;
```
