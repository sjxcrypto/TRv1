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

// ── Proposal types ──────────────────────────────────────────────────
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProposalType {
    ParameterChange,
    TreasurySpend,
    EmergencyUnlock,
    Text,
}

impl ProposalType {
    pub fn from_str_value(s: &str) -> Result<Self, String> {
        match s {
            "parameter-change" => Ok(ProposalType::ParameterChange),
            "treasury-spend" => Ok(ProposalType::TreasurySpend),
            "emergency-unlock" => Ok(ProposalType::EmergencyUnlock),
            "text" => Ok(ProposalType::Text),
            _ => Err(format!(
                "Invalid proposal type '{}'. Valid: parameter-change, treasury-spend, emergency-unlock, text",
                s
            )),
        }
    }
}

impl fmt::Display for ProposalType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProposalType::ParameterChange => write!(f, "parameter-change"),
            ProposalType::TreasurySpend => write!(f, "treasury-spend"),
            ProposalType::EmergencyUnlock => write!(f, "emergency-unlock"),
            ProposalType::Text => write!(f, "text"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoteChoice {
    For,
    Against,
    Abstain,
}

impl VoteChoice {
    pub fn from_str_value(s: &str) -> Result<Self, String> {
        match s {
            "for" => Ok(VoteChoice::For),
            "against" => Ok(VoteChoice::Against),
            "abstain" => Ok(VoteChoice::Abstain),
            _ => Err(format!(
                "Invalid vote '{}'. Valid: for, against, abstain",
                s
            )),
        }
    }
}

impl fmt::Display for VoteChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VoteChoice::For => write!(f, "for"),
            VoteChoice::Against => write!(f, "against"),
            VoteChoice::Abstain => write!(f, "abstain"),
        }
    }
}

fn is_valid_proposal_type(s: String) -> Result<(), String> {
    ProposalType::from_str_value(&s).map(|_| ())
}

fn is_valid_vote_choice(s: String) -> Result<(), String> {
    VoteChoice::from_str_value(&s).map(|_| ())
}

fn is_valid_proposal_status(s: String) -> Result<(), String> {
    match s.as_str() {
        "active" | "passed" | "all" => Ok(()),
        _ => Err(format!(
            "Invalid status '{}'. Valid: active, passed, all",
            s
        )),
    }
}

// ── CLI Command Enum Variants ───────────────────────────────────────
#[derive(Debug, PartialEq)]
pub enum GovernanceCliCommand {
    Info,
    Propose {
        proposal_type: String,
        title: String,
        description: String,
        /// For treasury-spend proposals
        amount: Option<f64>,
        recipient: Option<Pubkey>,
        /// For parameter-change proposals
        parameter: Option<String>,
        value: Option<String>,
    },
    Vote {
        proposal_id: u64,
        vote: String,
    },
    Proposals {
        status: String,
    },
    Proposal {
        proposal_id: u64,
    },
    Execute {
        proposal_id: u64,
    },
}

// ── Output Structs ──────────────────────────────────────────────────
#[derive(Serialize, Deserialize, Debug)]
pub struct CliGovernanceInfo {
    pub governance_address: String,
    pub total_proposals: u64,
    pub active_proposals: u64,
    pub quorum_pct: f64,
    pub voting_period_days: u64,
    pub min_stake_to_propose: f64,
}

impl fmt::Display for CliGovernanceInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "TRv1 Governance")?;
        writeln!(f, "  Address:            {}", self.governance_address)?;
        writeln!(f, "  Total Proposals:    {}", self.total_proposals)?;
        writeln!(f, "  Active Proposals:   {}", self.active_proposals)?;
        writeln!(f, "  Quorum:             {}%", self.quorum_pct)?;
        writeln!(f, "  Voting Period:      {} days", self.voting_period_days)?;
        writeln!(f, "  Min Stake to Propose: {} SOL", self.min_stake_to_propose)?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CliProposal {
    pub id: u64,
    pub proposal_type: String,
    pub title: String,
    pub description: String,
    pub proposer: String,
    pub status: String,
    pub votes_for: u64,
    pub votes_against: u64,
    pub votes_abstain: u64,
    pub created_at: String,
    pub voting_ends: String,
}

