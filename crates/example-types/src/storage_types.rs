// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
    time::Duration,
};

use anyhow::{bail, Result};
use async_compatibility_layer::art::async_sleep;
use async_lock::RwLock;
use async_trait::async_trait;
use hotshot_types::{
    consensus::CommitmentMap,
    data::{DaProposal, Leaf, QuorumProposal, VidDisperseShare},
    message::Proposal,
    simple_certificate::QuorumCertificate,
    traits::{node_implementation::NodeType, storage::Storage},
    utils::View,
    vote::HasViewNumber,
};
use rand::Rng;

use crate::testable_delay::{DelayConfig, DelayOptions, SupportedTypes, TestableDelay};

type VidShares<TYPES> = HashMap<
    <TYPES as NodeType>::Time,
    HashMap<<TYPES as NodeType>::SignatureKey, Proposal<TYPES, VidDisperseShare<TYPES>>>,
>;

#[derive(Clone, Debug)]
pub struct TestStorageState<TYPES: NodeType> {
    vids: VidShares<TYPES>,
    das: HashMap<TYPES::Time, Proposal<TYPES, DaProposal<TYPES>>>,
    proposals: BTreeMap<TYPES::Time, Proposal<TYPES, QuorumProposal<TYPES>>>,
    high_qc: Option<hotshot_types::simple_certificate::QuorumCertificate<TYPES>>,
}

impl<TYPES: NodeType> Default for TestStorageState<TYPES> {
    fn default() -> Self {
        Self {
            vids: HashMap::new(),
            das: HashMap::new(),
            proposals: BTreeMap::new(),
            high_qc: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TestStorage<TYPES: NodeType> {
    inner: Arc<RwLock<TestStorageState<TYPES>>>,
    /// `should_return_err` is a testing utility to validate negative cases.
    pub should_return_err: bool,
    pub delay_config: DelayConfig,
}

impl<TYPES: NodeType> Default for TestStorage<TYPES> {
    fn default() -> Self {
        Self {
            inner: Arc::new(RwLock::new(TestStorageState::default())),
            should_return_err: false,
            delay_config: DelayConfig::default(),
        }
    }
}

#[async_trait]
impl<TYPES: NodeType> TestableDelay for TestStorage<TYPES> {
    async fn handle_async_delay(delay_config: DelayConfig) {
        if let Some(settings) = delay_config.get_setting(SupportedTypes::Storage) {
            match settings.delay_option {
                DelayOptions::None => {}
                DelayOptions::Fixed => {
                    async_sleep(Duration::from_millis(settings.fixed_time_in_milliseconds)).await;
                }
                DelayOptions::Random => {
                    let sleep_in_millis = rand::thread_rng().gen_range(
                        settings.min_time_in_milliseconds..=settings.max_time_in_milliseconds,
                    );
                    async_sleep(Duration::from_millis(sleep_in_millis)).await;
                }
            }
        }
    }
}

impl<TYPES: NodeType> TestStorage<TYPES> {
    pub async fn proposals_cloned(
        &self,
    ) -> BTreeMap<TYPES::Time, Proposal<TYPES, QuorumProposal<TYPES>>> {
        self.inner.read().await.proposals.clone()
    }
    pub async fn high_qc_cloned(&self) -> Option<QuorumCertificate<TYPES>> {
        self.inner.read().await.high_qc.clone()
    }
}

#[async_trait]
impl<TYPES: NodeType> Storage<TYPES> for TestStorage<TYPES> {
    async fn append_vid(&self, proposal: &Proposal<TYPES, VidDisperseShare<TYPES>>) -> Result<()> {
        if self.should_return_err {
            bail!("Failed to append VID proposal to storage");
        }
        Self::handle_async_delay(self.delay_config.clone()).await;
        let mut inner = self.inner.write().await;
        inner
            .vids
            .entry(proposal.data.view_number)
            .or_default()
            .insert(proposal.data.recipient_key.clone(), proposal.clone());
        Ok(())
    }

    async fn append_da(&self, proposal: &Proposal<TYPES, DaProposal<TYPES>>) -> Result<()> {
        if self.should_return_err {
            bail!("Failed to append VID proposal to storage");
        }
        Self::handle_async_delay(self.delay_config.clone()).await;
        let mut inner = self.inner.write().await;
        inner
            .das
            .insert(proposal.data.view_number, proposal.clone());
        Ok(())
    }
    async fn append_proposal(
        &self,
        proposal: &Proposal<TYPES, QuorumProposal<TYPES>>,
    ) -> Result<()> {
        if self.should_return_err {
            bail!("Failed to append VID proposal to storage");
        }
        Self::handle_async_delay(self.delay_config.clone()).await;
        let mut inner = self.inner.write().await;
        inner
            .proposals
            .insert(proposal.data.view_number, proposal.clone());
        Ok(())
    }

    async fn record_action(
        &self,
        _view: <TYPES as NodeType>::Time,
        _action: hotshot_types::event::HotShotAction,
    ) -> Result<()> {
        if self.should_return_err {
            bail!("Failed to append Action to storage");
        }
        Self::handle_async_delay(self.delay_config.clone()).await;
        Ok(())
    }

    async fn update_high_qc(
        &self,
        new_high_qc: hotshot_types::simple_certificate::QuorumCertificate<TYPES>,
    ) -> Result<()> {
        if self.should_return_err {
            bail!("Failed to update high qc to storage");
        }
        Self::handle_async_delay(self.delay_config.clone()).await;
        let mut inner = self.inner.write().await;
        if let Some(ref current_high_qc) = inner.high_qc {
            if new_high_qc.view_number() > current_high_qc.view_number() {
                inner.high_qc = Some(new_high_qc);
            }
        } else {
            inner.high_qc = Some(new_high_qc);
        }
        Ok(())
    }
    async fn update_undecided_state(
        &self,
        _leafs: CommitmentMap<Leaf<TYPES>>,
        _state: BTreeMap<TYPES::Time, View<TYPES>>,
    ) -> Result<()> {
        if self.should_return_err {
            bail!("Failed to update high qc to storage");
        }
        Self::handle_async_delay(self.delay_config.clone()).await;
        Ok(())
    }
}
