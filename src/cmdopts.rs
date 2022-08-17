use std::net::SocketAddr;

use structopt::StructOpt;
use themelio_structs::Address;
// use tmelcrypt::Ed25519SK;

#[derive(Debug, StructOpt, Clone)]
pub struct CmdOpts {
    #[structopt(long, default_value = "127.0.0.1:11773")]
    /// Wallet API endpoint (daemon address of melwalletd)
    pub daemon: SocketAddr,
    #[structopt(long, default_value = "127.0.0.1:11773")]
    /// Alias to --daemon
    pub endpoint: SocketAddr,

    #[structopt(long)]
    /// set the bootstrap node address, otherwise defaults to {network}-bootstrap.themelio.org
    pub bootstrap: Option<SocketAddr>,

    #[structopt(long, default_value = "__melminter_")]
    /// Prefixes for the "owned" wallets created by the melminter.
    pub wallet_prefix: String,

    #[structopt(long)]
    /// Payout address for melminter profits.
    /// the program will send you 0.5 MEL once the mint-wallet balance more than 1.0 MEL.
    /// otherwise will do nothing and display warning if you doesn't specify one.
    pub payout: Option<Address>,

    #[structopt(long)]
    /// Force a certain number of threads. Defaults to the number of *physical* CPUs.
    pub threads: Option<usize>,

    #[structopt(long)]
    /// Whether melminter should be skipping the check that require balance >= 0.05 MEL
    pub skip_balance_check: bool,

    #[structopt(long)]
    /// Whether the negative-profit failsafe check should be disabled? (use for debugging only)
    pub disable_profit_failsafe: bool,

    #[structopt(long, default_value = "0.025")]
    /// Specify a "max lost" coins for balance safe (unit: MEL, for example 0.0321)
    pub balance_max_losts: String,

    #[structopt(long)]
    /// Whether enable debug output
    pub debug: bool,

    #[structopt(long)]
    /// Whether exporting the secret key of mint wallet. (default: do nothing)
    /// maybe only useful for without payout option
    pub export_sk: bool,

    #[structopt(long)]
    /// Manual specify a fixed difficulty here, otherwise melminter will automatic to select one.
    /// (PLEASE NOTE: this value should be chosen carefully!
    /// if you enter a too small value, your incomes may not be cover the expenses,
    /// because the ERG you minted may not be enough to cover the network fee for doscMint transactions)
    pub fixed_diff: Option<usize>,

    // #[structopt(long)]
    // /// Drain the fee reserve at the start.
    // pub drain_reserve: bool,
}
