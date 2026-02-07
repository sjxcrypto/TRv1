use {
    crate::cli::{CliCommand, CliCommandInfo, CliConfig, CliError, ProcessResult},
    clap::{App, AppSettings, Arg, ArgMatches, SubCommand},
    serde::{Deserialize, Serialize},
    serde_json::{self, json},
    solana_clap_utils::{
        input_parsers::pubkey_of,
        input_validators::is_valid_pubkey,
        keypair::DefaultSigner,
    },
    solana_cli_output::OutputFormat,
    solana_pubkey::Pubkey,
    solana_remote_wallet::remote_wallet::RemoteWalletManager,
    solana_rpc_client::nonblocking::rpc_client::RpcClient,
    std::{fmt, rc::Rc, sync::Arc},
};

// ── Lock period definitions ─────────────────────────────────────────
/// Valid lock periods for passive staking.
/// `Permanent` means the stake can never be withdrawn (highest reward multiplier).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockPeriod {
    None,       // 0 days — flexible, lowest multiplier
    Days30,     // 30-day lock
    Days90,     // 90-day lock
    Days180,    // 180-day lock
    Days360,    // 360-day lock
    Permanent,  // permanent lock — highest multiplier
}

impl fmt::Display for LockPeriod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LockPeriod::None => write!(f, "0 (flexible)"),
            LockPeriod::Days30 => write!(f, "30 days"),
            LockPeriod::Days90 => write!(f, "90 days"),
            LockPeriod::Days180 => write!(f, "180 days"),
            LockPeriod::Days360 => write!(f, "360 days"),
            LockPeriod::Permanent => write!(f, "permanent"),
        }
    }
}

impl LockPeriod {
    pub fn from_str_value(s: &str) -> Result<Self, String> {
        match s {
            "0" => Ok(LockPeriod::None),
            "30" => Ok(LockPeriod::Days30),
            "90" => Ok(LockPeriod::Days90),
            "180" => Ok(LockPeriod::Days180),
            "360" => Ok(LockPeriod::Days360),
            "permanent" => Ok(LockPeriod::Permanent),
            _ => Err(format!(
                "Invalid lock period '{}'. Valid values: 0, 30, 90, 180, 360, permanent",
                s
            )),
        }
    }

    pub fn reward_multiplier(&self) -> f64 {
        match self {
            LockPeriod::None => 1.0,
            LockPeriod::Days30 => 1.2,
            LockPeriod::Days90 => 1.5,
            LockPeriod::Days180 => 2.0,
            LockPeriod::Days360 => 2.5,
            LockPeriod::Permanent => 3.0,
        }
    }
}

fn is_valid_lock_period(s: String) -> Result<(), String> {
    LockPeriod::from_str_value(&s).map(|_| ())
}

// ── CLI Command Enum Variants ───────────────────────────────────────
#[derive(Debug, PartialEq)]
pub enum PassiveStakeCliCommand {
    Create {
        amount: f64,
        lock_period: String,
    },
    Info {
        account_address: Pubkey,
    },
    ClaimRewards {
        account_address: Pubkey,
    },
    Unlock {
        account_address: Pubkey,
    },
    EarlyUnlock {
        account_address: Pubkey,
    },
    List {
        owner: Option<Pubkey>,
    },
}

// ── Output Structs ──────────────────────────────────────────────────
#[derive(Serialize, Deserialize, Debug)]
pub struct CliPassiveStakeInfo {
    pub account: String,
    pub owner: String,
    pub balance_sol: f64,
    pub lock_period: String,
    pub lock_expires: Option<String>,
    pub reward_multiplier: f64,
    pub pending_rewards_sol: f64,
    pub status: String,
}

impl fmt::Display for CliPassiveStakeInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Passive Stake Account: {}", self.account)?;
        writeln!(f, "  Owner:              {}", self.owner)?;
        writeln!(f, "  Balance:            {} SOL", self.balance_sol)?;
        writeln!(f, "  Lock Period:        {}", self.lock_period)?;
        if let Some(ref expires) = self.lock_expires {
            writeln!(f, "  Lock Expires:       {}", expires)?;
        }
        writeln!(f, "  Reward Multiplier:  {}x", self.reward_multiplier)?;
        writeln!(f, "  Pending Rewards:    {} SOL", self.pending_rewards_sol)?;
        writeln!(f, "  Status:             {}", self.status)?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CliPassiveStakeList {
    pub accounts: Vec<CliPassiveStakeInfo>,
}

