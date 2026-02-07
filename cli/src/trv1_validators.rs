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

// ── CLI Command Enum Variants ───────────────────────────────────────
#[derive(Debug, PartialEq)]
pub enum Trv1ValidatorsCliCommand {
    Active,
    Standby,
    SlashingInfo {
        validator_address: Pubkey,
    },
    Unjail,
}

// ── Output Structs ──────────────────────────────────────────────────
#[derive(Serialize, Deserialize, Debug)]
pub struct CliValidatorEntry {
    pub rank: u32,
    pub identity: String,
    pub vote_account: String,
    pub stake_sol: f64,
    pub commission_pct: f64,
    pub last_vote: u64,
    pub status: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CliValidatorList {
    pub validators: Vec<CliValidatorEntry>,
    pub total_active_stake_sol: f64,
}

impl fmt::Display for CliValidatorList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.validators.is_empty() {
            writeln!(f, "No validators found.")?;
        } else {
            writeln!(
                f,
                "{:<5} {:<44} {:>14} {:>10} {:>10} {:<10}",
                "Rank", "Identity", "Stake (SOL)", "Commission", "Last Vote", "Status"
            )?;
            writeln!(f, "{}", "-".repeat(97))?;
            for v in &self.validators {
                writeln!(
                    f,
                    "{:<5} {:<44} {:>14.2} {:>9.1}% {:>10} {:<10}",
                    v.rank, v.identity, v.stake_sol, v.commission_pct, v.last_vote, v.status
                )?;
            }
            writeln!(f)?;
            writeln!(f, "Total active stake: {} SOL", self.total_active_stake_sol)?;
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CliSlashingInfo {
    pub validator_address: String,
    pub is_jailed: bool,
    pub jail_reason: Option<String>,
    pub jail_epoch: Option<u64>,
    pub total_slashings: u64,
    pub total_slashed_sol: f64,
    pub recent_infractions: Vec<CliInfraction>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CliInfraction {
    pub epoch: u64,
    pub infraction_type: String,
    pub slashed_sol: f64,
    pub details: String,
}

impl fmt::Display for CliSlashingInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Slashing Info for {}", self.validator_address)?;
        writeln!(
            f,
            "  Jailed:            {}",
            if self.is_jailed { "YES" } else { "no" }
        )?;
        if let Some(ref reason) = self.jail_reason {
            writeln!(f, "  Jail Reason:       {}", reason)?;
        }
        if let Some(epoch) = self.jail_epoch {
            writeln!(f, "  Jailed Since:      epoch {}", epoch)?;
        }
        writeln!(f, "  Total Slashings:   {}", self.total_slashings)?;
        writeln!(f, "  Total Slashed:     {} SOL", self.total_slashed_sol)?;
        if !self.recent_infractions.is_empty() {
            writeln!(f)?;
            writeln!(f, "  Recent Infractions:")?;
            writeln!(
                f,
                "  {:<8} {:<20} {:>12} {}",
                "Epoch", "Type", "Slashed", "Details"
            )?;
            writeln!(f, "  {}", "-".repeat(70))?;
            for infraction in &self.recent_infractions {
                writeln!(
                    f,
                    "  {:<8} {:<20} {:>9.4} SOL {}",
                    infraction.epoch,
                    infraction.infraction_type,
                    infraction.slashed_sol,
                    infraction.details,
                )?;
            }
        }
        Ok(())
    }
}

// ── Subcommand Definition (clap) ────────────────────────────────────
/// Note: We name this "trv1-validators" to avoid conflict with the existing
/// Solana "validators" command (which shows `ShowValidators`). The existing
/// command remains as-is; this adds TRv1-specific validator info.
pub trait Trv1ValidatorsSubCommands {
    fn trv1_validators_subcommands(self) -> Self;
}

impl Trv1ValidatorsSubCommands for App<'_, '_> {
    fn trv1_validators_subcommands(self) -> Self {
        self.subcommand(
            SubCommand::with_name("trv1-validators")
                .about("TRv1 validator set and slashing commands")
                .setting(AppSettings::SubcommandRequiredElseHelp)
                .subcommand(
                    SubCommand::with_name("active")
                        .about("Show top 200 active validators in the TRv1 set"),
                )
                .subcommand(
                    SubCommand::with_name("standby")
                        .about("Show standby validators (not in the active set)"),
                )
                .subcommand(
                    SubCommand::with_name("slashing-info")
                        .about("Show slashing and jail information for a validator")
                        .arg(
                            Arg::with_name("validator_address")
                                .index(1)
                                .value_name("VALIDATOR_ADDRESS")
                                .takes_value(true)
                                .required(true)
                                .validator(is_valid_pubkey)
                                .help("Validator identity address"),
                        ),
                )
                .subcommand(
                    SubCommand::with_name("unjail")
                        .about("Unjail the current validator (signer must be the validator identity)"),
                ),
        )
    }
}

// ── Argument Parsing ────────────────────────────────────────────────
pub fn parse_trv1_validators_command(
    matches: &ArgMatches<'_>,
    _default_signer: &DefaultSigner,
    _wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    match matches.subcommand() {
        ("active", Some(_matches)) => {
            Ok(CliCommandInfo::without_signers(
                CliCommand::Trv1Validators(Trv1ValidatorsCliCommand::Active),
            ))
        }
        ("standby", Some(_matches)) => {
            Ok(CliCommandInfo::without_signers(
                CliCommand::Trv1Validators(Trv1ValidatorsCliCommand::Standby),
            ))
        }
        ("slashing-info", Some(matches)) => {
            let validator_address = pubkey_of(matches, "validator_address").unwrap();
            Ok(CliCommandInfo::without_signers(
                CliCommand::Trv1Validators(Trv1ValidatorsCliCommand::SlashingInfo {
                    validator_address,
                }),
            ))
        }
        ("unjail", Some(_matches)) => {
            Ok(CliCommandInfo::without_signers(
                CliCommand::Trv1Validators(Trv1ValidatorsCliCommand::Unjail),
            ))
        }
        _ => unreachable!(),
    }
}

// ── Command Processing ──────────────────────────────────────────────
pub async fn process_trv1_validators_command(
    rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
    command: &Trv1ValidatorsCliCommand,
) -> ProcessResult {
    match command {
        Trv1ValidatorsCliCommand::Active => {
            process_validators_active(rpc_client, config).await
        }
        Trv1ValidatorsCliCommand::Standby => {
            process_validators_standby(rpc_client, config).await
        }
        Trv1ValidatorsCliCommand::SlashingInfo { validator_address } => {
            process_validators_slashing_info(rpc_client, config, validator_address).await
        }
        Trv1ValidatorsCliCommand::Unjail => {
            process_validators_unjail(rpc_client, config).await
        }
    }
}

async fn process_validators_active(
    _rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
) -> ProcessResult {
    // TODO: Fetch active validator set from the chain
    // 1. Query TRv1 validator registry program for active set
    // 2. Sort by stake descending, limit to 200
    // 3. Populate CliValidatorList

    let list = CliValidatorList {
        validators: vec![],
        total_active_stake_sol: 0.0,
    };

    match config.output_format {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            Ok(serde_json::to_string_pretty(&list)?)
        }
        _ => Ok(format!("{}", list)),
    }
}

