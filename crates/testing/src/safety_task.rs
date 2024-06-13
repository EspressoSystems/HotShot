#![allow(clippy::unwrap_or_default)]
use std::collections::BTreeMap;

use anyhow::Result;
use async_trait::async_trait;
use committable::Committable;
use hotshot_types::{
    data::Leaf,
    event::{Event, EventType},
    traits::node_implementation::NodeType,
};

use crate::{
    overall_safety_task::OverallSafetyPropertiesDescription,
    test_task::{TestResult, TestTaskState},
};

trait Validatable {
    fn valid(&self) -> bool;
}

pub type NodeLeafMap<TYPES> = BTreeMap<<TYPES as NodeType>::Time, Leaf<TYPES>>;

/// For a NodeLeafMap, we validate that each leaf extends the preceding leaf.
impl<TYPES: NodeType> Validatable for NodeLeafMap<TYPES> {
    fn valid(&self) -> bool {
        let leaf_pairs = self.values().zip(self.values().skip(1));

        // Check that the child leaf follows the parent, possibly with a gap.
        leaf_pairs.fold(true, |acc, (parent, child)| {
            acc && ((child.justify_qc().view_number > parent.view_number())
                || (child.justify_qc().view_number == parent.view_number()
                    && child.justify_qc().data.leaf_commit == parent.commit()))
        })
    }
}

pub type NetworkLeafMap<TYPES> = BTreeMap<usize, NodeLeafMap<TYPES>>;

/// For a NetworkLeafMap, we validate that no two nodes have submitted differing leaves for any given view, in addition to the individual NodeLeafMap checks.
impl<TYPES: NodeType> Validatable for NetworkLeafMap<TYPES> {
    fn valid(&self) -> bool {
        // Invert the map by interchanging the roles of the node_id and view number.
        let mut inverted_map = BTreeMap::new();
        for (node_id, node_map) in self.iter() {
            if !node_map.valid() {
                return false;
            }
            // validate each node's leaf map
            for (view, leaf) in node_map.iter() {
                let view_map = inverted_map.entry(*view).or_insert(BTreeMap::new());
                view_map.insert(*node_id, leaf.clone());
            }
        }

        // iterate over the inverted map, ensuring we have at most one leaf for each view number.
        inverted_map.iter().fold(true, |acc, (_view, view_map)| {
            let mut leaves: Vec<_> = view_map.values().collect();

            leaves.dedup();

            acc && (leaves.len() <= 1)
        })
    }
}

/// Data availability task state
pub struct SafetyTask<TYPES: NodeType> {
    /// A map from node ids to (leaves keyed on view number)
    pub consensus_leaves: NetworkLeafMap<TYPES>,
    /// safety task requirements
    pub safety_properties: OverallSafetyPropertiesDescription,
}

#[async_trait]
impl<TYPES: NodeType> TestTaskState for SafetyTask<TYPES> {
    type Event = Event<TYPES>;

    /// Handles an event from one of multiple receivers.
    async fn handle_event(&mut self, (message, id): (Self::Event, usize)) -> Result<()> {
        if let Event {
            event: EventType::Decide { leaf_chain, .. },
            ..
        } = message
        {
            let map = &mut self.consensus_leaves.entry(id).or_insert(BTreeMap::new());

            leaf_chain.iter().for_each(|leaf_info| {
                map.insert(leaf_info.leaf.view_number(), leaf_info.leaf.clone());
            });
        }

        Ok(())
    }

    fn check(&self) -> TestResult {
        if !self.consensus_leaves.valid() {
            return TestResult::Fail(anyhow::anyhow!("invalid network state").into());
        }

        TestResult::Pass
    }
}
