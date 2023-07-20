//! Testing harness for the hotshot repository
//!
//! To build a test environment you can create a [`TestLauncher`] instance. This launcher can be configured to have a custom networking layer, initial state, etc.
//!
//! Calling `TestLauncher::launch()` will turn this launcher into a [`TestRunner`], which can be used to start and stop nodes, send transacstions, etc.
//!
//! Node that `TestLauncher::launch()` is only available if the given `NETWORK`, `STATE` and `STORAGE` are correct.

#![warn(missing_docs)]
#![ doc = include_str!("../README.md")]

// /// implementations of various networking models
// pub mod network_reliability;

/// test launcher infrastructure
pub mod test_launcher;

/// structs and infra to describe the tests to be written
pub mod test_builder;

/// set of commonly used test types for our tests
pub mod test_types;

/// test runner
pub mod test_runner;

/// errors for tests
pub mod test_errors;

/// describe a round of consensus
pub mod round;

/// helper functions to build a round
pub mod round_builder;

/// tasks for view
pub mod app_tasks;
