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
pub enum DevRewardsCliCommand {
    Register {
        program_id: Pubkey,
        recipient: Pubkey,
    },
    Update {
        program_id: Pubkey,
        new_recipient: Pubkey,
    },
    Info {
        program_id: Pubkey,
    },
    Claim {
        program_id: Pubkey,
    },
}

// ── Output Structs ──────────────────────────────────────────────────
#[derive(Serialize, Deserialize, Debug)]
pub struct CliDevRewardsInfo {
    pub program_id: String,
    pub recipient: String,
    pub total_earned_sol: f64,
    pub pending_rewards_sol: f64,
    pub total_transactions: u64,
    pub registered_at: String,
}

impl fmt::Display for CliDevRewardsInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Developer Rewards")?;
        writeln!(f, "  Program:           {}", self.program_id)?;
        writeln!(f, "  Recipient:         {}", self.recipient)?;
        writeln!(f, "  Total Earned:      {} SOL", self.total_earned_sol)?;
        writeln!(f, "  Pending Rewards:   {} SOL", self.pending_rewards_sol)?;
        writeln!(f, "  Total Transactions:{}", self.total_transactions)?;
        writeln!(f, "  Registered:        {}", self.registered_at)?;
        Ok(())
    }
}

// ── Subcommand Definition (clap) ────────────────────────────────────
pub trait DevRewardsSubCommands {
    fn dev_rewards_subcommands(self) -> Self;
}

impl DevRewardsSubCommands for App<'_, '_> {
    fn dev_rewards_subcommands(self) -> Self {
        self.subcommand(
            SubCommand::with_name("dev-rewards")
                .about("TRv1 developer rewards commands")
                .setting(AppSettings::SubcommandRequiredElseHelp)
                .subcommand(
                    SubCommand::with_name("register")
                        .about("Register a program for developer rewards")
                        .arg(
                            Arg::with_name("program")
                                .long("program")
                                .value_name("PROGRAM_ID")
                                .takes_value(true)
                                .required(true)
                                .validator(is_valid_pubkey)
                                .help("Program ID to register for rewards"),
                        )
                        .arg(
                            Arg::with_name("recipient")
                                .long("recipient")
                                .value_name("ADDRESS")
                                .takes_value(true)
                                .required(true)
                                .validator(is_valid_pubkey)
                                .help("Address to receive developer rewards"),
                        ),
                )
                .subcommand(
                    SubCommand::with_name("update")
                        .about("Update the reward recipient for a registered program")
                        .arg(
                            Arg::with_name("program")
                                .long("program")
                                .value_name("PROGRAM_ID")
                                .takes_value(true)
                                .required(true)
                                .validator(is_valid_pubkey)
                                .help("Program ID to update"),
                        )
                        .arg(
                            Arg::with_name("new_recipient")
                                .long("new-recipient")
                                .value_name("ADDRESS")
                                .takes_value(true)
                                .required(true)
                                .validator(is_valid_pubkey)
                                .help("New address to receive developer rewards"),
                        ),
                )
                .subcommand(
                    SubCommand::with_name("info")
                        .about("Display developer rewards info for a program")
                        .arg(
                            Arg::with_name("program")
                                .long("program")
                                .value_name("PROGRAM_ID")
                                .takes_value(true)
                                .required(true)
                                .validator(is_valid_pubkey)
                                .help("Program ID to query"),
                        ),
                )
                .subcommand(
                    SubCommand::with_name("claim")
                        .about("Claim pending developer rewards for a program")
                        .arg(
                            Arg::with_name("program")
                                .long("program")
                                .value_name("PROGRAM_ID")
                                .takes_value(true)
                                .required(true)
                                .validator(is_valid_pubkey)
                                .help("Program ID to claim rewards for"),
                        ),
                ),
        )
    }
}

