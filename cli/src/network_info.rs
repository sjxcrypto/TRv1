use {
    crate::cli::{CliCommand, CliCommandInfo, CliConfig, CliError, ProcessResult},
    clap::{App, AppSettings, ArgMatches, SubCommand},
    serde::{Deserialize, Serialize},
    serde_json::{self},
    solana_clap_utils::keypair::DefaultSigner,
    solana_cli_output::OutputFormat,
    solana_remote_wallet::remote_wallet::RemoteWalletManager,
    solana_rpc_client::nonblocking::rpc_client::RpcClient,
    std::{fmt, rc::Rc, sync::Arc},
};

// ── CLI Command Enum Variants ───────────────────────────────────────
#[derive(Debug, PartialEq)]
pub enum NetworkInfoCliCommand {
    Info,
    FeeInfo,
    InflationInfo,
    EpochInfo,
}

// ── Output Structs ──────────────────────────────────────────────────
#[derive(Serialize, Deserialize, Debug)]
pub struct CliNetworkInfo {
    pub block_height: u64,
    pub current_epoch: u64,
    pub current_slot: u64,
    pub base_fee_lamports: u64,
    pub total_staked_sol: f64,
    pub active_validators: u64,
    pub standby_validators: u64,
    pub tps: f64,
}

impl fmt::Display for CliNetworkInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "TRv1 Network Info")?;
        writeln!(f, "  Block Height:       {}", self.block_height)?;
        writeln!(f, "  Current Epoch:      {}", self.current_epoch)?;
        writeln!(f, "  Current Slot:       {}", self.current_slot)?;
        writeln!(f, "  Base Fee:           {} lamports", self.base_fee_lamports)?;
        writeln!(f, "  Total Staked:       {} SOL", self.total_staked_sol)?;
        writeln!(f, "  Active Validators:  {}", self.active_validators)?;
        writeln!(f, "  Standby Validators: {}", self.standby_validators)?;
        writeln!(f, "  TPS:                {:.1}", self.tps)?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CliFeeInfo {
    pub base_fee_lamports: u64,
    pub base_fee_sol: f64,
    pub priority_fee_median_lamports: u64,
    pub utilization_pct: f64,
    pub fee_burn_pct: f64,
    pub recent_fee_history: Vec<CliFeeHistoryEntry>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CliFeeHistoryEntry {
    pub slot: u64,
    pub base_fee_lamports: u64,
    pub utilization_pct: f64,
}

impl fmt::Display for CliFeeInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "TRv1 Fee Info")?;
        writeln!(f, "  Base Fee:             {} lamports ({} SOL)", self.base_fee_lamports, self.base_fee_sol)?;
        writeln!(f, "  Priority Fee (median):{} lamports", self.priority_fee_median_lamports)?;
        writeln!(f, "  Network Utilization:  {:.1}%", self.utilization_pct)?;
        writeln!(f, "  Fee Burn Rate:        {:.1}%", self.fee_burn_pct)?;
        if !self.recent_fee_history.is_empty() {
            writeln!(f)?;
            writeln!(f, "  Recent Fee History:")?;
            writeln!(f, "  {:<12} {:>14} {:>14}", "Slot", "Base Fee", "Utilization")?;
            writeln!(f, "  {}", "-".repeat(44))?;
            for entry in &self.recent_fee_history {
                writeln!(
                    f,
                    "  {:<12} {:>10} lam {:>12.1}%",
                    entry.slot, entry.base_fee_lamports, entry.utilization_pct
                )?;
            }
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CliInflationInfo {
    pub current_inflation_rate_pct: f64,
    pub target_inflation_rate_pct: f64,
    pub staking_participation_pct: f64,
    pub total_supply_sol: f64,
    pub circulating_supply_sol: f64,
    pub staked_supply_sol: f64,
    pub annual_staking_yield_pct: f64,
}

impl fmt::Display for CliInflationInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "TRv1 Inflation Info")?;
        writeln!(f, "  Current Inflation:     {:.2}%", self.current_inflation_rate_pct)?;
        writeln!(f, "  Target Inflation:      {:.2}%", self.target_inflation_rate_pct)?;
        writeln!(f, "  Staking Participation: {:.1}%", self.staking_participation_pct)?;
        writeln!(f, "  Total Supply:          {} SOL", self.total_supply_sol)?;
        writeln!(f, "  Circulating Supply:    {} SOL", self.circulating_supply_sol)?;
        writeln!(f, "  Staked Supply:         {} SOL", self.staked_supply_sol)?;
        writeln!(f, "  Annual Staking Yield:  {:.2}%", self.annual_staking_yield_pct)?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CliEpochInfoTrv1 {
    pub current_epoch: u64,
    pub epoch_start_slot: u64,
    pub epoch_end_slot: u64,
    pub slots_in_epoch: u64,
    pub slots_completed: u64,
    pub slots_remaining: u64,
    pub epoch_progress_pct: f64,
    pub estimated_time_remaining: String,
}