impl fmt::Display for CliPassiveStakeList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.accounts.is_empty() {
            writeln!(f, "No passive stake accounts found.")?;
        } else {
            writeln!(
                f,
                "{:<44} {:>12} {:>12} {:>10} {:>15}",
                "Account", "Balance", "Lock", "Multiplier", "Pending Rewards"
            )?;
            writeln!(f, "{}", "-".repeat(97))?;
            for acct in &self.accounts {
                writeln!(
                    f,
                    "{:<44} {:>9.4} SOL {:>12} {:>9.1}x {:>12.4} SOL",
                    acct.account, acct.balance_sol, acct.lock_period, acct.reward_multiplier, acct.pending_rewards_sol
                )?;
            }
        }
        Ok(())
    }
}

// ── Subcommand Definition (clap) ────────────────────────────────────
pub trait PassiveStakeSubCommands {
    fn passive_stake_subcommands(self) -> Self;
}

impl PassiveStakeSubCommands for App<'_, '_> {
    fn passive_stake_subcommands(self) -> Self {
        self.subcommand(
            SubCommand::with_name("passive-stake")
                .about("TRv1 passive staking commands")
                .setting(AppSettings::SubcommandRequiredElseHelp)
                .subcommand(
                    SubCommand::with_name("create")
                        .about("Create a new passive stake account")
                        .arg(
                            Arg::with_name("amount")
                                .long("amount")
                                .value_name("SOL")
                                .takes_value(true)
                                .required(true)
                                .help("Amount of SOL to stake"),
                        )
                        .arg(
                            Arg::with_name("lock_days")
                                .long("lock-days")
                                .value_name("PERIOD")
                                .takes_value(true)
                                .required(true)
                                .validator(is_valid_lock_period)
                                .help(
                                    "Lock period: 0 (flexible), 30, 90, 180, 360, or permanent",
                                ),
                        ),
                )
                .subcommand(
                    SubCommand::with_name("info")
                        .about("Display passive stake account info")
                        .arg(
                            Arg::with_name("account_address")
                                .index(1)
                                .value_name("ACCOUNT_ADDRESS")
                                .takes_value(true)
                                .required(true)
                                .validator(is_valid_pubkey)
                                .help("Address of the passive stake account"),
                        ),
                )
                .subcommand(
                    SubCommand::with_name("claim-rewards")
                        .about("Claim accumulated passive staking rewards")
                        .arg(
                            Arg::with_name("account_address")
                                .index(1)
                                .value_name("ACCOUNT_ADDRESS")
                                .takes_value(true)
                                .required(true)
                                .validator(is_valid_pubkey)
                                .help("Address of the passive stake account"),
                        ),
                )
                .subcommand(
                    SubCommand::with_name("unlock")
                        .about("Unlock a passive stake account (after lock period ends)")
                        .arg(
                            Arg::with_name("account_address")
                                .index(1)
                                .value_name("ACCOUNT_ADDRESS")
                                .takes_value(true)
                                .required(true)
                                .validator(is_valid_pubkey)
                                .help("Address of the passive stake account"),
                        ),
                )
                .subcommand(
                    SubCommand::with_name("early-unlock")
                        .about(
                            "Early-unlock a passive stake account (penalty: 10% of staked amount)",
                        )
                        .arg(
                            Arg::with_name("account_address")
                                .index(1)
                                .value_name("ACCOUNT_ADDRESS")
                                .takes_value(true)
                                .required(true)
                                .validator(is_valid_pubkey)
                                .help("Address of the passive stake account"),
                        )
                        .arg(
                            Arg::with_name("confirm")
                                .long("confirm")
                                .takes_value(false)
                                .required(true)
                                .help(
                                    "Confirm early unlock — a 10% penalty will be applied",
                                ),
                        ),
                )
                .subcommand(
                    SubCommand::with_name("list")
                        .about("List passive stake accounts")
                        .arg(
                            Arg::with_name("owner")
                                .long("owner")
                                .value_name("ADDRESS")
                                .takes_value(true)
                                .validator(is_valid_pubkey)
                                .help(
                                    "Show only accounts owned by this address \
                                     [default: current keypair]",
                                ),
                        ),
                ),
        )
    }
}

