use crate::BrokerError;
use common::{err, error::Result};
use redis::Client as RedisClientInner;
use std::{net::SocketAddr, time::Duration};

#[derive(Clone)]
pub struct RedisConfig {
    pub redis_url: String,
    pub redis_password: String,

    pub public_advertise_address: SocketAddr,
    pub private_advertise_address: SocketAddr,

    pub record_expiry: Duration,
}

#[derive(Clone)]
pub struct RedisClient {
    client: RedisClientInner,
    public_advertise_address: SocketAddr,
    private_advertise_address: SocketAddr,
    record_expiry: Duration,
}

impl RedisClient {
    pub fn new(config: RedisConfig) -> Result<Self> {
        // extrapolate config
        let RedisConfig {
            redis_url,
            redis_password,
            public_advertise_address,
            private_advertise_address,
            record_expiry,
        } = config;

        // create Redis client
        let client = err!(
            redis::Client::open(redis_url),
            ParseError,
            "failed to parse Redis address"
        )?;

        // spawn register task
        Ok(Self {
            client,
            public_advertise_address,
            private_advertise_address,
            record_expiry,
        })
    }

    pub async fn _retrieve_current_brokers(&self) -> Result<Vec<SocketAddr>> {
        Ok(vec![])
    }

    pub async fn register(&self) -> Result<()> {
        Ok(())
    }
}