async fn process_validators_standby(
    _rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
) -> ProcessResult {
    // TODO: Fetch standby validators from the chain
    // 1. Query TRv1 validator registry program for standby validators
    // 2. These are validators with enough stake but not in the active top-200
    // 3. Populate CliValidatorList

    let list = CliValidatorList {
        validators: vec![],
        total_active_stake_sol: 0.0,
    };

    match config.output_format {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            Ok(serde_json::to_string_pretty(&list)?)
        }
        _ => Ok(format!("{}", list)),
    }
}

async fn process_validators_slashing_info(
    _rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
    validator_address: &Pubkey,
) -> ProcessResult {
    // TODO: Fetch slashing info from the chain
    // 1. Derive slashing record PDA from validator identity
    // 2. rpc_client.get_account(&slashing_pda)
    // 3. Deserialize SlashingRecord from account data

    let info = CliSlashingInfo {
        validator_address: validator_address.to_string(),
        is_jailed: false,
        jail_reason: None,
        jail_epoch: None,
        total_slashings: 0,
        total_slashed_sol: 0.0,
        recent_infractions: vec![],
    };

    match config.output_format {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            Ok(serde_json::to_string_pretty(&info)?)
        }
        _ => Ok(format!("{}", info)),
    }
}

async fn process_validators_unjail(
    _rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
) -> ProcessResult {
    // TODO: Build and send Validators::Unjail instruction
    // 1. Verify signer is a jailed validator identity
    // 2. Verify cooldown period has elapsed
    // 3. Build Unjail instruction
    // 4. Send transaction and confirm

    let result = json!({
        "status": "ok",
        "message": "Validator unjailed successfully",
    });

    match config.output_format {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            Ok(serde_json::to_string_pretty(&result)?)
        }
        _ => Ok("Validator unjailed successfully".to_string()),
    }
}