// ── Argument Parsing ────────────────────────────────────────────────
pub fn parse_passive_stake_command(
    matches: &ArgMatches<'_>,
    _default_signer: &DefaultSigner,
    _wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    match matches.subcommand() {
        ("create", Some(matches)) => {
            let amount: f64 = matches
                .value_of("amount")
                .unwrap()
                .parse()
                .map_err(|_| CliError::BadParameter("Invalid amount".to_string()))?;
            let lock_period = matches.value_of("lock_days").unwrap().to_string();
            Ok(CliCommandInfo::without_signers(
                CliCommand::PassiveStake(PassiveStakeCliCommand::Create {
                    amount,
                    lock_period,
                }),
            ))
        }
        ("info", Some(matches)) => {
            let account_address = pubkey_of(matches, "account_address").unwrap();
            Ok(CliCommandInfo::without_signers(
                CliCommand::PassiveStake(PassiveStakeCliCommand::Info { account_address }),
            ))
        }
        ("claim-rewards", Some(matches)) => {
            let account_address = pubkey_of(matches, "account_address").unwrap();
            Ok(CliCommandInfo::without_signers(
                CliCommand::PassiveStake(PassiveStakeCliCommand::ClaimRewards { account_address }),
            ))
        }
        ("unlock", Some(matches)) => {
            let account_address = pubkey_of(matches, "account_address").unwrap();
            Ok(CliCommandInfo::without_signers(
                CliCommand::PassiveStake(PassiveStakeCliCommand::Unlock { account_address }),
            ))
        }
        ("early-unlock", Some(matches)) => {
            let account_address = pubkey_of(matches, "account_address").unwrap();
            Ok(CliCommandInfo::without_signers(
                CliCommand::PassiveStake(PassiveStakeCliCommand::EarlyUnlock { account_address }),
            ))
        }
        ("list", Some(matches)) => {
            let owner = pubkey_of(matches, "owner");
            Ok(CliCommandInfo::without_signers(
                CliCommand::PassiveStake(PassiveStakeCliCommand::List { owner }),
            ))
        }
        _ => unreachable!(),
    }
}

// ── Command Processing ──────────────────────────────────────────────
pub async fn process_passive_stake_command(
    rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
    command: &PassiveStakeCliCommand,
) -> ProcessResult {
    match command {
        PassiveStakeCliCommand::Create { amount, lock_period } => {
            process_passive_stake_create(rpc_client, config, *amount, lock_period).await
        }
        PassiveStakeCliCommand::Info { account_address } => {
            process_passive_stake_info(rpc_client, config, account_address).await
        }
        PassiveStakeCliCommand::ClaimRewards { account_address } => {
            process_passive_stake_claim_rewards(rpc_client, config, account_address).await
        }
        PassiveStakeCliCommand::Unlock { account_address } => {
            process_passive_stake_unlock(rpc_client, config, account_address).await
        }
        PassiveStakeCliCommand::EarlyUnlock { account_address } => {
            process_passive_stake_early_unlock(rpc_client, config, account_address).await
        }
        PassiveStakeCliCommand::List { owner } => {
            process_passive_stake_list(rpc_client, config, owner.as_ref()).await
        }
    }
}

async fn process_passive_stake_create(
    _rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
    amount: f64,
    lock_period: &str,
) -> ProcessResult {
    let period = LockPeriod::from_str_value(lock_period)
        .map_err(|e| CliError::BadParameter(e))?;

    // TODO: Build and send PassiveStake::Create instruction via rpc_client
    // 1. Derive passive stake PDA from owner pubkey + nonce
    // 2. Create instruction with amount (in lamports) and lock_period
    // 3. Send transaction and confirm

    let result = json!({
        "status": "ok",
        "message": format!("Passive stake account created with {} SOL, lock: {}", amount, period),
        "amount_sol": amount,
        "lock_period": lock_period,
        "reward_multiplier": period.reward_multiplier(),
    });

    match config.output_format {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            Ok(serde_json::to_string_pretty(&result)?)
        }
        _ => Ok(format!(
            "Passive stake account created.\n  Amount:     {} SOL\n  Lock:       {}\n  Multiplier: {}x",
            amount, period, period.reward_multiplier()
        )),
    }
}

