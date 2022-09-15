First, log into `0.us-east-2.cluster.aws.espresso.network` (ask Matt for credentials)

# Deploying a branch

This will set the current branch to `benchmarking_libp2p_orchestrator`:

`ecs deploy Main hotshot_0 --tag benchmarking_libp2p_orchestrator`

# Running the clients 

`ecs scale --region us-east-2 Main hotshot_0 <num> --timeout -1`

Make sure to set this to 0 after you're done

