KeyDB is the ephemeral database, it's like Redis but with extra features. The only thing we run it with is with `--requirepass` to set a password.

**Marshals:**
The marshal is the entry point of the push CDN, all users connect there first. It tells users which broker to connect to.

- `-d` is the "discovery endpoint", which in this case is the URL of KeyDB.
- `-b` is the bind port. This is what you would set in run_config.toml for cdn_broker_marshal_endpoint
- `-m` is metrics stuff. You shouldn't have to use that


**Brokers:**
In a run with multiple machines, we want two brokers. With one machine, it's probably fine to do one broker. These are what route the messages. Here are the relevant command line arguments:

- `-d` is the "discovery endpoint", which in this case is the URL of KeyDB.
- `--public-bind-endpoint`: the endpoint which we bind to locally for users to connect to (e.g. 0.0.0.0:1740)
- `--public-advertise-endpoint`: the endpoint which we advertise to users (e.g. my.public.ip:1740)
- `--private-bind-endpoint`: the endpoint which we bind to locally for brokers to connect to (e.g. 0.0.0.0:1741)
- `--private-advertise-endpoint`: the endpoint which we advertise to brokers (e.g. my.public.ip:1741)
- `-m` is metrics stuff. You shouldn't have to use that
For brokers, there is a magic value called `local_ip`. This resolves to the local IP address, which skips the need for talking to the AWS metadata server. For in-AWS uses, the following configuration is probably fine:
`cdn-broker --public-bind-endpoint 0.0.0.0:1740 --public-advertise-endpoint local_ip:1740 --private-bind-endpoint 0.0.0.0:1741 --private-advertise-endpoint local_ip:1741`. You won't need to put this port or values anywhere, as the marshal does everything for you.

**Examples:**

`just async_std example all-push-cdn -- --config_file ./crates/orchestrator/run-config.toml`

OR

```
just async_std example orchestrator-webserver -- --config_file ./crates/orchestrator/run-config.toml --orchestrator_url http://127.0.0.1:4444
just async_std cdn-broker -d "test.sqlite" --public-bind-endpoint 0.0.0.0:1740 --public-advertise-endpoint local_ip:1740 --private-bind-endpoint 0.0.0.0:1741 --private-advertise-endpoint local_ip:1741
just async_std cdn-marshal -d "test.sqlite" -b 127.0.0.1:9000
just async_std validator-push-cdn -- 10 http://127.0.0.1:4444
```