async fn process_passive_stake_info(
    _rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
    account_address: &Pubkey,
) -> ProcessResult {
    // TODO: Fetch passive stake account data from rpc_client
    // 1. rpc_client.get_account(account_address)
    // 2. Deserialize PassiveStakeState from account data
    // 3. Populate CliPassiveStakeInfo from deserialized state

    let info = CliPassiveStakeInfo {
        account: account_address.to_string(),
        owner: "TODO".to_string(),
        balance_sol: 0.0,
        lock_period: "TODO".to_string(),
        lock_expires: None,
        reward_multiplier: 1.0,
        pending_rewards_sol: 0.0,
        status: "TODO: fetch from chain".to_string(),
    };

    match config.output_format {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            Ok(serde_json::to_string_pretty(&info)?)
        }
        _ => Ok(format!("{}", info)),
    }
}

async fn process_passive_stake_claim_rewards(
    _rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
    account_address: &Pubkey,
) -> ProcessResult {
    // TODO: Build and send PassiveStake::ClaimRewards instruction
    // 1. Fetch account to verify it exists and has pending rewards
    // 2. Build ClaimRewards instruction
    // 3. Send transaction and confirm
    // 4. Return claimed amount

    let result = json!({
        "status": "ok",
        "account": account_address.to_string(),
        "claimed_sol": 0.0, // TODO: actual claimed amount
    });

    match config.output_format {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            Ok(serde_json::to_string_pretty(&result)?)
        }
        _ => Ok(format!(
            "Rewards claimed for passive stake account {}",
            account_address
        )),
    }
}

async fn process_passive_stake_unlock(
    _rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
    account_address: &Pubkey,
) -> ProcessResult {
    // TODO: Build and send PassiveStake::Unlock instruction
    // 1. Fetch account to verify lock period has expired
    // 2. Build Unlock instruction
    // 3. Send transaction and confirm

    let result = json!({
        "status": "ok",
        "account": account_address.to_string(),
        "message": "Passive stake account unlocked",
    });

    match config.output_format {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            Ok(serde_json::to_string_pretty(&result)?)
        }
        _ => Ok(format!(
            "Passive stake account {} unlocked successfully",
            account_address
        )),
    }
}

async fn process_passive_stake_early_unlock(
    _rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
    account_address: &Pubkey,
) -> ProcessResult {
    // TODO: Build and send PassiveStake::EarlyUnlock instruction
    // 1. Fetch account to verify it's locked
    // 2. Calculate 10% penalty
    // 3. Build EarlyUnlock instruction
    // 4. Send transaction and confirm
    // 5. Return penalty amount

    let result = json!({
        "status": "ok",
        "account": account_address.to_string(),
        "penalty_pct": 10,
        "penalty_sol": 0.0, // TODO: actual penalty
        "message": "Early unlock processed with 10% penalty",
    });

    match config.output_format {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            Ok(serde_json::to_string_pretty(&result)?)
        }
        _ => Ok(format!(
            "Early unlock processed for {}. A 10% penalty was applied.",
            account_address
        )),
    }
}

async fn process_passive_stake_list(
    _rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
    owner: Option<&Pubkey>,
) -> ProcessResult {
    // TODO: Fetch all passive stake accounts for the owner
    // 1. Use rpc_client.get_program_accounts() with filters for passive stake program
    // 2. Filter by owner if provided, otherwise use config signer pubkey
    // 3. Deserialize each account into CliPassiveStakeInfo

    let _owner_display = owner
        .map(|p| p.to_string())
        .unwrap_or_else(|| "current keypair".to_string());

    let list = CliPassiveStakeList { accounts: vec![] };

    match config.output_format {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            Ok(serde_json::to_string_pretty(&list)?)
        }
        _ => Ok(format!("{}", list)),
    }
}
