use std::net::SocketAddr;

use structopt::StructOpt;
use themelio_structs::Address;
// use tmelcrypt::Ed25519SK;

#[derive(Debug, StructOpt, Clone)]
pub struct CmdOpts {
    #[structopt(long)]
    /// Wallet API endpoint. For example localhost:11773
    pub daemon: Option<SocketAddr>,

    #[structopt(long, default_value = "__melminter_")]
    /// Prefixes for the "owned" wallets created by the melminter.
    pub wallet_prefix: String,

    #[structopt(long)]
    /// Payout address for melminter profits.
    pub payout: Address,

    #[structopt(long)]
    /// Whether to use testnet
    pub testnet: bool,

    #[structopt(long)]
    /// Force a certain number of threads. Defaults to the number of *physical* CPUs.
    pub threads: Option<usize>,

    #[structopt(long)]
    /// Is this program should be skipping the check that require balance is greater than or equal to 0.05
    pub skip_fee_check: bool,

    #[structopt(long)]
    /// Is the negative-profit failsafe check should be disabled? (PLEASE NOTE: use only for debugging!)
    pub disable_profit_failsafe: bool,

    #[structopt(long)]
    /// Manual specify a "max losts" value for balance safe, or defaults to 0.02 (unit: MEL, for example 0.0321)
    pub balance_max_losts: Option<String>,

    #[structopt(long)]
    /// enable debug output (default: disabled)
    pub debug: bool,

    #[structopt(long)]
    /// If you want, you can specify a fixed difficulty here, otherwise this program will automatic to select one. (PLEASE NOTE: this value should be chosen carefully! if you enter a too small value, your incomes may not be cover the expenses, because the ERG you minted may not be enough to cover the network fee for doscMint transactions)
    pub fixed_diff: Option<usize>,

    // #[structopt(long)]
    // /// Drain the fee reserve at the start.
    // pub drain_reserve: bool,
}
