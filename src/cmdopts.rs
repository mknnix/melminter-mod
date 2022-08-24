use std::net::SocketAddr;

use structopt::StructOpt;
use themelio_structs::{ Address, NetID };
// use tmelcrypt::Ed25519SK;

#[derive(Debug, StructOpt, Clone)]
pub struct CmdOpts {
    #[structopt(long)]
    /// Specify the network type, should be mainnet/testnet/custom-xxx. otherwise auto detect which network of connected melwalletd.
    /// (NOTE: you should not manual control fixed network type instead of auto, unless for debug or experimental)
    pub network: Option<NetID>,

    #[structopt(long)]
    /// Wallet API endpoint (daemon address of melwalletd) [default value: 127.0.0.1:11773]
    pub daemon: Option<SocketAddr>,
    #[structopt(long)]
    /// Alias to --daemon
    pub endpoint: Option<SocketAddr>,

    #[structopt(long)]
    /// set the bootstrap node address, otherwise defaults to {network}-bootstrap.themelio.org
    pub bootstrap: Option<SocketAddr>,

    #[structopt(long, default_value = "__melminter_")]
    /// Prefixes for the "owned" wallets created by the melminter.
    pub wallet_prefix: String,

    #[structopt(long)]
    /// Payout address for melminter profits.
    /// the program will send you 0.5 MEL once the mint-wallet balance more than 1.0 MEL;
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
    #[structopt(long)]
    /// Whether to allow send any tx in any case? (defaults to disabled)
    pub allow_any_tx: bool,
    #[structopt(long)]
    /// Disable all failsafe and no warns
    pub no_failsafe: bool,

    #[structopt(long, default_value = "0.025")]
    /// Specify a "max lost" coins for balance safe (unit: MEL, for example 0.0321)
    /// testnet default value = 5 MEL fixed
    pub balance_max_losts: String,

    #[structopt(long)]
    /// Whether enable debug output for all mods
    pub debug: bool,

    #[structopt(long)]
    /// Whether exporting the secret key of mint wallet. (defaults to do nothing)
    /// maybe only useful for without payout option
    pub export_sk: bool,

    #[structopt(long)]
    /// Manual specify a fixed difficulty here, otherwise melminter will automatic to select one.
    ///   (PLEASE NOTE: this value should be chosen carefully!
    ///   if you enter a too small value, your incomes may not be cover the expenses,
    ///   because the ERG you minted may not be enough to cover the network fee for doscMint transactions)
    pub fixed_diff: Option<u8>,
    #[structopt(long)]
    /// if provided, to control the approx time to specified seconds.
    /// [fixed-secs and fixed-diff is cannot give both!]
    /// PLEASE NOTE SEE --fixed-diff
    pub fixed_secs: Option<u32>,

    #[structopt(long)]
    /// [EXPERIMENTAL] Whether melminter should be bulk to sent new-coin seeds tx...
    pub bulk_seeds: bool,

    // #[structopt(long)]
    // /// Drain the fee reserve at the start.
    // pub drain_reserve: bool,
}
