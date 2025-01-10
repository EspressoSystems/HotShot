# Example Types

This crate provides example implementations and test utilities for the HotShot consensus system. It contains various type implementations that are used for testing and demonstration purposes.

## Components

### Storage Types (`storage_types.rs`)

Contains test implementations for storage-related functionality:

- `TestStorage`: A test implementation of the `Storage` trait
- `TestStorageState`: Maintains the state for test storage including:
  - VID shares
  - Data Availability (DA) proposals
  - Quorum proposals
  - High QC (Quorum Certificate) tracking
  - Action and epoch tracking

### State Types (`state_types.rs`)

Provides test implementations for state-related components:

- `TestInstanceState`: Implementation of instance-level state for testing
- `TestStateDelta`: Application-specific state delta implementation
- `TestValidatedState`: Test implementation of validated state with:
  - Block height tracking
  - State commitment management
  - Header validation
  - Random transaction generation for testing

## Usage

This crate is primarily used for:
1. Testing the HotShot consensus implementation
2. Providing example implementations for reference
3. Demonstrating how to implement various traits required by the HotShot system

## Features

- Configurable delay settings for testing timing-related scenarios
- Support for both VID and VID2 implementations
- Test utilities for consensus migration
- Mock implementations of core HotShot traits

## Note

This crate is intended for testing and example purposes only. Do not use these implementations in production environments. 