impl fmt::Display for CliEpochInfoTrv1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "TRv1 Epoch Info")?;
        writeln!(f, "  Current Epoch:      {}", self.current_epoch)?;
        writeln!(f, "  Start Slot:         {}", self.epoch_start_slot)?;
        writeln!(f, "  End Slot:           {}", self.epoch_end_slot)?;
        writeln!(f, "  Slots in Epoch:     {}", self.slots_in_epoch)?;
        writeln!(f, "  Slots Completed:    {}", self.slots_completed)?;
        writeln!(f, "  Slots Remaining:    {}", self.slots_remaining)?;
        writeln!(f, "  Progress:           {:.1}%", self.epoch_progress_pct)?;
        writeln!(f, "  Time Remaining:     {}", self.estimated_time_remaining)?;
        Ok(())
    }
}

// ── Subcommand Definition (clap) ────────────────────────────────────
pub trait NetworkInfoSubCommands {
    fn network_info_subcommands(self) -> Self;
}

impl NetworkInfoSubCommands for App<'_, '_> {
    fn network_info_subcommands(self) -> Self {
        self.subcommand(
            SubCommand::with_name("network")
                .about("TRv1 network information commands")
                .setting(AppSettings::SubcommandRequiredElseHelp)
                .subcommand(
                    SubCommand::with_name("info")
                        .about("Show network overview: block height, epoch, base fee, staking stats"),
                )
                .subcommand(
                    SubCommand::with_name("fee-info")
                        .about("Show current base fee, utilization, and fee history"),
                )
                .subcommand(
                    SubCommand::with_name("inflation-info")
                        .about("Show current inflation rate and staking participation"),
                )
                .subcommand(
                    SubCommand::with_name("epoch-info")
                        .about("Show current epoch details and time remaining"),
                ),
        )
    }
}

// ── Argument Parsing ────────────────────────────────────────────────
pub fn parse_network_info_command(
    matches: &ArgMatches<'_>,
    _default_signer: &DefaultSigner,
    _wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    match matches.subcommand() {
        ("info", Some(_matches)) => {
            Ok(CliCommandInfo::without_signers(
                CliCommand::NetworkInfo(NetworkInfoCliCommand::Info),
            ))
        }
        ("fee-info", Some(_matches)) => {
            Ok(CliCommandInfo::without_signers(
                CliCommand::NetworkInfo(NetworkInfoCliCommand::FeeInfo),
            ))
        }
        ("inflation-info", Some(_matches)) => {
            Ok(CliCommandInfo::without_signers(
                CliCommand::NetworkInfo(NetworkInfoCliCommand::InflationInfo),
            ))
        }
        ("epoch-info", Some(_matches)) => {
            Ok(CliCommandInfo::without_signers(
                CliCommand::NetworkInfo(NetworkInfoCliCommand::EpochInfo),
            ))
        }
        _ => unreachable!(),
    }
}

