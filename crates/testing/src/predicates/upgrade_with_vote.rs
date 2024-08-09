// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

#![cfg(feature = "dependency-tasks")]

use std::sync::Arc;

use async_trait::async_trait;
use hotshot_example_types::node_types::{MemoryImpl, TestTypes};
use hotshot_task_impls::quorum_vote::QuorumVoteTaskState;
use hotshot_types::simple_certificate::UpgradeCertificate;

use crate::predicates::{Predicate, PredicateResult};
type QuorumVoteTaskTestState = QuorumVoteTaskState<TestTypes, MemoryImpl>;

type UpgradeCertCallback =
    Arc<dyn Fn(Arc<Option<UpgradeCertificate<TestTypes>>>) -> bool + Send + Sync>;

pub struct UpgradeCertPredicate {
    check: UpgradeCertCallback,
    info: String,
}

impl std::fmt::Debug for UpgradeCertPredicate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.info)
    }
}

#[cfg(feature = "dependency-tasks")]
#[async_trait]
impl Predicate<QuorumVoteTaskTestState> for UpgradeCertPredicate {
    async fn evaluate(&self, input: &QuorumVoteTaskTestState) -> PredicateResult {
        let upgrade_cert = input.decided_upgrade_certificate.read().await.clone();
        PredicateResult::from((self.check)(upgrade_cert.into()))
    }

    async fn info(&self) -> String {
        self.info.clone()
    }
}

pub fn no_decided_upgrade_certificate() -> Box<UpgradeCertPredicate> {
    let info = "expected decided_upgrade_certificate to be None".to_string();
    let check: UpgradeCertCallback = Arc::new(move |s| s.is_none());
    Box::new(UpgradeCertPredicate { info, check })
}

pub fn decided_upgrade_certificate() -> Box<UpgradeCertPredicate> {
    let info = "expected decided_upgrade_certificate to be Some(_)".to_string();
    let check: UpgradeCertCallback = Arc::new(move |s| s.is_some());
    Box::new(UpgradeCertPredicate { info, check })
}
