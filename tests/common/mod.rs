#![allow(dead_code)]

use phaselock::demos::dentry::{Account, Addition, Balance, State, Subtraction, Transaction};

use std::collections::{BTreeMap, BTreeSet};
use std::env::{var, VarError};
use std::sync::Once;
use tracing_error::ErrorLayer;
use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    prelude::*,
    EnvFilter, Registry,
};

/// Provides a common starting state
pub fn get_starting_state() -> State {
    let balances: BTreeMap<Account, Balance> = vec![
        ("Joe", 1_000_000),
        ("Nathan M", 500_000),
        ("John", 400_000),
        ("Nathan Y", 600_000),
        ("Ian", 0),
    ]
    .into_iter()
    .map(|(x, y)| (x.to_string(), y))
    .collect();
    State {
        balances,
        nonces: BTreeSet::default(),
    }
}

/// generates random transactions with no regard for going negative
/// # Panics when accounts is empty
pub fn random_transactions<R: rand::Rng>(
    mut rng: &mut R,
    num_txns: usize,
    accounts: Vec<String>,
) -> Vec<Transaction> {
    use rand::seq::IteratorRandom;
    let mut txns = Vec::new();
    for _ in 0..num_txns {
        let input_account = accounts.iter().choose(&mut rng).unwrap();
        let output_account = accounts.iter().choose(&mut rng).unwrap();
        let amount = rng.gen_range(0, 100);
        txns.push(Transaction {
            add: Addition {
                account: input_account.to_string(),
                amount,
            },
            sub: Subtraction {
                account: output_account.to_string(),
                amount,
            },
            nonce: rng.gen(),
        })
    }
    txns
}

/// Provides a common list of transactions
pub fn get_ten_prebaked_trasnactions() -> Vec<Transaction> {
    vec![
        Transaction {
            add: Addition {
                account: "Ian".to_string(),
                amount: 100,
            },
            sub: Subtraction {
                account: "Joe".to_string(),
                amount: 100,
            },
            nonce: 0,
        },
        Transaction {
            add: Addition {
                account: "John".to_string(),
                amount: 25,
            },
            sub: Subtraction {
                account: "Joe".to_string(),
                amount: 25,
            },
            nonce: 1,
        },
        Transaction {
            add: Addition {
                account: "Nathan Y".to_string(),
                amount: 534044,
            },
            sub: Subtraction {
                account: "Nathan Y".to_string(),
                amount: 534044,
            },
            nonce: 2,
        },
        Transaction {
            add: Addition {
                account: "Nathan Y".to_string(),
                amount: 957954,
            },
            sub: Subtraction {
                account: "Joe".to_string(),
                amount: 957954,
            },
            nonce: 3,
        },
        Transaction {
            add: Addition {
                account: "Nathan M".to_string(),
                amount: 40,
            },
            sub: Subtraction {
                account: "Ian".to_string(),
                amount: 40,
            },
            nonce: 4,
        },
        Transaction {
            add: Addition {
                account: "John".to_string(),
                amount: 83187,
            },
            sub: Subtraction {
                account: "John".to_string(),
                amount: 83187,
            },
            nonce: 5,
        },
        Transaction {
            add: Addition {
                account: "Joe".to_string(),
                amount: 340375,
            },
            sub: Subtraction {
                account: "Nathan M".to_string(),
                amount: 340375,
            },
            nonce: 6,
        },
        Transaction {
            add: Addition {
                account: "Ian".to_string(),
                amount: 180862,
            },
            sub: Subtraction {
                account: "John".to_string(),
                amount: 180862,
            },
            nonce: 7,
        },
        Transaction {
            add: Addition {
                account: "Nathan Y".to_string(),
                amount: 373073,
            },
            sub: Subtraction {
                account: "Joe".to_string(),
                amount: 373073,
            },
            nonce: 8,
        },
        Transaction {
            add: Addition {
                account: "Nathan Y".to_string(),
                amount: 71525,
            },
            sub: Subtraction {
                account: "Nathan M".to_string(),
                amount: 71525,
            },
            nonce: 9,
        },
    ]
}

static INIT: Once = Once::new();

pub fn setup_logging() {
    INIT.call_once(|| {
            let internal_event_filter =
                match var("RUST_LOG_SPAN_EVENTS") {
                    Ok(value) => {
                        value
                            .to_ascii_lowercase()
                            .split(',')
                            .map(|filter| match filter.trim() {
                                "new" => FmtSpan::NEW,
                                "enter" => FmtSpan::ENTER,
                                "exit" => FmtSpan::EXIT,
                                "close" => FmtSpan::CLOSE,
                                "active" => FmtSpan::ACTIVE,
                                "full" => FmtSpan::FULL,
                                _ => panic!("test-env-log: RUST_LOG_SPAN_EVENTS must contain filters separated by `,`.\n\t\
                                             For example: `active` or `new,close`\n\t\
                                             Supported filters: new, enter, exit, close, active, full\n\t\
                                             Got: {}", value),
                            })
                            .fold(FmtSpan::NONE, |acc, filter| filter | acc)
                    },
                    Err(VarError::NotUnicode(_)) =>
                        panic!("test-env-log: RUST_LOG_SPAN_EVENTS must contain a valid UTF-8 string"),
                    Err(VarError::NotPresent) => FmtSpan::NONE,
                };
            let fmt_env = var("RUST_LOG_FORMAT").map(|x| x.to_lowercase());
            match fmt_env.as_deref().map(|x| x.trim()) {
                Ok("full") => {
                    let fmt_layer = fmt::Layer::default()
                        .with_span_events(internal_event_filter)
                        .with_ansi(true)
                        .with_test_writer();
                    Registry::default()
                        .with(EnvFilter::from_default_env())
                        .with(ErrorLayer::default())
                        .with(fmt_layer)
                        .init();
                },
                Ok("json") => {
                    let fmt_layer = fmt::Layer::default()
                        .with_span_events(internal_event_filter)
                        .json()
                        .with_test_writer();
                    Registry::default()
                        .with(EnvFilter::from_default_env())
                        .with(ErrorLayer::default())
                        .with(fmt_layer)
                        .init();
                },
                Ok("compact") => {
                    let fmt_layer = fmt::Layer::default()
                        .with_span_events(internal_event_filter)
                        .with_ansi(true)
                        .compact()
                        .with_test_writer();
                    Registry::default()
                        .with(EnvFilter::from_default_env())
                        .with(ErrorLayer::default())
                        .with(fmt_layer)
                        .init();
                },
                _ => {
                    let fmt_layer = fmt::Layer::default()
                        .with_span_events(internal_event_filter)
                        .with_ansi(true)
                        .pretty()
                        .with_test_writer();
                    Registry::default()
                        .with(EnvFilter::from_default_env())
                        .with(ErrorLayer::default())
                        .with(fmt_layer)
                        .init();
                },
            };
        });
}