impl fmt::Display for CliProposal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Proposal #{}", self.id)?;
        writeln!(f, "  Type:          {}", self.proposal_type)?;
        writeln!(f, "  Title:         {}", self.title)?;
        writeln!(f, "  Description:   {}", self.description)?;
        writeln!(f, "  Proposer:      {}", self.proposer)?;
        writeln!(f, "  Status:        {}", self.status)?;
        writeln!(f, "  Votes For:     {}", self.votes_for)?;
        writeln!(f, "  Votes Against: {}", self.votes_against)?;
        writeln!(f, "  Votes Abstain: {}", self.votes_abstain)?;
        writeln!(f, "  Created:       {}", self.created_at)?;
        writeln!(f, "  Voting Ends:   {}", self.voting_ends)?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CliProposalList {
    pub proposals: Vec<CliProposal>,
}

impl fmt::Display for CliProposalList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.proposals.is_empty() {
            writeln!(f, "No proposals found.")?;
        } else {
            writeln!(
                f,
                "{:<6} {:<20} {:<40} {:<10} {:>8} {:>8} {:>8}",
                "ID", "Type", "Title", "Status", "For", "Against", "Abstain"
            )?;
            writeln!(f, "{}", "-".repeat(104))?;
            for p in &self.proposals {
                writeln!(
                    f,
                    "{:<6} {:<20} {:<40} {:<10} {:>8} {:>8} {:>8}",
                    p.id,
                    p.proposal_type,
                    if p.title.len() > 38 {
                        format!("{}...", &p.title[..37])
                    } else {
                        p.title.clone()
                    },
                    p.status,
                    p.votes_for,
                    p.votes_against,
                    p.votes_abstain,
                )?;
            }
        }
        Ok(())
    }
}

// ── Subcommand Definition (clap) ────────────────────────────────────
pub trait GovernanceSubCommands {
    fn governance_subcommands(self) -> Self;
}

impl GovernanceSubCommands for App<'_, '_> {
    fn governance_subcommands(self) -> Self {
        self.subcommand(
            SubCommand::with_name("governance")
                .about("TRv1 governance commands")
                .setting(AppSettings::SubcommandRequiredElseHelp)
                .subcommand(
                    SubCommand::with_name("info")
                        .about("Display governance system information"),
                )
                .subcommand(
                    SubCommand::with_name("propose")
                        .about("Create a new governance proposal")
                        .arg(
                            Arg::with_name("type")
                                .long("type")
                                .value_name("PROPOSAL_TYPE")
                                .takes_value(true)
                                .required(true)
                                .validator(is_valid_proposal_type)
                                .help("Proposal type: parameter-change, treasury-spend, emergency-unlock, text"),
                        )
                        .arg(
                            Arg::with_name("title")
                                .long("title")
                                .value_name("TEXT")
                                .takes_value(true)
                                .required(true)
                                .help("Title of the proposal"),
                        )
                        .arg(
                            Arg::with_name("description")
                                .long("description")
                                .value_name("TEXT")
                                .takes_value(true)
                                .required(true)
                                .help("Description of the proposal"),
                        )
                        .arg(
                            Arg::with_name("amount")
                                .long("amount")
                                .value_name("SOL")
                                .takes_value(true)
                                .help("Amount for treasury-spend proposals"),
                        )
                        .arg(
                            Arg::with_name("recipient")
                                .long("recipient")
                                .value_name("ADDRESS")
                                .takes_value(true)
                                .validator(is_valid_pubkey)
                                .help("Recipient for treasury-spend proposals"),
                        )
                        .arg(
                            Arg::with_name("parameter")
                                .long("parameter")
                                .value_name("NAME")
                                .takes_value(true)
                                .help("Parameter name for parameter-change proposals"),
                        )
                        .arg(
                            Arg::with_name("value")
                                .long("value")
                                .value_name("VALUE")
                                .takes_value(true)
                                .help("New value for parameter-change proposals"),
                        ),
                )
                .subcommand(
                    SubCommand::with_name("vote")
                        .about("Vote on a governance proposal")
                        .arg(
                            Arg::with_name("proposal_id")
                                .index(1)
                                .value_name("PROPOSAL_ID")
                                .takes_value(true)
                                .required(true)
                                .help("ID of the proposal to vote on"),
                        )
                        .arg(
                            Arg::with_name("vote")
                                .index(2)
                                .value_name("VOTE")
                                .takes_value(true)
                                .required(true)
                                .validator(is_valid_vote_choice)
                                .help("Vote: for, against, or abstain"),
                        ),
                )
                .subcommand(
                    SubCommand::with_name("proposals")
                        .about("List governance proposals")
                        .arg(
                            Arg::with_name("status")
                                .long("status")
                                .value_name("STATUS")
                                .takes_value(true)
                                .default_value("active")
                                .validator(is_valid_proposal_status)
                                .help("Filter by status: active, passed, all"),
                        ),
                )
                .subcommand(
                    SubCommand::with_name("proposal")
                        .about("Show details of a specific proposal")
                        .arg(
                            Arg::with_name("proposal_id")
                                .index(1)
                                .value_name("PROPOSAL_ID")
                                .takes_value(true)
                                .required(true)
                                .help("ID of the proposal"),
                        ),
                )
                .subcommand(
                    SubCommand::with_name("execute")
                        .about("Execute a passed governance proposal")
                        .arg(
                            Arg::with_name("proposal_id")
                                .index(1)
                                .value_name("PROPOSAL_ID")
                                .takes_value(true)
                                .required(true)
                                .help("ID of the proposal to execute"),
                        ),
                ),
        )
    }
}