// ── Command Processing ──────────────────────────────────────────────
pub async fn process_network_info_command(
    rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
    command: &NetworkInfoCliCommand,
) -> ProcessResult {
    match command {
        NetworkInfoCliCommand::Info => process_network_info(rpc_client, config).await,
        NetworkInfoCliCommand::FeeInfo => process_fee_info(rpc_client, config).await,
        NetworkInfoCliCommand::InflationInfo => process_inflation_info(rpc_client, config).await,
        NetworkInfoCliCommand::EpochInfo => process_epoch_info(rpc_client, config).await,
    }
}

async fn process_network_info(
    _rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
) -> ProcessResult {
    // TODO: Fetch network info from the chain
    // 1. rpc_client.get_block_height()
    // 2. rpc_client.get_epoch_info()
    // 3. Query TRv1 fee controller for base fee
    // 4. Query TRv1 validator registry for staking stats
    // 5. rpc_client.get_recent_performance_samples() for TPS

    let info = CliNetworkInfo {
        block_height: 0,
        current_epoch: 0,
        current_slot: 0,
        base_fee_lamports: 0,
        total_staked_sol: 0.0,
        active_validators: 0,
        standby_validators: 0,
        tps: 0.0,
    };

    match config.output_format {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            Ok(serde_json::to_string_pretty(&info)?)
        }
        _ => Ok(format!("{}", info)),
    }
}

async fn process_fee_info(
    _rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
) -> ProcessResult {
    // TODO: Fetch fee info from the chain
    // 1. Query TRv1 fee controller program for current base fee
    // 2. rpc_client.get_recent_prioritization_fees() for priority fee data
    // 3. Query utilization metrics
    // 4. Get recent fee history from fee controller state

    let info = CliFeeInfo {
        base_fee_lamports: 0,
        base_fee_sol: 0.0,
        priority_fee_median_lamports: 0,
        utilization_pct: 0.0,
        fee_burn_pct: 0.0,
        recent_fee_history: vec![],
    };

    match config.output_format {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            Ok(serde_json::to_string_pretty(&info)?)
        }
        _ => Ok(format!("{}", info)),
    }
}

async fn process_inflation_info(
    _rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
) -> ProcessResult {
    // TODO: Fetch inflation info from the chain
    // 1. rpc_client.get_inflation_rate()
    // 2. rpc_client.get_supply()
    // 3. Calculate staking participation = staked / circulating
    // 4. Calculate annual yield based on inflation and participation

    let info = CliInflationInfo {
        current_inflation_rate_pct: 0.0,
        target_inflation_rate_pct: 0.0,
        staking_participation_pct: 0.0,
        total_supply_sol: 0.0,
        circulating_supply_sol: 0.0,
        staked_supply_sol: 0.0,
        annual_staking_yield_pct: 0.0,
    };

    match config.output_format {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            Ok(serde_json::to_string_pretty(&info)?)
        }
        _ => Ok(format!("{}", info)),
    }
}

async fn process_epoch_info(
    _rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
) -> ProcessResult {
    // TODO: Fetch epoch info from the chain
    // 1. rpc_client.get_epoch_info()
    // 2. rpc_client.get_epoch_schedule()
    // 3. Calculate slots remaining and estimated time
    //    (slot time ~400ms for TRv1)

    let info = CliEpochInfoTrv1 {
        current_epoch: 0,
        epoch_start_slot: 0,
        epoch_end_slot: 0,
        slots_in_epoch: 0,
        slots_completed: 0,
        slots_remaining: 0,
        epoch_progress_pct: 0.0,
        estimated_time_remaining: "TODO".to_string(),
    };

    match config.output_format {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            Ok(serde_json::to_string_pretty(&info)?)
        }
        _ => Ok(format!("{}", info)),
    }
}
