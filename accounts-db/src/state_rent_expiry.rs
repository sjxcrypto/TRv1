//! TRv1 State Rent Expiry
//!
//! This module implements account archival and revival based on inactivity
//! and rent payment. Unlike Solana's current model where all accounts live
//! in RAM forever (making rent-exempt essentially free storage), TRv1
//! introduces meaningful storage costs through tiered archival.
//!
//! # Account Lifecycle
//!
//! ```text
//!   Active Account
//!       │
//!       ▼ (inactive > cold_threshold_days)
//!   ┌─────────────────────┐
//!   │  check_rent_expiry  │ ── false ──▶ Account stays active
//!   └─────────────────────┘
//!       │ true
//!       ▼
//!   ┌─────────────────────┐
//!   │  archive_account    │ ──▶ Writes data to cold storage
//!   └─────────────────────┘     Creates ArchivedAccount metadata
//!       │                       Generates Merkle proof
//!       ▼
//!   Cold Storage (disk)
//!       │
//!       ▼ (user requests revival + deposits rent)
//!   ┌─────────────────────┐
//!   │  revive_account     │ ──▶ Loads from cold, verifies proof
//!   └─────────────────────┘     Charges revival rent deposit
//!       │                       Returns to active state
//!       ▼
//!   Active Account (restored)
//! ```
//!
//! # Merkle Proofs
//!
//! Each archived account gets a Merkle proof that allows anyone to verify
//! the account existed with specific data at the time of archival. This
//! enables trustless revival without requiring the validator to scan all
//! cold storage.
//!
//! # Integration
//!
//! This module is designed to be called by the runtime during epoch
//! boundaries or by a background service. Full integration with the
//! Bank and AccountsDb will happen in a subsequent phase.

use {
    solana_account::{AccountSharedData, ReadableAccount, WritableAccount},
    solana_hash::Hash,
    solana_pubkey::Pubkey,
    solana_sha256_hasher::hash as sha256_hash,
    std::{
        collections::HashMap,
        io,
        path::{Path, PathBuf},
        time::{Duration, SystemTime, UNIX_EPOCH},
    },
};

// ── Constants ───────────────────────────────────────────────────────────────

/// Default lamports charged per byte per year for rent.
/// This is more aggressive than Solana's current rent since TRv1 actually
/// enforces storage costs via archival.
pub const DEFAULT_LAMPORTS_PER_BYTE_YEAR: u64 = 3_480;

/// Default inactivity threshold before archival (days).
pub const DEFAULT_ARCHIVE_AFTER_DAYS: u64 = 365;

/// Estimated slots per day at 400ms slot time.
pub const ESTIMATED_SLOTS_PER_DAY: u64 = 216_000;

/// Minimum account data size to consider for archival.
/// Accounts smaller than this are too cheap to bother archiving.
pub const MIN_ARCHIVAL_DATA_SIZE: usize = 128;

/// Minimum rent deposit required to revive an account (in lamports).
/// This covers at least 2 years of rent to prevent immediate re-archival.
pub const MIN_REVIVAL_RENT_YEARS: u64 = 2;

// ── State Rent Config ───────────────────────────────────────────────────────

/// Configuration for the state rent expiry system.
///
/// Controls when accounts are archived and how they can be revived.
#[derive(Debug, Clone)]
pub struct StateRentConfig {
    /// Minimum lamports per byte per year for rent.
    ///
    /// Accounts whose lamport balance is insufficient to cover
    /// `data_len * lamports_per_byte_year * remaining_years` are
    /// candidates for archival.
    pub lamports_per_byte_year: u64,

    /// Days of inactivity before an account is eligible for archival.
    ///
    /// An account is considered "inactive" if it has not been written to
    /// in this many days. Read-only access does NOT count as activity
    /// (to avoid gaming the system by simply reading an account).
    ///
    /// Default: 365 (1 year)
    pub archive_after_days: u64,

    /// Whether archived accounts can be revived.
    ///
    /// If `false`, archival is permanent and the account data is only
    /// preserved for historical proof purposes.
    ///
    /// Default: `true`
    pub allow_revival: bool,

    /// Path to cold storage where archived account data is written.
    pub cold_storage_path: PathBuf,

    /// If true, accounts owned by system programs (e.g., stake, vote)
    /// are exempt from archival regardless of inactivity.
    pub exempt_system_programs: bool,
}

