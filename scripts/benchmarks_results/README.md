## How to run the benchmarks

- To run it locally, check out `crates/examples/push-cdn/README.md`.
<!-- Sishan: add more details -->
- To run it in AWS, make sure you've installed everything needed and have access to needed AWS servers, and then `./scripts/aws_ecs_benchmarks_cdn.sh`. More details in `https://www.notion.so/espressosys/Running-Benchmarks-in-AWS-as-of-Feb-2024-fa680676053044aa8a04d5bccea0b1b4?pvs=4`.

## How to view the results

- Three ways to gather the results
    - (recommended) check out`scripts/benchmarks_results/results.csv` on EC2, where you should have organized overall results.
    - check out Datadog under `host:/hotshot/` where you have stats for each individual validator but it's hard to track since they’re distributed.
    - wait for the output of orchestrator in local terminal, where the results are not that organized if you do multiple runs, also hard to track.

- Explanation on arguments
    - `partial_results`: Whether the results are partially collected. It's set to `Partial` when the results are collected for half running nodes if not all nodes terminate successfully, `Complete` if the results are successfully collected from all nodes. The reason is sometimes we'll get high throughput however not all the nodes can terminate successfully (I suspect the reason is that some of them fall behind when fetching large transactions).