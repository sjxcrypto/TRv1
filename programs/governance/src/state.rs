//! Account state types for the TRv1 Governance program.

use {
    borsh::{BorshDeserialize, BorshSerialize},
    serde::{Deserialize, Serialize},
    solana_hash::Hash,
    solana_pubkey::Pubkey,
};

// ---------------------------------------------------------------------------
// Discriminator bytes
// ---------------------------------------------------------------------------

/// Discriminator for `GovernanceConfig` accounts.
pub const GOVERNANCE_CONFIG_DISCRIMINATOR: u8 = 1;

/// Discriminator for `Proposal` accounts.
pub const PROPOSAL_DISCRIMINATOR: u8 = 2;

/// Discriminator for `VoteRecord` accounts (prevents double-voting).
pub const VOTE_RECORD_DISCRIMINATOR: u8 = 3;

// ---------------------------------------------------------------------------
// GovernanceConfig
// ---------------------------------------------------------------------------

/// Global governance configuration, stored in a single PDA.
///
/// At launch `is_active` is `false` — the authority (a 5-of-7 multisig) can
/// execute parameter changes using the same instruction format that governance
/// will use later.  When `ActivateGovernance` is called, `is_active` flips to
/// `true` and full proposal/vote flow takes over.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct GovernanceConfig {
    /// Whether on-chain governance is live.  `false` at launch.
    pub is_active: bool,

    /// Controlling authority: multisig at launch, governance PDA later.
    pub authority: Pubkey,

    /// Minimum staked (commitment-weighted) tokens to create a proposal.
    pub proposal_threshold: u64,

    /// How many epochs the voting window stays open.
    pub voting_period_epochs: u64,

    /// Minimum participation (basis points of total eligible voting power).
    /// 3000 = 30%.
    pub quorum_bps: u16,

    /// Percentage of *for* votes required to pass (of for + against).
    /// 5000 = 50%.
    pub pass_threshold_bps: u16,

    /// Percentage of veto votes that can block a proposal.
    /// 3333 = 33.3%.
    pub veto_threshold_bps: u16,

    /// Epochs to wait after a proposal passes before it can be executed.
    pub timelock_epochs: u64,

    /// Emergency multisig that can cancel dangerous proposals.
    pub emergency_multisig: Pubkey,

    /// Running proposal ID counter (monotonically increasing).
    pub next_proposal_id: u64,
}

impl GovernanceConfig {
    /// Serialised size: discriminator (1) + fields.
    ///
    /// Layout:
    ///   discriminator        (1)
    ///   is_active            (1)
    ///   authority            (32)
    ///   proposal_threshold   (8)
    ///   voting_period_epochs (8)
    ///   quorum_bps           (2)
    ///   pass_threshold_bps   (2)
    ///   veto_threshold_bps   (2)
    ///   timelock_epochs      (8)
    ///   emergency_multisig   (32)
    ///   next_proposal_id     (8)
    ///   = 104 bytes
    pub const SERIALIZED_SIZE: usize = 1 + 1 + 32 + 8 + 8 + 2 + 2 + 2 + 8 + 32 + 8;

    /// Deserialise from raw account data (expects leading discriminator).
    pub fn deserialize(data: &[u8]) -> Result<Self, std::io::Error> {
        if data.is_empty() || data[0] != GOVERNANCE_CONFIG_DISCRIMINATOR {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "missing or invalid governance config discriminator",
            ));
        }
        let mut cursor = &data[1..];
        BorshDeserialize::deserialize_reader(&mut cursor)
    }

    /// Serialise into raw account data (prepends discriminator).
    pub fn serialize_into(&self, data: &mut [u8]) -> Result<(), std::io::Error> {
        if data.len() < Self::SERIALIZED_SIZE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "account data buffer too small for GovernanceConfig",
            ));
        }
        data[0] = GOVERNANCE_CONFIG_DISCRIMINATOR;
        let mut cursor = &mut data[1..];
        BorshSerialize::serialize(self, &mut cursor)
    }
}

// ---------------------------------------------------------------------------
// Proposal
// ---------------------------------------------------------------------------

/// The type of action a proposal represents.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub enum ProposalType {
    /// Change a network parameter (fee split, staking rate, etc.).
    ParameterChange {
        param_id: u32,
        new_value: u64,
    },
    /// Transfer lamports from the treasury.
    TreasurySpend {
        recipient: Pubkey,
        amount: u64,
        memo: [u8; 32],
    },
    /// Emergency: unlock a permanently locked account.
    /// Requires an 80% supermajority to pass.
    EmergencyUnlock {
        target_account: Pubkey,
    },
    /// Upgrade a program via the loader.
    ProgramUpgrade {
        program_id: Pubkey,
        buffer_account: Pubkey,
    },
    /// Activate or deactivate a runtime feature.
    FeatureToggle {
        feature_id: u32,
        enabled: bool,
    },
    /// Text-only signaling proposal (no on-chain execution).
    TextProposal,
}

/// Lifecycle status of a proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
#[borsh(use_discriminant = true)]
pub enum ProposalStatus {
    /// Created but not yet activated for voting.
    Draft = 0,
    /// Voting is open.
    Active = 1,
    /// Voting ended and the proposal met quorum + pass threshold.
    Passed = 2,
    /// Voting ended and the proposal failed to meet thresholds.
    Rejected = 3,
    /// Vetoed before or during voting.
    Vetoed = 4,
    /// Passed and waiting out the timelock delay.
    Timelocked = 5,
    /// Successfully executed on-chain.
    Executed = 6,
    /// Cancelled by the emergency multisig.
    Cancelled = 7,
    /// Voting period ended without reaching quorum.
    Expired = 8,
}