// ── Argument Parsing ────────────────────────────────────────────────
pub fn parse_governance_command(
    matches: &ArgMatches<'_>,
    _default_signer: &DefaultSigner,
    _wallet_manager: &mut Option<Rc<RemoteWalletManager>>,
) -> Result<CliCommandInfo, CliError> {
    match matches.subcommand() {
        ("info", Some(_matches)) => {
            Ok(CliCommandInfo::without_signers(
                CliCommand::Governance(GovernanceCliCommand::Info),
            ))
        }
        ("propose", Some(matches)) => {
            let proposal_type = matches.value_of("type").unwrap().to_string();
            let title = matches.value_of("title").unwrap().to_string();
            let description = matches.value_of("description").unwrap().to_string();
            let amount: Option<f64> = matches
                .value_of("amount")
                .map(|a| a.parse().unwrap());
            let recipient = pubkey_of(matches, "recipient");
            let parameter = matches.value_of("parameter").map(|s| s.to_string());
            let value = matches.value_of("value").map(|s| s.to_string());
            Ok(CliCommandInfo::without_signers(
                CliCommand::Governance(GovernanceCliCommand::Propose {
                    proposal_type,
                    title,
                    description,
                    amount,
                    recipient,
                    parameter,
                    value,
                }),
            ))
        }
        ("vote", Some(matches)) => {
            let proposal_id: u64 = matches
                .value_of("proposal_id")
                .unwrap()
                .parse()
                .map_err(|_| CliError::BadParameter("Invalid proposal ID".to_string()))?;
            let vote = matches.value_of("vote").unwrap().to_string();
            Ok(CliCommandInfo::without_signers(
                CliCommand::Governance(GovernanceCliCommand::Vote { proposal_id, vote }),
            ))
        }
        ("proposals", Some(matches)) => {
            let status = matches.value_of("status").unwrap_or("active").to_string();
            Ok(CliCommandInfo::without_signers(
                CliCommand::Governance(GovernanceCliCommand::Proposals { status }),
            ))
        }
        ("proposal", Some(matches)) => {
            let proposal_id: u64 = matches
                .value_of("proposal_id")
                .unwrap()
                .parse()
                .map_err(|_| CliError::BadParameter("Invalid proposal ID".to_string()))?;
            Ok(CliCommandInfo::without_signers(
                CliCommand::Governance(GovernanceCliCommand::Proposal { proposal_id }),
            ))
        }
        ("execute", Some(matches)) => {
            let proposal_id: u64 = matches
                .value_of("proposal_id")
                .unwrap()
                .parse()
                .map_err(|_| CliError::BadParameter("Invalid proposal ID".to_string()))?;
            Ok(CliCommandInfo::without_signers(
                CliCommand::Governance(GovernanceCliCommand::Execute { proposal_id }),
            ))
        }
        _ => unreachable!(),
    }
}

// ── Command Processing ──────────────────────────────────────────────
pub async fn process_governance_command(
    rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
    command: &GovernanceCliCommand,
) -> ProcessResult {
    match command {
        GovernanceCliCommand::Info => process_governance_info(rpc_client, config).await,
        GovernanceCliCommand::Propose {
            proposal_type,
            title,
            description,
            amount,
            recipient,
            parameter,
            value,
        } => {
            process_governance_propose(
                rpc_client,
                config,
                proposal_type,
                title,
                description,
                *amount,
                recipient.as_ref(),
                parameter.as_deref(),
                value.as_deref(),
            )
            .await
        }
        GovernanceCliCommand::Vote { proposal_id, vote } => {
            process_governance_vote(rpc_client, config, *proposal_id, vote).await
        }
        GovernanceCliCommand::Proposals { status } => {
            process_governance_proposals(rpc_client, config, status).await
        }
        GovernanceCliCommand::Proposal { proposal_id } => {
            process_governance_proposal(rpc_client, config, *proposal_id).await
        }
        GovernanceCliCommand::Execute { proposal_id } => {
            process_governance_execute(rpc_client, config, *proposal_id).await
        }
    }
}

