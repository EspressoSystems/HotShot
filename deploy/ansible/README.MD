## Centralized Server Deployment

The `centralized-server` Ansible playbook will deploy the centralized server binary to a designated host machine.  See [here](../../centralized_server/README.md) for more information about what the centralized server does.  

Use the following steps to run this playbook: 
1. Update the `config.toml` file in this directory if you wish to change the parameters of the server.  Some parameters you may wish to change are `transactions_per_round`, `padding`, `total_nodes`, etc. 

2. Update variables at the top of `centralized-server.yaml` under the `vars` section.  In particular, be sure to update your user that will ssh into the host.

3. If necessary, update `./inventories/centralized-server.yaml` to point to the correct host for the server.

4. From this directory, run `ansible-playbook -i inventories/centralized-server.yaml centralized-server.yaml`.

## Libp2p Deployment

TODO