//! The consensus layer for hotshot. This currently implements sequencing
//! consensus in an event driven way

/// the task which implements the main parts of consensus
pub mod consensus;

/// The task which implements the main parts of data availability.
pub mod da;

/// The task which implements all transaction handling
pub mod transactions;

/// Defines the events passed between tasks
pub mod events;

/// The task which implements the network.
pub mod network;

/// Defines the types to run unit tests for a task.
pub mod harness;

/// The task which implements view synchronization
pub mod view_sync;

/// The task which implements verifiable information dispersal
pub mod vid;

/// Generic task for collecting votes
pub mod vote;

/// Task for handling upgrades
pub mod upgrade;

/// Implementations for builder client
/// Should contain builder task in the future
pub mod builder;

/// Helper functions used by any task
pub mod helpers;

/// Task which responsds to requests from the network
pub mod response;

/// Task for requesting the network for things
pub mod request;
