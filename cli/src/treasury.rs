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
pub enum TreasuryCliCommand {
    Info,
    Disburse {
        amount: f64,
        recipient: Pubkey,
        memo: String,
    },
    UpdateAuthority {
        new_authority: Pubkey,
    },
}

// ── Output Structs ──────────────────────────────────────────────────
#[derive(Serialize, Deserialize, Debug)]
pub struct CliTreasuryInfo {
    pub treasury_address: String,
    pub authority: String,
    pub balance_sol: f64,
    pub total_disbursed_sol: f64,
    pub total_inflows_sol: f64,
    pub pending_proposals: u64,
}

impl fmt::Display for CliTreasuryInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "TRv1 Treasury")?;
        writeln!(f, "  Address:           {}", self.treasury_address)?;
        writeln!(f, "  Authority:         {}", self.authority)?;
        writeln!(f, "  Balance:           {} SOL", self.balance_sol)?;
        writeln!(f, "  Total Disbursed:   {} SOL", self.total_disbursed_sol)?;
        writeln!(f, "  Total Inflows:     {} SOL", self.total_inflows_sol)?;
        writeln!(f, "  Pending Proposals: {}", self.pending_proposals)?;
        Ok(())
    }
}

// ── Subcommand Definition (clap) ────────────────────────────────────
pub trait TreasurySubCommands {
    fn treasury_subcommands(self) -> Self;
}

impl TreasurySubCommands for App<'_, '_> {
    fn treasury_subcommands(self) -> Self {
        self.subcommand(
            SubCommand::with_name("treasury")
                .about("TRv1 treasury management commands")
                .setting(AppSettings::SubcommandRequiredElseHelp)
                .subcommand(
                    SubCommand::with_name("info")
                        .about("Display current treasury status"),
                )
                .subcommand(
                    SubCommand::with_name("disburse")
                        .about("Disburse funds from the treasury (requires authority)")
                        .arg(
                            Arg::with_name("amount")
                                .long("amount")
                                .value_name("SOL")
                                .takes_value(true)
                                .required(true)
                                .help("Amount of SOL to disburse"),
                        )
                        .arg(
                            Arg::with_name("recipient")
                                .long("recipient")
                                .value_name("ADDRESS")
                                .takes_value(true)
                                .required(true)
                                .validator(is_valid_pubkey)
                                .help("Recipient address for disbursement"),
                        )
                        .arg(
                            Arg::with_name("memo")
                                .long("memo")
                                .value_name("TEXT")
                                .takes_value(true)
                                .required(true)
                                .help("Memo describing the reason for disbursement"),
                        ),
                )
                .subcommand(
                    SubCommand::with_name("update-authority")
                        .about("Transfer treasury authority to a new address")
                        .arg(
                            Arg::with_name("new_authority")
                                .long("new-authority")
                                .value_name("ADDRESS")
                                .takes_value(true)
                                .required(true)
                                .validator(is_valid_pubkey)
                                .help("New treasury authority address"),
                        ),
                ),
        )
    }
}

// ── Argument Parsing ────────────────────────────────────────────────
pub fn parse_treasury_command(
    matches: &ArgMatches<'_>,
    _default_signer: &DefaultSigner,
    _wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    match matches.subcommand() {
        ("info", Some(_matches)) => {
            Ok(CliCommandInfo::without_signers(
                CliCommand::Treasury(TreasuryCliCommand::Info),
            ))
        }
        ("disburse", Some(matches)) => {
            let amount: f64 = matches
                .value_of("amount")
                .unwrap()
                .parse()
                .map_err(|_| CliError::BadParameter("Invalid amount".to_string()))?;
            let recipient = pubkey_of(matches, "recipient").unwrap();
            let memo = matches.value_of("memo").unwrap().to_string();
            Ok(CliCommandInfo::without_signers(
                CliCommand::Treasury(TreasuryCliCommand::Disburse {
                    amount,
                    recipient,
                    memo,
                }),
            ))
        }
        ("update-authority", Some(matches)) => {
            let new_authority = pubkey_of(matches, "new_authority").unwrap();
            Ok(CliCommandInfo::without_signers(
                CliCommand::Treasury(TreasuryCliCommand::UpdateAuthority { new_authority }),
            ))
        }
        _ => unreachable!(),
    }
}

// ── Command Processing ──────────────────────────────────────────────
pub async fn process_treasury_command(
    rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
    command: &TreasuryCliCommand,
) -> ProcessResult {
    match command {
        TreasuryCliCommand::Info => process_treasury_info(rpc_client, config).await,
        TreasuryCliCommand::Disburse {
            amount,
            recipient,
            memo,
        } => process_treasury_disburse(rpc_client, config, *amount, recipient, memo).await,
        TreasuryCliCommand::UpdateAuthority { new_authority } => {
            process_treasury_update_authority(rpc_client, config, new_authority).await
        }
    }
}

async fn process_treasury_info(
    _rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
) -> ProcessResult {
    // TODO: Fetch treasury account state from the chain
    // 1. Derive treasury PDA from the TRv1 treasury program
    // 2. rpc_client.get_account(&treasury_pda)
    // 3. Deserialize TreasuryState from account data
    // 4. Populate CliTreasuryInfo

    let info = CliTreasuryInfo {
        treasury_address: "TODO".to_string(),
        authority: "TODO".to_string(),
        balance_sol: 0.0,
        total_disbursed_sol: 0.0,
        total_inflows_sol: 0.0,
        pending_proposals: 0,
    };

    match config.output_format {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            Ok(serde_json::to_string_pretty(&info)?)
        }
        _ => Ok(format!("{}", info)),
    }
}

async fn process_treasury_disburse(
    _rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
    amount: f64,
    recipient: &Pubkey,
    memo: &str,
) -> ProcessResult {
    // TODO: Build and send Treasury::Disburse instruction
    // 1. Verify signer is the treasury authority
    // 2. Build Disburse instruction with amount (lamports), recipient, memo
    // 3. Send transaction and confirm
    // 4. Return transaction signature

    let result = json!({
        "status": "ok",
        "amount_sol": amount,
        "recipient": recipient.to_string(),
        "memo": memo,
        "signature": "TODO",
    });

    match config.output_format {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            Ok(serde_json::to_string_pretty(&result)?)
        }
        _ => Ok(format!(
            "Treasury disbursement: {} SOL → {}\n  Memo: {}",
            amount, recipient, memo
        )),
    }
}

async fn process_treasury_update_authority(
    _rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
    new_authority: &Pubkey,
) -> ProcessResult {
    // TODO: Build and send Treasury::UpdateAuthority instruction
    // 1. Verify signer is the current treasury authority
    // 2. Build UpdateAuthority instruction with new_authority
    // 3. Send transaction and confirm

    let result = json!({
        "status": "ok",
        "new_authority": new_authority.to_string(),
        "message": "Treasury authority updated",
    });

    match config.output_format {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            Ok(serde_json::to_string_pretty(&result)?)
        }
        _ => Ok(format!(
            "Treasury authority transferred to {}",
            new_authority
        )),
    }
}
