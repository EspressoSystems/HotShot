use std::sync::Once;

use tracing_subscriber::EnvFilter;

static LOGGING_INITIALIZED: Once = Once::new();

pub fn setup_logging() {
    LOGGING_INITIALIZED.call_once(|| {
        // Initialize tracing
        if std::env::var("RUST_LOG_FORMAT") == Ok("json".to_string()) {
            tracing_subscriber::fmt()
                .with_env_filter(EnvFilter::from_default_env())
                .json()
                .init();
        } else {
            tracing_subscriber::fmt()
                .with_env_filter(EnvFilter::from_default_env())
                .init();
        }
    });
}
