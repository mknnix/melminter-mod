use std::{ future::Future, time::Duration, net::SocketAddr };
use std::io::Write;

use anyhow::Context;
use cmdopts::CmdOpts;

use melwallet_client::DaemonClient;
use prodash::{
    render::line::{self, StreamKind},
    Tree,
};
use structopt::StructOpt;
use themelio_structs::{CoinValue, NetID};

mod cmdopts;
mod state;
mod worker;
mod db;

// use smol::prelude::*;
use crate::worker::{Worker, WorkerConfig};

fn main() -> surf::Result<()> {
    let dash_root = Tree::default();
    let dash_options = line::Options {
        keep_running_if_progress_is_empty: true,
        throughput: true,
        // hide_cursor: true,
        ..Default::default()
    }
    .auto_configure(StreamKind::Stdout);
    let _handle = line::render(std::io::stdout(), dash_root.clone(), dash_options);

    let opts: CmdOpts = CmdOpts::from_args();
    {
        let mut lb = env_logger::Builder::new();
        if opts.debug {
            // (debug mode) apply DEBUG log-level to all modules
            lb.filter(None, log::LevelFilter::Debug);
        } else {
            // defaults to INFO log-level and only apply to this program itself.
            lb.filter(Some( env!("CARGO_PKG_NAME").replace("-", "_").as_str() ), log::LevelFilter::Info);
        }
        lb.init();
    }

    if opts.daemon.is_some() && opts.endpoint.is_some() {
        panic_exit!(2, "unexpected found both option --daemon and --endpoint given");
    }

    let melwalletd_addr: SocketAddr =
        if let Some(addr) = opts.daemon {
            addr
        } else if let Some(addr) = opts.endpoint {
            addr
        } else {
            "127.0.0.1:11773".parse().unwrap()
        };

    smol::block_on(async move {
        // print version and daemon address
        print!("{} v{} ({}) / connect to melwalletd endpoint {} (",
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"), env!("GIT_COMMIT_HASH"),
            melwalletd_addr
        ); std::io::stdout().flush()?;

        // use the provided address of melwalletd daemon, and auto detect network type.
        let daemon = DaemonClient::new(melwalletd_addr);

        // For latest version of melwalletd, the HTTP API "/summary?testnet=1" does not works anymore
        //                   (melwalletd no longer connect both mainnet & testnet, must use option "--network" select one or defaults to "mainnet")
        // melwalletd no longer returns a different result based on "/summary?testnet=1" (it always depends on the value specified by "--network")
        //                   So just need to get the returned result to determine which network type.
        let auto_netid: NetID = daemon.get_summary(false).await?.network;

        let netid: NetID = if let Some(id) = opts.network {
            if id != auto_netid {
                log::error!("Warning: the network type you specified is Different to the network type of melwalletd reported! This program will wait 5 seconds to continue...");
                smol::Timer::after(Duration::from_secs(5)).await;
            }
            id
        } else {
            auto_netid
        };

        // println network id
        println!("{})", netid);
        println!("");

        // Is CustomXX also a kind of testnet ??
        let is_testnet = netid != NetID::Mainnet;

        // generate wallet name for minting
        let wallet_name = format!("{}{:?}", opts.wallet_prefix, netid);
        // make sure the working-wallet exists
        let worker_wallet = match daemon.get_wallet(&wallet_name).await? {
            Some(wallet) => wallet,
            None => {
                let mut evt = dash_root.add_child(format!("creating new wallet {}", wallet_name));
                evt.init(None, None);
                log::info!("creating new wallet");
                daemon.create_wallet(&wallet_name, is_testnet, None, None).await?;
                daemon.get_wallet(&wallet_name).await?.context("just-created wallet gone?!")?
            }
        };

        if let None = opts.payout {
            let wallet_sk = if opts.export_sk {
                if let Ok(sk) = worker_wallet.export_sk(None).await {
                    sk
                } else {
                    worker_wallet.export_sk(Some("".to_string())).await?
                }
            } else {
                "(use '--export-sk' if you want)".to_string()
            };

            log::warn!("You does not specify a payout address for receive your minted coins! but no problem because the balance safety stored in the mint wallet ({}).", &wallet_name);
            log::warn!("You can import this mint wallet by using secret-key: {}", wallet_sk);
            log::warn!("Please provide a payout if you want to get your incomes in your wallet, or importing this working-wallet if you want to manual manage it.");
            std::mem::drop(wallet_sk);
        }

        // make sure the working-wallet has enough money
        while worker_wallet
            .summary()
            .await?
            .detailed_balance
            .get("6d")
            .copied()
            .unwrap_or(CoinValue(0))
            < CoinValue::from_millions(1u64) / 20
        {
            let _evt = dash_root.add_child("balance of melminter working wallet is less than 0.05 MEL! melminter requires a small amount of 'seed' MEL to start minting...");
            let _evt = dash_root.add_child(format!(
                "Please send at least 0.1 MEL to {}",
                worker_wallet.summary().await?.address
            ));
            smol::Timer::after(Duration::from_secs(1)).await;

            if opts.skip_balance_check { break; }
        }

        let worker = Worker::start(WorkerConfig {
            wallet: worker_wallet,
            payout: opts.payout,
            connect: if let Some(bootstrap) = opts.bootstrap { bootstrap } else { themelio_bootstrap::bootstrap_routes(netid)[0] },
            netid,
            //name: "".into(),
            tree: dash_root.clone(),
            threads: opts.threads.unwrap_or_else(num_cpus::get_physical),

            cli_opts: opts.clone(),
        });

        // allow users to request program "safety exit" (avoid quitting after new-coin transaction, it cause more low-profit proofs)
        let mut worker_stopping = false;
        ctrlc::set_handler(move || {
            if worker_stopping {
                panic_exit!(1, "Press Ctrl+C key again? now process exiting...");
            }

            worker_stopping = true;
            log::warn!("Received Ctrl+C key, the program will stopping mint as soon as possible... scheduled to stop after the DoscMint transactions sent, or you can exit immediately (by press again) if you wish.");
            smol::block_on(worker.stop()).unwrap();
        }).unwrap();

        smol::future::pending().await
    })
}

#[macro_export]
macro_rules! panic_exit {
    ($status:tt, $($arg:tt)*) => {
        let orig_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            orig_hook(panic_info);
            std::process::exit($status);
        }));
        panic!($($arg)*)
    };
}

/// Repeats something until it stops failing
async fn repeat_fallible<T, E: std::fmt::Debug, F: Future<Output = Result<T, E>>>(
    mut clos: impl FnMut() -> F,
) -> T {
    loop {
        match clos().await {
            Ok(val) => return val,
            Err(err) => log::warn!("retrying failed: {:?}", err),
        }
        smol::Timer::after(Duration::from_secs(1)).await;
    }
}

/// Generate a new owner-less address. any coins that are sent to such addresses are considered lost forever, "the dead end of blockchain"
pub fn new_void_address() -> themelio_structs::Address {
    themelio_stf::melvm::Covenant::std_ed25519_pk_new(
        tmelcrypt::ed25519_keygen().1
            .to_public()
    ).hash()
}

/// the "/dev/null" of blockchain...
pub fn new_null_dst() -> themelio_structs::Address {
    let mut a;
    loop {
        a = new_void_address();
        if format!("{}", a).starts_with("t0000") {
            return a;
        }
    }
}

#[test]
fn nva_test() {
    for i in 0..100000 {
    println!("VA-{}: {}", i+1, new_void_address());
    }
}

#[test]
fn nnd_test() {
    for i in 0..1 {
        println!("null[{}] {}", i+1, new_null_dst());
    }
}