/// The vote choice a participant casts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
#[borsh(use_discriminant = true)]
pub enum Vote {
    For = 0,
    Against = 1,
    Abstain = 2,
    Veto = 3,
}

/// On-chain state for a single governance proposal.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Proposal {
    /// Unique, monotonically increasing proposal identifier.
    pub id: u64,

    /// The account that created this proposal.
    pub proposer: Pubkey,

    /// Human-readable title (UTF-8, zero-padded to 64 bytes).
    pub title: [u8; 64],

    /// Hash of the full description (e.g. IPFS CID).
    pub description_hash: Hash,

    /// What this proposal does when executed.
    pub proposal_type: ProposalType,

    /// Current lifecycle status.
    pub status: ProposalStatus,

    /// Epoch when the proposal was created.
    pub created_epoch: u64,

    /// Epoch when voting closes.
    pub voting_ends_epoch: u64,

    /// Epoch when the proposal may be executed (after timelock).
    pub execution_epoch: u64,

    /// Total commitment-weighted votes *for*.
    pub votes_for: u64,

    /// Total commitment-weighted votes *against*.
    pub votes_against: u64,

    /// Total commitment-weighted *abstain* votes (count toward quorum).
    pub votes_abstain: u64,

    /// Total commitment-weighted *veto* votes.
    pub veto_votes: u64,

    /// Whether the proposal has been executed.
    pub executed: bool,
}

impl Proposal {
    /// Conservative upper bound on serialised size.
    ///
    /// The actual size varies by `ProposalType` variant, but we allocate the
    /// maximum to keep accounts fixed-size.
    ///
    /// Layout (worst case — TreasurySpend is largest variant):
    ///   discriminator       (1)
    ///   id                  (8)
    ///   proposer            (32)
    ///   title               (64)
    ///   description_hash    (32)
    ///   proposal_type tag   (4)  (borsh enum discriminant)
    ///   proposal_type data  (72) (TreasurySpend: Pubkey(32) + u64(8) + [u8;32](32))
    ///   status              (1)
    ///   created_epoch       (8)
    ///   voting_ends_epoch   (8)
    ///   execution_epoch     (8)
    ///   votes_for           (8)
    ///   votes_against       (8)
    ///   votes_abstain       (8)
    ///   veto_votes          (8)
    ///   executed            (1)
    ///   = 271 bytes
    ///
    /// We round up to 512 for future extensibility.
    pub const SERIALIZED_SIZE: usize = 512;

    /// Deserialise from raw account data (expects leading discriminator).
    pub fn deserialize(data: &[u8]) -> Result<Self, std::io::Error> {
        if data.is_empty() || data[0] != PROPOSAL_DISCRIMINATOR {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "missing or invalid proposal discriminator",
            ));
        }
        let mut cursor = &data[1..];
        BorshDeserialize::deserialize_reader(&mut cursor)
    }

    /// Serialise into raw account data (prepends discriminator).
    pub fn serialize_into(&self, data: &mut [u8]) -> Result<(), std::io::Error> {
        if data.len() < Self::SERIALIZED_SIZE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "account data buffer too small for Proposal",
            ));
        }
        data[0] = PROPOSAL_DISCRIMINATOR;
        let mut cursor = &mut data[1..];
        BorshSerialize::serialize(self, &mut cursor)
    }

    /// Returns `true` if this is an `EmergencyUnlock` proposal.
    pub fn is_emergency_unlock(&self) -> bool {
        matches!(self.proposal_type, ProposalType::EmergencyUnlock { .. })
    }
}

// ---------------------------------------------------------------------------
// VoteRecord — prevents double-voting
// ---------------------------------------------------------------------------

/// Per-voter record for a given proposal.  Created on first vote; existence
/// prevents the same voter from voting again on the same proposal.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct VoteRecord {
    /// The proposal this vote is for.
    pub proposal_id: u64,

    /// The voter's public key.
    pub voter: Pubkey,

    /// The vote cast.
    pub vote: Vote,

    /// The commitment-weighted voting power applied.
    pub weight: u64,

    /// Epoch when the vote was cast.
    pub voted_epoch: u64,
}

impl VoteRecord {
    /// Serialised size:
    ///   discriminator  (1)
    ///   proposal_id    (8)
    ///   voter          (32)
    ///   vote           (1)
    ///   weight         (8)
    ///   voted_epoch    (8)
    ///   = 58 bytes
    pub const SERIALIZED_SIZE: usize = 1 + 8 + 32 + 1 + 8 + 8;

    /// Deserialise from raw account data.
    pub fn deserialize(data: &[u8]) -> Result<Self, std::io::Error> {
        if data.is_empty() || data[0] != VOTE_RECORD_DISCRIMINATOR {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "missing or invalid vote record discriminator",
            ));
        }
        let mut cursor = &data[1..];
        BorshDeserialize::deserialize_reader(&mut cursor)
    }

    /// Serialise into raw account data.
    pub fn serialize_into(&self, data: &mut [u8]) -> Result<(), std::io::Error> {
        if data.len() < Self::SERIALIZED_SIZE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "account data buffer too small for VoteRecord",
            ));
        }
        data[0] = VOTE_RECORD_DISCRIMINATOR;
        let mut cursor = &mut data[1..];
        BorshSerialize::serialize(self, &mut cursor)
    }
}
