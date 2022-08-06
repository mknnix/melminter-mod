use std::{
    future::Future,
    process::{Command, Stdio},
    time::Duration,
};

use anyhow::Context;
use cmdopts::CmdOpts;

use melwallet_client::DaemonClient;
use prodash::{
    render::line::{self, StreamKind},
    Tree,
};
use structopt::StructOpt;
use tap::Tap;
use themelio_structs::{CoinValue, NetID};

mod cmdopts;
mod state;
mod worker;
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

    smol::block_on(async move {
        // either start a daemon, or use the provided one
        let mut _running_daemon = None;
        let daemon_addr = if let Some(addr) = opts.daemon {
            addr
        } else {
            // start a daemon naw
            let port = fastrand::usize(50000..65000); // avoid use 11773/11814 or any usual port
            let daemon = Command::new("melwalletd")
                .arg("--listen")
                .arg(format!("127.0.0.1:{}", port))
                .arg("--wallet-dir")
                .arg(dirs::config_dir().unwrap().tap_mut(|p| p.push("melminter")))
                .stderr(Stdio::null())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .spawn()
                .unwrap();
            smol::Timer::after(Duration::from_secs(1)).await;
            _running_daemon = Some(daemon);
            format!("127.0.0.1:{}", port).parse().unwrap()
        };
        scopeguard::defer!({
            if let Some(mut d) = _running_daemon {
                let _ = d.kill();
            }
        });
        let daemon = DaemonClient::new(daemon_addr);
        let network_id = if opts.testnet {
            NetID::Testnet
        } else {
            NetID::Mainnet
        };

        println!("{} v{} / connect to melwalletd endpoint {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"), daemon_addr);
        println!("");

        // generate wallet name for minting
        let wallet_name = format!("{}{:?}", opts.wallet_prefix, network_id);
        // make sure the working-wallet exists
        let worker_wallet = match daemon.get_wallet(&wallet_name).await? {
            Some(wallet) => wallet,
            None => {
                let mut evt = dash_root.add_child(format!("creating new wallet {}", wallet_name));
                evt.init(None, None);
                log::info!("creating new wallet");
                daemon.create_wallet(&wallet_name, opts.testnet, None, None).await?;
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
            connect: themelio_bootstrap::bootstrap_routes(network_id)[0],
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
/*
fn panic_exit(status: i32, text: &str) {
    let orig_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        orig_hook(panic_info);
        std::process::exit(status);
    }));
    panic!("{}", text)
}
*/

// Repeats something until it stops failing
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
