# PhaseLock: A linear time, committee electing, BFT Protocol.

## Table of contents
  1. [Background](#background)
  2. [Protocol Overview](#protocol-overview)
     - [Sequential PhaseLock](#sequential)
     - [Pipelined PhaseLock](#sequential)
  3. [Appendices](#appendices) 
     1. [Type Definitions](#type-definitions)
        1. [Quorum Certificate](#quorum-certificate)


# Background

PhaseLock is a hybrid, committee electing, Proof of Stake protocol for the partially synchronous model that exhibits
optimistic responsiveness and linear communication footprint.

PhaseLock's construction borrows heavily from the construction of [Hotstuff](https://arxiv.org/abs/1803.05069) and
[Algorand](https://people.csail.mit.edu/nickolai/papers/gilad-algorand-eprint.pdf), in many senses being a synthesis of
Hotstuff's protocol with Algorand's sortition.

# Protocol Overview

PhaseLock comes in two variants, [Pipelined Phaselock](#pipelined) and [Sequential Phaselock](#sequential).
Sequential PhaseLock is the simpler of the two variants, and is the basal form, from which Pipelined PhaseLock is
derived, so it will be discussed first.

## Sequential

## Pipelined

# Appendices

## Type Definitions

### Quorum Certificate
