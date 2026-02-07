//! Instruction definitions for the Passive Stake program.
//!
//! All instructions are serialised / deserialised via `bincode` to stay
//! consistent with the other Agave built-in programs.

use serde::{Deserialize, Serialize};

/// Instructions supported by the Passive Stake program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PassiveStakeInstruction {
    /// Create a new passive stake account, transfer `amount` lamports from the
    /// funding account, and configure the lock tier.
    ///
    /// # Accounts expected
    ///
    /// 0. `[signer, writable]` — Funding / authority account (pays lamports).
    /// 1. `[writable]`         — Passive stake account (must be pre-allocated
    ///                           with the correct size and owned by this program).
    ///
    /// # Data
    ///
    /// * `lock_days` — Lock tier (0, 30, 90, 180, 360, or `u64::MAX` for permanent).
    /// * `amount`    — Lamports to lock.
    InitializePassiveStake {
        lock_days: u64,
        amount: u64,
    },

    /// Claim all accumulated (unclaimed) rewards and transfer them to the
    /// authority's wallet.  Rewards are always liquid — no lock applies.
    ///
    /// # Accounts expected
    ///
    /// 0. `[signer, writable]` — Authority account (receives rewards).
    /// 1. `[writable]`         — Passive stake account.
    /// 2. `[writable]`         — Rewards pool account (source of reward lamports).
    ClaimRewards,

    /// Unlock a non-permanent lock **after** the lock period has expired.
    /// Returns the full principal to the authority.
    ///
    /// # Accounts expected
    ///
    /// 0. `[signer, writable]` — Authority account (receives principal).
    /// 1. `[writable]`         — Passive stake account.
    Unlock,

    /// Early-unlock a non-permanent lock **before** the lock period expires.
    /// A penalty (percentage of principal) is burned; the remainder is returned.
    ///
    /// Permanent locks **cannot** be early-unlocked.
    ///
    /// # Accounts expected
    ///
    /// 0. `[signer, writable]` — Authority account (receives remainder).
    /// 1. `[writable]`         — Passive stake account.
    EarlyUnlock,

    /// Calculate epoch rewards for a passive stake account.
    /// Typically invoked at epoch boundaries by the runtime or a crank.
    ///
    /// # Accounts expected
    ///
    /// 0. `[writable]` — Passive stake account.
    ///
    /// # Data
    ///
    /// * `current_epoch`          — The current epoch number.
    /// * `validator_reward_rate`  — The validator staking rate for this epoch
    ///                              expressed in basis points (e.g. 500 = 5%).
    CalculateEpochRewards {
        current_epoch: u64,
        validator_reward_rate: u64,
    },
}
