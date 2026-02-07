macro_rules! ACCOUNT_STRING {
    () => {
        r#" Address is one of:
  * a base58-encoded public key
  * a path to a keypair file
  * a hyphen; signals a JSON-encoded keypair on stdin
  * the 'ASK' keyword; to recover a keypair via its seed phrase
  * a hardware wallet keypair URL (i.e. usb://ledger)"#
    };
}

macro_rules! pubkey {
    ($arg:expr, $help:expr) => {
        $arg.takes_value(true)
            .validator(is_valid_pubkey)
            .help(concat!($help, ACCOUNT_STRING!()))
    };
}

#[macro_use]
extern crate const_format;

pub mod address_lookup_table;
pub mod checks;
pub mod clap_app;
pub mod cli;
pub mod cluster_query;
pub mod compute_budget;
pub mod developer_rewards;
pub mod feature;
pub mod governance;
pub mod inflation;
pub mod memo;
pub mod network_info;
pub mod nonce;
pub mod passive_stake;
pub mod program;
pub mod program_v4;
pub mod spend_utils;
pub mod stake;
pub mod test_utils;
pub mod treasury;
pub mod trv1_validators;
pub mod validator_info;
pub mod vote;
pub mod wallet;
