use async_compatibility_layer::art::async_spawn;
use hotshot::traits::election::static_committee::StaticElectionConfig;
use hotshot::types::ed25519::Ed25519Pub;
use hotshot_orchestrator::config::NetworkConfig;
use hotshot_orchestrator::run_orchestrator;
use portpicker::pick_unused_port;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use surf_disco::error::ClientError;

use hotshot_orchestrator::StatisticsStruct;

#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]

async fn test_orchestrator() {
    let config = NetworkConfig::<Ed25519Pub, StaticElectionConfig>::default();
    let port = 7777; //pick_unused_port().unwrap();
    let ipaddr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
    let base_url = format!("127.0.0.1:{port}");

    async_spawn(async move {
        run_orchestrator::<Ed25519Pub, StaticElectionConfig>(config, ipaddr, port).await;
    });

    let base_url = format!("http://{base_url}").parse().unwrap();
    println!("base url: {}", base_url);
    let client = surf_disco::Client::<ClientError>::new(base_url);
    assert!(client.connect(None).await);

    // just need to test the stats (not really needed for another endpoint)
    let stat_data = StatisticsStruct {
        stat_runduration: vec![10],
        stat_throughput: vec![15],
        stat_viewtime: vec![20],
    };

    client
        .post::<()>("api/results/1")
        .body_json(&stat_data)
        .unwrap()
        .send()
        .await
        .unwrap();
    assert!(true)
}
