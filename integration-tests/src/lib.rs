//! TRv1 Integration Tests
//!
//! Comprehensive integration test suite for all TRv1-specific subsystems.
//!
//! # Subsystems Tested
//!
//! 1. **Passive Staking** — tiered locks (0/30/90/180/360/permanent), reward
//!    rates, early unlock penalties, governance vote weights
//! 2. **Developer Rewards** — revenue recipient registration, multi-splits,
//!    anti-gaming (CU threshold, 7-day cooldown, 10% epoch cap)
//! 3. **Treasury** — initialize, disburse, authority transfer, governance
//!    activation
//! 4. **Slashing** — double-sign penalties, escalating offenses, permanent bans,
//!    delegator protection, jailing/unjailing
//! 5. **Validator Set** — 200-cap active set, standby rotation, jailed exclusion
//! 6. **Fee Distribution** — epoch-dependent 4-way split (burn/validator/treasury/dev)
//! 7. **Inflation** — flat 5% annual on staked supply only

pub mod harness;

#[cfg(test)]
mod passive_staking_tests;

#[cfg(test)]
mod developer_rewards_tests;

#[cfg(test)]
mod treasury_tests;

#[cfg(test)]
mod slashing_tests;

#[cfg(test)]
mod validator_set_tests;

#[cfg(test)]
mod fee_distribution_tests;

#[cfg(test)]
mod inflation_tests;