impl Default for StateRentConfig {
    fn default() -> Self {
        Self {
            lamports_per_byte_year: DEFAULT_LAMPORTS_PER_BYTE_YEAR,
            archive_after_days: DEFAULT_ARCHIVE_AFTER_DAYS,
            allow_revival: true,
            cold_storage_path: PathBuf::from("trv1-cold-storage"),
            exempt_system_programs: true,
        }
    }
}

impl StateRentConfig {
    /// Create a config suitable for testing with shorter thresholds.
    pub fn for_testing() -> Self {
        Self {
            lamports_per_byte_year: 1_000,
            archive_after_days: 1,
            allow_revival: true,
            cold_storage_path: PathBuf::from("/tmp/trv1-test-cold"),
            exempt_system_programs: false,
        }
    }

    /// Calculate the rent owed for an account of the given data length
    /// over the specified number of years.
    pub fn calculate_rent(&self, data_len: usize, years: u64) -> u64 {
        (data_len as u64)
            .saturating_mul(self.lamports_per_byte_year)
            .saturating_mul(years)
    }

    /// Calculate the minimum lamport balance required to keep an account
    /// active for the specified number of years.
    pub fn minimum_balance(&self, data_len: usize, years: u64) -> u64 {
        self.calculate_rent(data_len, years)
    }

    /// Calculate the minimum revival deposit for an account.
    pub fn revival_deposit(&self, data_len: usize) -> u64 {
        self.calculate_rent(data_len, MIN_REVIVAL_RENT_YEARS)
    }
}

// ── Archived Account ────────────────────────────────────────────────────────

/// Metadata for an account that has been archived to cold storage.
///
/// This struct contains everything needed to verify and potentially
/// revive an archived account. The actual account data is stored
/// separately in cold storage files.
#[derive(Debug, Clone)]
pub struct ArchivedAccount {
    /// The public key of the archived account.
    pub pubkey: Pubkey,

    /// The slot at which the account was archived.
    pub archive_slot: u64,

    /// The epoch at which the account was archived.
    pub archive_epoch: u64,

    /// Hash of the account data at the time of archival.
    /// Used to verify data integrity when reviving.
    pub account_hash: Hash,

    /// Lamport balance at the time of archival.
    pub lamports_at_archive: u64,

    /// Length of the account's data field at archival.
    pub data_len: usize,

    /// The owner program of the archived account.
    pub owner: Pubkey,

    /// Whether the account was executable.
    pub executable: bool,

    /// Timestamp of when the account was archived (Unix epoch seconds).
    pub archive_timestamp: u64,

    /// Path to the cold storage file containing this account's data.
    pub storage_path: PathBuf,

    /// Merkle proof for the account at the time of archival.
    /// Allows trustless verification that this account existed.
    pub merkle_proof: Option<MerkleProof>,
}

// ── Merkle Proof ────────────────────────────────────────────────────────────

/// A Merkle proof demonstrating that an account existed in the
/// accounts hash tree at a specific slot.
///
/// This is a simplified proof structure. The full implementation
/// will integrate with Solana's existing accounts hash infrastructure.
#[derive(Debug, Clone)]
pub struct MerkleProof {
    /// The leaf hash (hash of the account data).
    pub leaf_hash: Hash,

    /// Sibling hashes along the path from leaf to root.
    pub proof_hashes: Vec<Hash>,

    /// The root hash at the time the proof was generated.
    pub root_hash: Hash,

    /// The slot at which this proof is valid.
    pub proof_slot: u64,

    /// Index of the leaf in the tree (for determining left/right siblings).
    pub leaf_index: u64,
}

impl MerkleProof {
    /// Verify this proof against the given account data.
    ///
    /// Returns `true` if the proof is valid, meaning the account data
    /// hashes to `leaf_hash` and the proof path leads to `root_hash`.
    pub fn verify(&self, account_data_hash: &Hash) -> bool {
        if *account_data_hash != self.leaf_hash {
            return false;
        }

        let mut current_hash = self.leaf_hash;
        let mut index = self.leaf_index;

        for sibling in &self.proof_hashes {
            current_hash = if index % 2 == 0 {
                // Current is left child, sibling is right
                combine_hashes(&current_hash, sibling)
            } else {
                // Current is right child, sibling is left
                combine_hashes(sibling, &current_hash)
            };
            index /= 2;
        }

        current_hash == self.root_hash
    }
}