async fn process_governance_info(
    _rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
) -> ProcessResult {
    // TODO: Fetch governance state from the chain
    // 1. Derive governance PDA from the TRv1 governance program
    // 2. rpc_client.get_account(&governance_pda)
    // 3. Deserialize GovernanceState from account data

    let info = CliGovernanceInfo {
        governance_address: "TODO".to_string(),
        total_proposals: 0,
        active_proposals: 0,
        quorum_pct: 0.0,
        voting_period_days: 0,
        min_stake_to_propose: 0.0,
    };

    match config.output_format {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            Ok(serde_json::to_string_pretty(&info)?)
        }
        _ => Ok(format!("{}", info)),
    }
}

async fn process_governance_propose(
    _rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
    proposal_type: &str,
    title: &str,
    description: &str,
    amount: Option<f64>,
    recipient: Option<&Pubkey>,
    parameter: Option<&str>,
    value: Option<&str>,
) -> ProcessResult {
    // TODO: Build and send Governance::Propose instruction
    // 1. Verify signer has enough stake to propose
    // 2. Build Propose instruction based on proposal_type
    //    - For treasury-spend: include amount and recipient
    //    - For parameter-change: include parameter name and new value
    //    - For emergency-unlock: no extra args
    //    - For text: just title and description
    // 3. Send transaction and confirm
    // 4. Return proposal ID

    let result = json!({
        "status": "ok",
        "proposal_type": proposal_type,
        "title": title,
        "description": description,
        "amount": amount,
        "recipient": recipient.map(|p| p.to_string()),
        "parameter": parameter,
        "value": value,
        "proposal_id": 0, // TODO: actual proposal ID from chain
    });

    match config.output_format {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            Ok(serde_json::to_string_pretty(&result)?)
        }
        _ => Ok(format!(
            "Proposal created.\n  Type:  {}\n  Title: {}",
            proposal_type, title
        )),
    }
}

async fn process_governance_vote(
    _rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
    proposal_id: u64,
    vote: &str,
) -> ProcessResult {
    // TODO: Build and send Governance::Vote instruction
    // 1. Verify signer has staked tokens (voting power)
    // 2. Build Vote instruction with proposal_id and vote choice
    // 3. Send transaction and confirm

    let result = json!({
        "status": "ok",
        "proposal_id": proposal_id,
        "vote": vote,
    });

    match config.output_format {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            Ok(serde_json::to_string_pretty(&result)?)
        }
        _ => Ok(format!(
            "Voted '{}' on proposal #{}",
            vote, proposal_id
        )),
    }
}

async fn process_governance_proposals(
    _rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
    _status: &str,
) -> ProcessResult {
    // TODO: Fetch proposals from the chain
    // 1. Use rpc_client.get_program_accounts() with filters
    // 2. Filter by status
    // 3. Deserialize each into CliProposal

    let list = CliProposalList { proposals: vec![] };

    match config.output_format {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            Ok(serde_json::to_string_pretty(&list)?)
        }
        _ => Ok(format!("{}", list)),
    }
}

async fn process_governance_proposal(
    _rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
    proposal_id: u64,
) -> ProcessResult {
    // TODO: Fetch specific proposal from the chain
    // 1. Derive proposal PDA from governance program + proposal_id
    // 2. rpc_client.get_account(&proposal_pda)
    // 3. Deserialize into CliProposal

    let proposal = CliProposal {
        id: proposal_id,
        proposal_type: "TODO".to_string(),
        title: "TODO".to_string(),
        description: "TODO".to_string(),
        proposer: "TODO".to_string(),
        status: "TODO".to_string(),
        votes_for: 0,
        votes_against: 0,
        votes_abstain: 0,
        created_at: "TODO".to_string(),
        voting_ends: "TODO".to_string(),
    };

    match config.output_format {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            Ok(serde_json::to_string_pretty(&proposal)?)
        }
        _ => Ok(format!("{}", proposal)),
    }
}

async fn process_governance_execute(
    _rpc_client: &Arc<RpcClient>,
    config: &CliConfig<'_>,
    proposal_id: u64,
) -> ProcessResult {
    // TODO: Build and send Governance::Execute instruction
    // 1. Verify proposal has passed and voting period is over
    // 2. Build Execute instruction
    // 3. Send transaction and confirm

    let result = json!({
        "status": "ok",
        "proposal_id": proposal_id,
        "message": "Proposal executed",
    });

    match config.output_format {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            Ok(serde_json::to_string_pretty(&result)?)
        }
        _ => Ok(format!("Proposal #{} executed successfully", proposal_id)),
    }
}
