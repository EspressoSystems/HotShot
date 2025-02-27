## How to run the benchmarks

- To run it locally, check out `crates/examples/push-cdn/README.md`.
- To run it in AWS, take a look at `scripts/benchmark_scripts/aws_ecs_benchmarks_cdn.sh` and change value of parameters as you'd like, make sure you've installed everything needed in the script and have access to needed servers (and have hotshot on those servers), then start `key-db` in one `tmux` session, in another session, enter `HotShot`, run `./scripts/benchmark_scripts/aws_ecs_benchmarks_cdn.sh [YOUR_NAME] [REMOTE_BROKER_HOST_PUBLIC_IP_1] [REMOTE_BROKER_HOST_PUBLIC_IP_2] ...`. If you want to run leaders on GPU, for the last step, run `./scripts/benchmark_scripts/aws_ecs_benchmarks_cdn_gpu.sh [YOUR_NAME] [REMOTE_GPU_HOST] [REMOTE_BROKER_HOST_PUBLIC_IP_1] [REMOTE_BROKER_HOST_PUBLIC_IP_2] ...` instead (e.g. `./scripts/benchmark_scripts/aws_ecs_benchmarks_cdn_gpu.sh sishan 11.111.223.224 3.111.223.224`).
- When running on a large group of nodes (1000 etc.), it might take too long for all nodes to post "start", you can use `manual_start` when you think there're enough nodes connected:
```
export ORCHESTRATOR_MANUAL_START_PASSWORD=password
curl -X POST http://172.31.8.82:4444/v0/api/manual_start -d 'password'
```

## How to view the results

- Three ways to gather the results
    - (recommended) check out`scripts/benchmarks_results/results.csv` on EC2, where you should have organized overall results.
    - check out Datadog under `host:/hotshot/` where you have stats for each individual validator but it's hard to track since they’re distributed.
    - wait for the output of orchestrator in local terminal, where the results are not that organized if you do multiple runs, also hard to track.

- Explanation on confusing arguments
    - `partial_results`: Whether the results are partially collected. It's set to "One" when the results are collected for one node; "HalfDA" when the results are collected for the number equals to DA_committee_number / 2; "Half" when the results are collected for half running nodes; "Full" if the results are successfully collected from all nodes. The reason is sometimes we'll get high throughput however not all the nodes can terminate successfully (I suspect the reason is that some of them fall behind when fetching large transactions).