/// Combine two hashes to produce a parent hash in the Merkle tree.
fn combine_hashes(left: &Hash, right: &Hash) -> Hash {
    let mut combined = Vec::with_capacity(64);
    combined.extend_from_slice(left.as_ref());
    combined.extend_from_slice(right.as_ref());
    sha256_hash(&combined)
}

// ── Archive Index ───────────────────────────────────────────────────────────

/// In-memory index of all archived accounts.
///
/// This provides O(1) lookup to check if an account has been archived
/// and to retrieve its metadata for revival.
///
/// In production, this would be backed by a persistent key-value store
/// (e.g., RocksDB) rather than a HashMap.
#[derive(Debug, Default)]
pub struct ArchiveIndex {
    /// Map from pubkey to archived account metadata.
    entries: HashMap<Pubkey, ArchivedAccount>,

    /// Total number of accounts archived.
    pub total_archived: u64,

    /// Total bytes of account data archived.
    pub total_archived_bytes: u64,

    /// Total number of accounts revived.
    pub total_revived: u64,
}

impl ArchiveIndex {
    /// Create a new empty archive index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an archived account in the index.
    pub fn insert(&mut self, archived: ArchivedAccount) {
        self.total_archived = self.total_archived.saturating_add(1);
        self.total_archived_bytes = self
            .total_archived_bytes
            .saturating_add(archived.data_len as u64);
        self.entries.insert(archived.pubkey, archived);
    }

    /// Look up an archived account by pubkey.
    pub fn get(&self, pubkey: &Pubkey) -> Option<&ArchivedAccount> {
        self.entries.get(pubkey)
    }

    /// Remove an account from the archive index (on revival).
    pub fn remove(&mut self, pubkey: &Pubkey) -> Option<ArchivedAccount> {
        if let Some(archived) = self.entries.remove(pubkey) {
            self.total_revived = self.total_revived.saturating_add(1);
            Some(archived)
        } else {
            None
        }
    }

    /// Check if a pubkey is archived.
    pub fn is_archived(&self, pubkey: &Pubkey) -> bool {
        self.entries.contains_key(pubkey)
    }

