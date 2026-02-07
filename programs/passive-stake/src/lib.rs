//! TRv1 Passive Stake Program
//!
//! Manages tiered passive staking with configurable lock periods.
//! Users lock tokens for a chosen duration and earn a percentage of the
//! validator staking rate as rewards.  Longer locks earn higher rates and
//! greater governance voting weight.
//!
//! ## Tiers
//!
//! | Lock     | Reward (% of validator rate) | Approx APY | Vote weight |
//! |----------|:---------------------------:|:----------:|:-----------:|
//! | No lock  | 5%                          | 0.25%      | 0           |
//! | 30 days  | 10%                         | 0.50%      | 0.10×       |
//! | 90 days  | 20%                         | 1.00%      | 0.20×       |
//! | 180 days | 30%                         | 1.50%      | 0.30×       |
//! | 360 days | 50%                         | 2.50%      | 0.50×       |
//! | Permanent| 120%                        | 6.00%      | 1.50×       |

#![cfg(feature = "agave-unstable-api")]
#![allow(clippy::arithmetic_side_effects)]

pub mod constants;
pub mod error;
pub mod instruction;
pub mod processor;
pub mod state;

/// Re-export the program ID.
pub use processor::id;
