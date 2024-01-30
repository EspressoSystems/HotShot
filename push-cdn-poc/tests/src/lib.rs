#[cfg(test)]
mod test {
    use std::sync::Arc;

    use client::{Client, Config as ClientConfig};
    use common::crypto;
    use common::schema::Message;
    use common::{rng::DeterministicRng, topic::Topic};
    use jf_primitives::signatures::bls_over_bn254::{
        BLSOverBN254CurveSignatureScheme as BLS, SignKey, VerKey,
    };
    use jf_primitives::signatures::SignatureScheme;
    use rand::seq::SliceRandom;
    use server::{Config as ServerConfig, Server};
    use tokio::spawn;

    struct ClientDefinition {
        initial_topics: Vec<Topic>,
    }

    // struct ServerDefinition {}

    struct TestDefinition {
        clients: Vec<ClientDefinition>,
        key_generation_type: KeyGenerationType,
        num_servers: u64,
    }

    enum KeyGenerationType {
        Random,
        Same,
    }

    fn do_vecs_match<T: PartialEq>(a: &[T], b: &[T]) -> bool {
        b.iter().all(|item| a.contains(item))
    }

    fn key_from_seed(seed: u64) -> (SignKey, VerKey) {
        let mut prng = DeterministicRng(seed);
        BLS::key_gen(&(), &mut prng).unwrap()
    }

    async fn setup(definition: TestDefinition) -> (Vec<(Client, usize)>, Vec<Server>) {
        let mut clients = Vec::new();
        let mut servers = Vec::new();
        let mut server_ports = Vec::new();

        for _ in 0..definition.num_servers {
            let port = portpicker::pick_unused_port().unwrap();
            server_ports.push(port);

            let server = Server::new(ServerConfig {
                bind_address: format!("0.0.0.0:{port}"),
                public_advertise_address: format!("0.0.0.0:{port}"),
                private_advertise_address: format!("0.0.0.0:{port}"),
                redis_url: "redis://127.0.0.1:6379".to_string(),
                signing_key: None,
                redis_password: "".to_string(),
                tls_cert_path: None,
                tls_key_path: None,
            })
            .unwrap();

            servers.push(server);
        }

        for (index, def) in definition.clients.iter().enumerate() {
            let (signing_key, verify_key) = match definition.key_generation_type {
                KeyGenerationType::Same => key_from_seed(0),
                KeyGenerationType::Random => key_from_seed(index as u64),
            };

            let client = Client::new(ClientConfig {
                // random broker
                broker_address: format!(
                    "127.0.0.1:{}",
                    server_ports.choose(&mut rand::thread_rng()).unwrap()
                ),
                initial_topics: def.initial_topics.clone(),
                signing_key,
                verify_key,
            })
            .unwrap();

            clients.push((client, index));
        }

        (clients, servers)
    }

    #[tokio::test]
    async fn test_direct() {
        let (mut clients, servers) = setup(TestDefinition {
            key_generation_type: KeyGenerationType::Random,
            clients: vec![
                ClientDefinition {
                    initial_topics: Vec::from([Topic::Global]),
                },
                ClientDefinition {
                    initial_topics: Vec::from([Topic::Global]),
                },
                ClientDefinition {
                    initial_topics: Vec::from([Topic::Global]),
                },
                ClientDefinition {
                    initial_topics: Vec::from([Topic::Global]),
                },
                ClientDefinition {
                    initial_topics: Vec::from([Topic::Global]),
                },
                ClientDefinition {
                    initial_topics: Vec::from([Topic::Global]),
                },
                ClientDefinition {
                    initial_topics: Vec::from([Topic::Global]),
                },
            ],
            num_servers: 1,
        })
        .await;

        // start servers
        for server in servers.into_iter() {
            spawn(server.run());
        }

        let num_clients = clients.len();
        let (first_client, _) = clients.remove(0);
        let mut client_jh = Vec::new();

        for (client, index) in clients.into_iter() {
            client.wait_connect().await;
            client_jh.push(spawn(async move {
                let message = client.recv_message().await;
                assert_eq!(message, Message::Data((index).to_be_bytes().to_vec()));
            }));
        }

        client_jh.push(spawn(async move {
            first_client.wait_connect().await;

            for client in 1 as u64..num_clients as u64 {
                let recipient = key_from_seed(client as u64).1;
                let message = Arc::new(Message::Direct(
                    crypto::serialize(&recipient).unwrap(),
                    client.to_be_bytes().to_vec(),
                ));
                first_client.send_message(message).await;
            }
        }));

        for jh in client_jh.into_iter() {
            jh.await.unwrap();
        }
    }

    #[tokio::test]
    async fn test_broadcast() {
        let (mut clients, servers) = setup(TestDefinition {
            key_generation_type: KeyGenerationType::Random,
            clients: vec![
                ClientDefinition {
                    initial_topics: Vec::from([Topic::Global]),
                },
                ClientDefinition {
                    initial_topics: Vec::from([Topic::DA]),
                },
                ClientDefinition {
                    initial_topics: Vec::from([Topic::Global, Topic::DA]),
                },
            ],
            num_servers: 1,
        })
        .await;

        // start servers
        for server in servers.into_iter() {
            spawn(server.run());
        }

        let (global, _) = clients.remove(0);
        let (da, _) = clients.remove(0);
        let (both, _) = clients.remove(0);

        global.wait_connect().await;
        da.wait_connect().await;
        both.wait_connect().await;

        let mut client_jh = Vec::new();

        client_jh.push(spawn(async move {
            global
                .send_message(Arc::new(Message::Broadcast(
                    Vec::from([Topic::Global]),
                    vec![0],
                )))
                .await;

            let mut received_messages = Vec::new();
            for _ in 0..2 {
                let message = global.recv_message().await;
                received_messages.push(message);
            }

            assert_eq!(
                do_vecs_match(
                    &received_messages,
                    &vec![Message::Data(vec![0]), Message::Data(vec![2])]
                ),
                true
            );
        }));

        client_jh.push(spawn(async move {
            da.send_message(Arc::new(Message::Broadcast(
                Vec::from([Topic::DA]),
                vec![1],
            )))
            .await;

            let mut received_messages = Vec::new();
            for _ in 0..2 {
                let message = da.recv_message().await;
                received_messages.push(message);
            }

            assert_eq!(
                do_vecs_match(
                    &received_messages,
                    &vec![Message::Data(vec![1]), Message::Data(vec![2])]
                ),
                true
            );
        }));

        client_jh.push(spawn(async move {
            both.send_message(Arc::new(Message::Broadcast(
                Vec::from([Topic::Global, Topic::DA]),
                vec![2],
            )))
            .await;

            let mut received_messages = Vec::new();
            for _ in 0..3 {
                let message = both.recv_message().await;
                received_messages.push(message);
            }

            assert_eq!(
                do_vecs_match(
                    &received_messages,
                    &vec![
                        Message::Data(vec![1]),
                        Message::Data(vec![2]),
                        Message::Data(vec![0])
                    ]
                ),
                true
            );
        }));

        for jh in client_jh.into_iter() {
            jh.await.unwrap();
        }
    }
}