    /// Return the number of currently-archived accounts.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Return whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ── Core Functions ──────────────────────────────────────────────────────────

/// Check whether an account is eligible for rent expiry / archival.
///
/// An account is eligible if:
/// 1. It has been inactive for longer than `config.archive_after_days`
/// 2. Its data length exceeds `MIN_ARCHIVAL_DATA_SIZE`
/// 3. It is not owned by an exempt system program (if configured)
///
/// # Arguments
///
/// * `account` - The account to check
/// * `last_active_slot` - The slot when the account was last written to
/// * `current_slot` - The current slot
/// * `config` - Rent expiry configuration
///
/// # Returns
///
/// `true` if the account should be archived
pub fn check_rent_expiry(
    account: &AccountSharedData,
    last_active_slot: u64,
    current_slot: u64,
    config: &StateRentConfig,
) -> bool {
    // Skip accounts that are too small to bother archiving
    if account.data().len() < MIN_ARCHIVAL_DATA_SIZE {
        return false;
    }

    // Skip zero-lamport accounts (they'll be cleaned by normal GC)
    if account.lamports() == 0 {
        return false;
    }

    // Check system program exemption
    if config.exempt_system_programs && is_system_program(account.owner()) {
        return false;
    }

    // Calculate inactivity in slots
    let inactive_slots = current_slot.saturating_sub(last_active_slot);
    let inactive_days = inactive_slots / ESTIMATED_SLOTS_PER_DAY;

    inactive_days >= config.archive_after_days
}

/// Archive an account to cold storage.
///
/// This serializes the account data to disk and creates an `ArchivedAccount`
/// metadata entry. The account should be removed from the accounts database
/// after this call succeeds.
///
/// # Arguments
///
/// * `pubkey` - The account's public key
/// * `account` - The account data to archive
/// * `current_slot` - The current slot
/// * `current_epoch` - The current epoch
/// * `config` - Rent configuration
///
/// # Returns
///
/// An `ArchivedAccount` containing the metadata, or an error if storage fails.
pub fn archive_account(
    pubkey: &Pubkey,
    account: &AccountSharedData,
    current_slot: u64,
    current_epoch: u64,
    config: &StateRentConfig,
) -> io::Result<ArchivedAccount> {
    // Compute the account data hash for integrity verification
    let account_hash = compute_account_hash(pubkey, account);

    // Determine storage path for this account
    let storage_path = cold_storage_path_for_account(pubkey, &config.cold_storage_path);

    // Ensure parent directory exists
    if let Some(parent) = storage_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Serialize account data to cold storage
    let serialized = serialize_account_for_cold_storage(account);
    std::fs::write(&storage_path, &serialized)?;

    // Get current timestamp
    let archive_timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();

    Ok(ArchivedAccount {
        pubkey: *pubkey,
        archive_slot: current_slot,
        archive_epoch: current_epoch,
        account_hash,
        lamports_at_archive: account.lamports(),
        data_len: account.data().len(),
        owner: *account.owner(),
        executable: account.executable(),
        archive_timestamp,
        storage_path,
        merkle_proof: None, // Will be populated by the accounts hash system
    })
}

/// Revive an archived account from cold storage.
///
/// Loads the account data from cold storage, verifies its integrity
/// using the stored hash, and returns the restored account.
///
/// # Arguments
///
/// * `archived` - Metadata of the archived account
/// * `rent_deposit` - Lamports deposited for future rent
/// * `config` - Rent configuration
///
/// # Returns
///
/// The restored `AccountSharedData`, or an error if verification fails.
pub fn revive_account(
    archived: &ArchivedAccount,
    rent_deposit: u64,
    config: &StateRentConfig,
) -> Result<AccountSharedData, RevivalError> {
    // Check that revival is allowed
    if !config.allow_revival {
        return Err(RevivalError::RevivalDisabled);
    }

    // Verify sufficient rent deposit
    let min_deposit = config.revival_deposit(archived.data_len);
    if rent_deposit < min_deposit {
        return Err(RevivalError::InsufficientRentDeposit {
            required: min_deposit,
            provided: rent_deposit,
        });
    }

    // Load account data from cold storage
    let serialized = std::fs::read(&archived.storage_path).map_err(|e| {
        RevivalError::StorageError(format!(
            "Failed to read cold storage at {:?}: {}",
            archived.storage_path, e
        ))
    })?;

    let account = deserialize_account_from_cold_storage(&serialized).map_err(|e| {
        RevivalError::StorageError(format!("Failed to deserialize account: {}", e))
    })?;

    // Verify data integrity
    let computed_hash = compute_account_hash(&archived.pubkey, &account);
    if computed_hash != archived.account_hash {
        return Err(RevivalError::IntegrityCheckFailed {
            expected: archived.account_hash,
            computed: computed_hash,
        });
    }

    // Verify Merkle proof if available
    if let Some(ref proof) = archived.merkle_proof {
        if !proof.verify(&computed_hash) {
            return Err(RevivalError::MerkleProofFailed);
        }
    }

    Ok(account)
}

/// Get the Merkle proof for an archived account.
///
/// Returns `None` if the account is not archived or if no proof is available.
pub fn get_archive_proof(
    pubkey: &Pubkey,
    archive_index: &ArchiveIndex,
) -> Option<MerkleProof> {
    archive_index
        .get(pubkey)
        .and_then(|archived| archived.merkle_proof.clone())
}

// ── Error Types ─────────────────────────────────────────────────────────────

/// Errors that can occur during account revival.
#[derive(Debug)]
pub enum RevivalError {
    /// Revival has been disabled in the configuration.
    RevivalDisabled,

    /// The rent deposit is insufficient to cover the minimum revival period.
    InsufficientRentDeposit {
        required: u64,
        provided: u64,
    },

    /// Failed to read or write cold storage.
    StorageError(String),

    /// The account data hash doesn't match the archived hash.
    IntegrityCheckFailed {
        expected: Hash,
        computed: Hash,
    },