// ── Argument Parsing ────────────────────────────────────────────────
pub fn parse_dev_rewards_command(
    matches: &ArgMatches<'_>,
    _default_signer: &DefaultSigner,
    _wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    match matches.subcommand() {
        ("register", Some(matches)) => {
            let program_id = pubkey_of(matches, "program").unwrap();
            let recipient = pubkey_of(matches, "recipient").unwrap();
            Ok(CliCommandInfo::without_signers(
                CliCommand::DevRewards(DevRewardsCliCommand::Register {
                    program_id,
                    recipient,
                }),
            ))
        }
        ("update", Some(matches)) => {
            let program_id = pubkey_of(matches, "program").unwrap();
            let new_recipient = pubkey_of(matches, "new_recipient").unwrap();
            Ok(CliCommandInfo::without_signers(
                CliCommand::DevRewards(DevRewardsCliCommand::Update {
                    program_id,
                    new_recipient,
                }),
            ))
        }
        ("info", Some(matches)) => {
            let program_id = pubkey_of(matches, "program").unwrap();
            Ok(CliCommandInfo::without_signers(
                CliCommand::DevRewards(DevRewardsCliCommand::Info { program_id }),
            ))
        }
        ("claim", Some(matches)) => {
            let program_id = pubkey_of(matches, "program").unwrap();
            Ok(CliCommandInfo::without_signers(
                CliCommand::DevRewards(DevRewardsCliCommand::Claim { program_id }),
            ))
        }
        _ => unreachable!(),
    }
}

// ── Command Processing ──────────────────────────────────────────────
pub async fn process_dev_rewards_command(
    rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
    command: &DevRewardsCliCommand,
) -> ProcessResult {
    match command {
        DevRewardsCliCommand::Register {
            program_id,
            recipient,
        } => process_dev_rewards_register(rpc_client, config, program_id, recipient).await,
        DevRewardsCliCommand::Update {
            program_id,
            new_recipient,
        } => process_dev_rewards_update(rpc_client, config, program_id, new_recipient).await,
        DevRewardsCliCommand::Info { program_id } => {
            process_dev_rewards_info(rpc_client, config, program_id).await
        }
        DevRewardsCliCommand::Claim { program_id } => {
            process_dev_rewards_claim(rpc_client, config, program_id).await
        }
    }
}

async fn process_dev_rewards_register(
    _rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
    program_id: &Pubkey,
    recipient: &Pubkey,
) -> ProcessResult {
    // TODO: Build and send DevRewards::Register instruction
    // 1. Verify signer is the program's upgrade authority
    // 2. Derive dev-rewards PDA from program_id
    // 3. Build Register instruction with program_id and recipient
    // 4. Send transaction and confirm

    let result = json!({
        "status": "ok",
        "program_id": program_id.to_string(),
        "recipient": recipient.to_string(),
        "message": "Program registered for developer rewards",
    });

    match config.output_format {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            Ok(serde_json::to_string_pretty(&result)?)
        }
        _ => Ok(format!(
            "Program {} registered for developer rewards.\n  Recipient: {}",
            program_id, recipient
        )),
    }
}

async fn process_dev_rewards_update(
    _rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
    program_id: &Pubkey,
    new_recipient: &Pubkey,
) -> ProcessResult {
    // TODO: Build and send DevRewards::Update instruction
    // 1. Verify signer is the program's upgrade authority
    // 2. Build Update instruction with new recipient
    // 3. Send transaction and confirm

    let result = json!({
        "status": "ok",
        "program_id": program_id.to_string(),
        "new_recipient": new_recipient.to_string(),
    });

    match config.output_format {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            Ok(serde_json::to_string_pretty(&result)?)
        }
        _ => Ok(format!(
            "Developer rewards recipient updated for program {}.\n  New recipient: {}",
            program_id, new_recipient
        )),
    }
}

async fn process_dev_rewards_info(
    _rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
    program_id: &Pubkey,
) -> ProcessResult {
    // TODO: Fetch dev rewards state from the chain
    // 1. Derive dev-rewards PDA from program_id
    // 2. rpc_client.get_account(&dev_rewards_pda)
    // 3. Deserialize DevRewardsState from account data

    let info = CliDevRewardsInfo {
        program_id: program_id.to_string(),
        recipient: "TODO".to_string(),
        total_earned_sol: 0.0,
        pending_rewards_sol: 0.0,
        total_transactions: 0,
        registered_at: "TODO".to_string(),
    };

    match config.output_format {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            Ok(serde_json::to_string_pretty(&info)?)
        }
        _ => Ok(format!("{}", info)),
    }
}

async fn process_dev_rewards_claim(
    _rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
    program_id: &Pubkey,
) -> ProcessResult {
    // TODO: Build and send DevRewards::Claim instruction
    // 1. Fetch dev-rewards account to get pending amount
    // 2. Build Claim instruction
    // 3. Send transaction and confirm
    // 4. Return claimed amount

    let result = json!({
        "status": "ok",
        "program_id": program_id.to_string(),
        "claimed_sol": 0.0, // TODO: actual claimed amount
    });

    match config.output_format {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            Ok(serde_json::to_string_pretty(&result)?)
        }
        _ => Ok(format!(
            "Developer rewards claimed for program {}",
            program_id
        )),
    }
}
