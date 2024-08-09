use std::collections::HashMap;

use async_trait::async_trait;

#[derive(Eq, Hash, PartialEq, Debug, Clone)]
/// What type of delay we want to apply to
pub enum DelayOptions {
    None,
    Random,
    Fixed,
}

#[derive(Eq, Hash, PartialEq, Debug, Clone)]
/// Current implementations that are supported for testing async delays
pub enum SupportedTypes {
    Storage,
    ValidatedState,
    BlockHeader,
}

#[derive(Eq, Hash, PartialEq, Debug, Clone)]
/// Config for each supported type
pub struct DelaySettings {
    // Option to tell the async function what to do
    delay_option: DelayOptions,
    // Rng min time
    min_time: u64,
    // Rng max time
    max_time: u64,
    // Fixed time for fixed delay option
    fixed_time: u64,
}

impl Default for DelaySettings {
    fn default() -> Self {
        DelaySettings {
            delay_option: DelayOptions::None,
            min_time: 0,
            max_time: 0,
            fixed_time: 0,
        }
    }
}

#[derive(Eq, Hash, PartialEq, Debug, Clone)]
pub struct DelayConfig {
    config: HashMap<SupportedTypes, DelaySettings>,
}

impl Default for DelayConfig {
    fn default() -> Self {
        DelayConfig {
            config: HashMap::new(),
        }
    }
}

impl DelayConfig {
    pub fn new(config: HashMap<SupportedTypes, DelaySettings>) -> Self {
        DelayConfig { config }
    }

    pub fn add_setting(&mut self, supported_type: SupportedTypes, settings: DelaySettings) {
        self.config.insert(supported_type, settings);
    }

    pub fn get_setting(&self, supported_type: SupportedTypes) -> Option<&DelaySettings> {
        self.config.get(&supported_type)
    }
}

#[async_trait]
pub trait TestableDelay {
    async fn handle_delay_option(config: DelayConfig);
}