    /// The Merkle proof verification failed.
    MerkleProofFailed,
}

impl std::fmt::Display for RevivalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RevivalError::RevivalDisabled => write!(f, "Account revival is disabled"),
            RevivalError::InsufficientRentDeposit { required, provided } => {
                write!(
                    f,
                    "Insufficient rent deposit: required {} lamports, provided {}",
                    required, provided
                )
            }
            RevivalError::StorageError(msg) => write!(f, "Storage error: {}", msg),
            RevivalError::IntegrityCheckFailed { expected, computed } => {
                write!(
                    f,
                    "Integrity check failed: expected hash {}, computed {}",
                    expected, computed
                )
            }
            RevivalError::MerkleProofFailed => write!(f, "Merkle proof verification failed"),
        }
    }
}

impl std::error::Error for RevivalError {}

// ── Helper Functions ────────────────────────────────────────────────────────

/// Compute a hash of the account data for integrity verification.
fn compute_account_hash(pubkey: &Pubkey, account: &AccountSharedData) -> Hash {
    let mut hasher_input = Vec::new();
    hasher_input.extend_from_slice(pubkey.as_ref());
    hasher_input.extend_from_slice(&account.lamports().to_le_bytes());
    hasher_input.extend_from_slice(account.owner().as_ref());
    hasher_input.extend_from_slice(&[account.executable() as u8]);
    hasher_input.extend_from_slice(account.data());
    sha256_hash(&hasher_input)
}

/// Determine the cold storage file path for an account.
///
/// Uses the first 4 bytes of the pubkey to create a two-level directory
/// structure, distributing files evenly across directories:
///   `cold_storage_path/ab/cd/<pubkey>.bin`
fn cold_storage_path_for_account(pubkey: &Pubkey, base_path: &Path) -> PathBuf {
    let bytes = pubkey.as_ref();
    let dir1 = format!("{:02x}", bytes[0]);
    let dir2 = format!("{:02x}", bytes[1]);
    base_path
        .join(dir1)
        .join(dir2)
        .join(format!("{}.bin", pubkey))
}

/// Serialize an account for cold storage.
///
/// Format: [lamports:8][owner:32][executable:1][data_len:8][data:N]
fn serialize_account_for_cold_storage(account: &AccountSharedData) -> Vec<u8> {
    let data = account.data();
    let mut buf = Vec::with_capacity(8 + 32 + 1 + 8 + data.len());
    buf.extend_from_slice(&account.lamports().to_le_bytes());
    buf.extend_from_slice(account.owner().as_ref());
    buf.push(account.executable() as u8);
    buf.extend_from_slice(&(data.len() as u64).to_le_bytes());
    buf.extend_from_slice(data);
    buf
}

/// Deserialize an account from cold storage format.
fn deserialize_account_from_cold_storage(data: &[u8]) -> io::Result<AccountSharedData> {
    // Minimum size: 8 (lamports) + 32 (owner) + 1 (executable) + 8 (data_len) = 49
    if data.len() < 49 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Cold storage data too short",
        ));
    }

    let lamports = u64::from_le_bytes(data[0..8].try_into().unwrap());
    let owner = Pubkey::try_from(&data[8..40]).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidData, "Invalid owner pubkey")
    })?;
    let executable = data[40] != 0;
    let data_len = u64::from_le_bytes(data[41..49].try_into().unwrap()) as usize;

    if data.len() < 49 + data_len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "Cold storage data truncated: expected {} data bytes, got {}",
                data_len,
                data.len() - 49
            ),
        ));
    }

    let account_data = &data[49..49 + data_len];

    let mut account = AccountSharedData::new(lamports, data_len, &owner);
    account.set_data_from_slice(account_data);
    account.set_executable(executable);

    Ok(account)
}

