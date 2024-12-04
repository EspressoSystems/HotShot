// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use std::time::Duration;

/// The configuration for the HotShot network
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct NetworkConfig {
    /// The timeout before starting the next view
    pub next_view_timeout: u64,
    /// The timeout before starting next view sync round
    pub view_sync_timeout: Duration,
}
