use async_std::fs::File;
use futures::AsyncReadExt;
use libp2p::identity::Keypair;
use networking_demo::parse_config::NodeDescription;
use tracing::instrument;

#[async_std::main]
#[instrument]
async fn main() {
    parse_ids().await;
}

pub async fn parse_ids() {
    let mut f = File::open(&"./identity_mapping.json").await.unwrap();
    let mut s = String::new();
    f.read_to_string(&mut s).await.unwrap();
    println!("s{}", s);
    let _: Vec<NodeDescription> = serde_json::from_str(&s).unwrap();
}

pub async fn gen_ids() {
    for _i in 0..10 {
        let identity = Keypair::generate_ed25519();
        let pbuf_encoding = identity.to_protobuf_encoding().unwrap();
        println!("{:?}", pbuf_encoding);
    }
}
