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
    pub delay_option: DelayOptions,
    // Rng min time
    pub min_time_in_milliseconds: u64,
    // Rng max time
    pub max_time_in_milliseconds: u64,
    // Fixed time for fixed delay option
    pub fixed_time_in_milliseconds: u64,
}

impl Default for DelaySettings {
    fn default() -> Self {
        DelaySettings {
            delay_option: DelayOptions::None,
            min_time_in_milliseconds: 0,
            max_time_in_milliseconds: 0,
            fixed_time_in_milliseconds: 0,
        }
    }
}

#[derive(Eq, PartialEq, Debug, Clone, Default)]
/// Settings for each type
pub struct DelayConfig {
    config: HashMap<SupportedTypes, DelaySettings>,
}

impl DelayConfig {
    pub fn new(config: HashMap<SupportedTypes, DelaySettings>) -> Self {
        DelayConfig { config }
    }

    pub fn add_settings_for_all_types(&mut self, settings: DelaySettings) {
        let iterator = SupportedTypesIterator::new();

        for supported_type in iterator {
            self.config.insert(supported_type, settings.clone());
        }
    }

    pub fn add_setting(&mut self, supported_type: SupportedTypes, settings: DelaySettings) {
        self.config.insert(supported_type, settings);
    }

    pub fn get_setting(&self, supported_type: SupportedTypes) -> Option<&DelaySettings> {
        self.config.get(&supported_type)
    }
}

#[async_trait]
/// Implement this method to add some delay to async call
pub trait TestableDelay {
    async fn handle_async_delay(config: DelayConfig);
}

/// Iterator to iterate over enum
struct SupportedTypesIterator {
    index: usize,
}

impl SupportedTypesIterator {
    fn new() -> Self {
        SupportedTypesIterator { index: 0 }
    }
}

impl Iterator for SupportedTypesIterator {
    type Item = SupportedTypes;

    fn next(&mut self) -> Option<Self::Item> {
        let supported_type = match self.index {
            0 => Some(SupportedTypes::Storage),
            1 => Some(SupportedTypes::ValidatedState),
            2 => Some(SupportedTypes::BlockHeader),
            _ => {
                assert_eq!(self.index, 3, "Need to ensure that newly added or removed `SupportedTypes` enum is handled in iterator");
                return None;
            }
        };
        self.index += 1;
        supported_type
    }
}