/// Check if a pubkey is a known system program that should be exempt
/// from archival.
fn is_system_program(owner: &Pubkey) -> bool {
    // Well-known system program IDs
    // In production, this would reference solana_system_program::id(), etc.
    let system_programs: [&[u8]; 4] = [
        // System Program: 11111111111111111111111111111111
        &[0u8; 32],
        // BPF Loader: BPFLoaderUpgradeab1e11111111111111111111111
        &[
            2, 168, 246, 145, 78, 136, 161, 107, 189, 35, 149, 133, 95, 100, 4, 217, 180, 244,
            86, 183, 130, 27, 176, 20, 87, 73, 66, 140, 0, 0, 0, 0,
        ],
        // Token Program: TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA
        &[
            6, 221, 246, 225, 215, 101, 161, 147, 217, 203, 225, 70, 206, 235, 121, 172, 28,
            180, 133, 237, 95, 91, 55, 145, 58, 140, 245, 133, 126, 255, 0, 169,
        ],
        // Stake Program: Stake11111111111111111111111111111111111111
        &[
            6, 161, 216, 23, 145, 55, 84, 42, 152, 52, 55, 189, 254, 42, 122, 178, 85, 127, 83,
            92, 138, 120, 114, 43, 104, 164, 157, 192, 0, 0, 0, 0,
        ],
    ];

    system_programs.iter().any(|sp| owner.as_ref() == *sp)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_account(lamports: u64, data_len: usize) -> AccountSharedData {
        let owner = Pubkey::new_unique();
        let mut account = AccountSharedData::new(lamports, data_len, &owner);
        account.set_data_from_slice(&vec![42u8; data_len]);
        account
    }

    #[test]
    fn test_default_config() {
        let config = StateRentConfig::default();
        assert_eq!(config.lamports_per_byte_year, DEFAULT_LAMPORTS_PER_BYTE_YEAR);
        assert_eq!(config.archive_after_days, DEFAULT_ARCHIVE_AFTER_DAYS);
        assert!(config.allow_revival);
        assert!(config.exempt_system_programs);
    }

    #[test]
    fn test_calculate_rent() {
        let config = StateRentConfig {
            lamports_per_byte_year: 1000,
            ..Default::default()
        };
        assert_eq!(config.calculate_rent(100, 1), 100_000);
        assert_eq!(config.calculate_rent(100, 2), 200_000);
        assert_eq!(config.calculate_rent(0, 1), 0);
    }

    #[test]
    fn test_check_rent_expiry_inactive_account() {
        let config = StateRentConfig {
            archive_after_days: 365,
            exempt_system_programs: false,
            ..Default::default()
        };
        let account = make_account(1000, 256);
        let last_active_slot = 0;
        // 400 days worth of slots
        let current_slot = 400 * ESTIMATED_SLOTS_PER_DAY;

        assert!(check_rent_expiry(&account, last_active_slot, current_slot, &config));
    }

    #[test]
    fn test_check_rent_expiry_active_account() {
        let config = StateRentConfig {
            archive_after_days: 365,
            exempt_system_programs: false,
            ..Default::default()
        };
        let account = make_account(1000, 256);
        let current_slot = 100 * ESTIMATED_SLOTS_PER_DAY; // 100 days
        let last_active_slot = current_slot - 50 * ESTIMATED_SLOTS_PER_DAY; // active 50 days ago

        assert!(!check_rent_expiry(&account, last_active_slot, current_slot, &config));
    }

    #[test]
    fn test_check_rent_expiry_small_account_exempt() {
        let config = StateRentConfig {
            archive_after_days: 1,
            exempt_system_programs: false,
            ..Default::default()
        };
        let account = make_account(1000, 10); // tiny account
        let current_slot = 1000 * ESTIMATED_SLOTS_PER_DAY;

        assert!(!check_rent_expiry(&account, 0, current_slot, &config));
    }

    #[test]
    fn test_check_rent_expiry_zero_lamport_exempt() {
        let config = StateRentConfig {
            archive_after_days: 1,
            exempt_system_programs: false,
            ..Default::default()
        };
        let account = make_account(0, 256);
        let current_slot = 1000 * ESTIMATED_SLOTS_PER_DAY;

        assert!(!check_rent_expiry(&account, 0, current_slot, &config));
    }

    #[test]
    fn test_archive_and_revive_roundtrip() {
        let config = StateRentConfig::for_testing();
        let pubkey = Pubkey::new_unique();
        let account = make_account(100_000, 256);

        // Archive
        let archived = archive_account(&pubkey, &account, 42, 1, &config).unwrap();
        assert_eq!(archived.pubkey, pubkey);
        assert_eq!(archived.archive_slot, 42);
        assert_eq!(archived.data_len, 256);
        assert_eq!(archived.lamports_at_archive, 100_000);

        // Revive
        let revival_deposit = config.revival_deposit(256);
        let revived = revive_account(&archived, revival_deposit, &config).unwrap();
        assert_eq!(revived.data(), account.data());
        assert_eq!(revived.lamports(), account.lamports());
        assert_eq!(revived.owner(), account.owner());

        // Cleanup
        let _ = std::fs::remove_file(&archived.storage_path);
    }

    #[test]
    fn test_revive_insufficient_deposit() {
        let config = StateRentConfig::for_testing();
        let pubkey = Pubkey::new_unique();
        let account = make_account(100_000, 256);

        let archived = archive_account(&pubkey, &account, 42, 1, &config).unwrap();

        let result = revive_account(&archived, 1, &config); // too little
        assert!(matches!(result, Err(RevivalError::InsufficientRentDeposit { .. })));

        // Cleanup
        let _ = std::fs::remove_file(&archived.storage_path);
    }

    #[test]
    fn test_revive_disabled() {
        let config = StateRentConfig {
            allow_revival: false,
            ..StateRentConfig::for_testing()
        };
        let pubkey = Pubkey::new_unique();
        let account = make_account(100_000, 256);

        let archived = archive_account(&pubkey, &account, 42, 1, &config).unwrap();

        let result = revive_account(&archived, 1_000_000, &config);
        assert!(matches!(result, Err(RevivalError::RevivalDisabled)));

        // Cleanup
        let _ = std::fs::remove_file(&archived.storage_path);
    }

    #[test]
    fn test_serialize_deserialize_roundtrip() {
        let account = make_account(42_000, 512);
        let serialized = serialize_account_for_cold_storage(&account);
        let deserialized = deserialize_account_from_cold_storage(&serialized).unwrap();

        assert_eq!(deserialized.lamports(), account.lamports());
        assert_eq!(deserialized.owner(), account.owner());
        assert_eq!(deserialized.executable(), account.executable());
        assert_eq!(deserialized.data(), account.data());
    }

    #[test]
    fn test_deserialize_truncated_data() {
        let result = deserialize_account_from_cold_storage(&[0u8; 10]);
        assert!(result.is_err());
    }

    #[test]
    fn test_cold_storage_path_distribution() {
        let base = PathBuf::from("/tmp/cold");
        let pk1 = Pubkey::new_unique();
        let pk2 = Pubkey::new_unique();

        let path1 = cold_storage_path_for_account(&pk1, &base);
        let path2 = cold_storage_path_for_account(&pk2, &base);

        // Paths should be different for different pubkeys
        assert_ne!(path1, path2);
        // Should have the expected directory structure
        assert!(path1.to_string_lossy().contains("/tmp/cold/"));
        assert!(path1.to_string_lossy().ends_with(".bin"));
    }

    #[test]
    fn test_archive_index() {
        let mut index = ArchiveIndex::new();
        assert!(index.is_empty());

        let pubkey = Pubkey::new_unique();
        let archived = ArchivedAccount {
            pubkey,
            archive_slot: 100,
            archive_epoch: 1,
            account_hash: Hash::default(),
            lamports_at_archive: 1000,
            data_len: 256,
            owner: Pubkey::new_unique(),
            executable: false,
            archive_timestamp: 0,
            storage_path: PathBuf::from("/tmp/test.bin"),
            merkle_proof: None,
        };

        index.insert(archived);
        assert_eq!(index.len(), 1);
        assert!(index.is_archived(&pubkey));
        assert_eq!(index.total_archived, 1);

        let retrieved = index.get(&pubkey).unwrap();
        assert_eq!(retrieved.archive_slot, 100);

        let removed = index.remove(&pubkey).unwrap();
        assert_eq!(removed.pubkey, pubkey);
        assert!(index.is_empty());
        assert_eq!(index.total_revived, 1);
    }

    #[test]
    fn test_merkle_proof_verify() {
        let data_hash = sha256_hash(b"test account data");
        let sibling = sha256_hash(b"sibling");

        // Build a simple one-level proof
        let root = combine_hashes(&data_hash, &sibling);

        let proof = MerkleProof {
            leaf_hash: data_hash,
            proof_hashes: vec![sibling],
            root_hash: root,
            proof_slot: 42,
            leaf_index: 0, // left child
        };

        assert!(proof.verify(&data_hash));
        assert!(!proof.verify(&sha256_hash(b"wrong data")));
    }

    #[test]
    fn test_revival_deposit_calculation() {
        let config = StateRentConfig {
            lamports_per_byte_year: 1000,
            ..Default::default()
        };

        // MIN_REVIVAL_RENT_YEARS = 2
        assert_eq!(config.revival_deposit(100), 200_000);
    }
}
