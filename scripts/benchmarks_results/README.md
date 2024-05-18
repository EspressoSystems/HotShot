## How to run the benchmarks

- To run it locally, check out `crates/examples/push-cdn/README.md`.
- To run it in AWS, take a look at `scripts/aws_ecs_benchmarks_cdn.sh`, make sure you've installed everything needed in the script and have access to needed AWS servers, update `REMOTE_USER` and `REMOTE_HOST`, then start `key-db` in one `tmux` session, run `./scripts/aws_ecs_benchmarks_cdn.sh` in another session. More details in `https://www.notion.so/espressosys/Running-Benchmarks-in-AWS-as-of-Feb-2024-fa680676053044aa8a04d5bccea0b1b4?pvs=4`.
- When running on a large group of nodes (1000 etc.), it might take too long for all nodes to post "start", you can use `manual_start` when you think there're enough nodes connected:
```
export ORCHESTRATOR_MANUAL_START_PASSWORD=password
curl -X POST http://172.31.8.82:4444/v0/api/manual_start -d 'password'
```

## How to view the results

- Three ways to gather the results
    - (recommended) check out`scripts/benchmarks_results/results.csv` on EC2, where you should have organized overall results.
    - check out Datadog under `host:/hotshot/` where you have stats for each individual validator but it's hard to track since they’re distributed.
    - wait for the output of orchestrator in local terminal, where the results are not that organized if you do multiple runs, also hard to track.

- Explanation on arguments
    - `partial_results`: Whether the results are partially collected. It's set to "One" when the results are collected for one node; "HalfDA" when the results are collected for the number equals to DA_committee_number / 2; "Half" when the results are collected for half running nodes; "Full" if the results are successfully collected from all nodes. The reason is sometimes we'll get high throughput however not all the nodes can terminate successfully (I suspect the reason is that some of them fall behind when fetching large transactions).