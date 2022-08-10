use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::{repeat_fallible, state::MintState, CmdOpts, panic_exit};
use dashmap::{mapref::multiple::RefMulti, DashMap};
use melwallet_client::WalletClient;
use prodash::{messages::MessageLevel, tree::Item, unit::display::Mode};
use smol::{
    channel::{Receiver, Sender},
    Task,
};
use themelio_nodeprot::ValClient;
use themelio_stf::Tip910MelPowHash;
use themelio_structs::{
    Address, CoinData, CoinDataHeight, CoinID, CoinValue, Denom, NetID, PoolKey, TxKind,
};

/// Worker configuration
#[derive(Clone, Debug)]
pub struct WorkerConfig {
    pub wallet: WalletClient,
    pub payout: Option<Address>,
    pub connect: SocketAddr,
    //pub name: String,
    pub tree: prodash::Tree,
    pub threads: usize,

    pub cli_opts: CmdOpts,
}

/// Represents a worker.
pub struct Worker {
    send_stop: Sender<()>,
    _task: smol::Task<surf::Result<()>>,
}

impl Worker {
    /// Starts a worker with the given WorkerConfig.
    pub fn start(config: WorkerConfig) -> Self {
        let (send_stop, recv_stop) = smol::channel::bounded(1);
        Self {
            send_stop,
            _task: smol::spawn(main_async(config, recv_stop)),
        }
    }

    /// Send a stop request to the worker
    pub async fn stop(&self) -> anyhow::Result<()> {
        self.send_stop.send(()).await?;
        Ok(())
    }
    /// Waits for the worker to complete the current iteration
    pub async fn wait(self) -> surf::Result<()> {
        self._task.await
    }
}

async fn main_async(opts: WorkerConfig, recv_stop: Receiver<()>) -> surf::Result<()> {
    let tree = opts.tree.clone();
    repeat_fallible(|| async {
        let cli_opts = opts.cli_opts.clone();

        let worker = tree.add_child("worker");
        let worker = Arc::new(Mutex::new(worker));
        let is_testnet = opts.wallet.summary().await?.network == NetID::Testnet;
        let client = get_valclient(is_testnet, opts.connect).await?;

        let mut mint_state = MintState::new(opts.wallet.clone(), client.clone());
        let quit_without_profit = ! cli_opts.disable_profit_failsafe;
        let max_losts = if let Some(v) = cli_opts.balance_max_losts { Some(v.parse().unwrap()) } else { None };

        loop {
            // check profit status, and/or quitting without incomes
            mint_state.fee_failsafe(max_losts, quit_without_profit);

            let snapshot = client.snapshot().await?;
            let erg_to_mel = snapshot
                .get_pool(PoolKey::mel_and(Denom::Erg))
                .await?
                .expect("must have erg-mel pool");

            // If we have any erg, convert it all to mel.
            let our_ergs = opts
                .wallet
                .summary()
                .await?
                .detailed_balance
                .get("64")
                .copied()
                .unwrap_or_default();
            if our_ergs > CoinValue(0) {
                worker
                    .lock()
                    .unwrap()
                    .message(MessageLevel::Info, format!("CONVERTING {} ERG!", our_ergs));
                mint_state.convert_doscs(our_ergs).await?;
            }

            // skipping transfer profits if no provide payout address.
            if let Some(payout) = opts.payout {
                // If we have more than 1 MEL, transfer [half balance] to the backup wallet.
                let our_mels = opts
                    .wallet
                    .summary()
                    .await?
                    .detailed_balance
                    .get("6d")
                    .copied()
                    .unwrap_or_default();
                if our_mels > CoinValue::from_millions(1u8) {
                    let to_transfer = our_mels / 2;
                    worker.lock().unwrap().info(format!("balance of working-wallet: {} | profits have more than 1.0 MEL, transferring half to payout address...", our_mels));

                    let to_send = opts
                        .wallet
                        .prepare_transaction(
                            TxKind::Normal,
                            vec![],
                            vec![CoinData {
                                covhash: payout,
                                value: to_transfer,
                                additional_data: vec![],
                                denom: Denom::Mel,
                            }],
                            vec![],
                            vec![],
                            vec![],
                        )
                        .await?;
                    let h = opts.wallet.send_tx(to_send).await?;
                    worker.lock().unwrap().info( format!("sent {} MEL to payout wallet. tx hash: {}", to_transfer, h) );
                    opts.wallet.wait_transaction(h).await?;
                }
            }

            let my_speed = compute_speed().await;
            let my_difficulty = {
                let auto = (my_speed * if is_testnet { 120.0 } else { 30000.0 }).log2().ceil() as usize;
                match cli_opts.fixed_diff {
                    None => { auto },
                    Some(diff) => { diff }
                }
            };
            let approx_iter = 2.0f64.powi(my_difficulty as _) / my_speed;

            worker.lock().unwrap().message(
                MessageLevel::Info,
                format!(
                    "Selected difficulty {}: {} (approx. {:.3}s / tx)",

                    if let Some(_) = cli_opts.fixed_diff { "[fixed]" } else { "[auto]" },
                    my_difficulty,
                    approx_iter,
                ),
            );

            let threads = opts.threads;
            let fastest_speed = client.snapshot().await?.current_header().dosc_speed as f64 / 30.0;
            worker.lock().unwrap().info(format!("Max speed on chain: {:.2} kH/s", fastest_speed / 1000.0));

            let seed_ttl = mint_state.set_seed_expire(Duration::from_secs_f64(approx_iter*2.0));
            worker.lock().unwrap().info(format!("Seed TTL: {} blocks ({}s)", seed_ttl, seed_ttl*30));

            // if requested, stopping before generate seed
            if recv_stop.try_recv().is_ok() {
                log::warn!("melminter process terminating");
                std::process::exit(0);
            }

            // generates some seeds
            {
                let mut sub = worker
                    .lock()
                    .unwrap()
                    .add_child("generating seed UTXOs for minting...");
                sub.init(None, None);
                mint_state.generate_seeds(threads).await?;
            }

            // repeat because wallet could be out of money
            let batch: Vec<(CoinID, CoinDataHeight, Vec<u8>)> = repeat_fallible(|| {
                let mint_state = &mint_state;
                let subworkers = Arc::new(DashMap::new());
                let worker = worker.clone();

                let total = 100 * (1usize << (my_difficulty.saturating_sub(10)));

                // background task that tallies speeds
                let speed_task: Arc<Task<()>> = {
                    let subworkers = subworkers.clone();
                    let worker = worker.clone();
                    let snapshot = snapshot.clone();
                    let wallet = opts.wallet.clone();
                    Arc::new(smol::spawn(async move {
                        let mut previous: HashMap<usize, usize> = HashMap::new();
                        let mut _space = None;
                        let mut delta_sum = 0;
                        let start = Instant::now();

                        let total_sum = (total * threads) as f64;

                        let disconnect_timeout = Duration::from_secs(600); // ten minutes
                        let mut disconnect_started: Option<Instant> = None;

                        loop {
                            smol::Timer::after(Duration::from_secs(1)).await;

                            let mut curr_sum = 0;
                            subworkers.iter().for_each(|pp: RefMulti<usize, Item>| {
                                let prev = previous.entry(*pp.key()).or_insert(0usize);
                                let curr = pp.value().step().unwrap_or_default(); curr_sum += curr;
                                delta_sum += curr.saturating_sub(*prev);
                                *prev = curr;
                            });
                            let curr_sum = curr_sum as f64;

                            let speed = (delta_sum * 1024) as f64 / start.elapsed().as_secs_f64();
                            let per_core_speed = speed / (threads as f64);
                            let dosc_per_day =
                                (per_core_speed / fastest_speed).powi(2) * (threads as f64);
                            let erg_per_day = dosc_per_day
                                * (themelio_stf::dosc_to_erg(
                                    snapshot.current_header().height,
                                    10000,
                                ) as f64)
                                / 10000.0;
                            let (_, mel_per_day) = erg_to_mel
                                .clone()
                                .swap_many((erg_per_day * 10000.0) as u128, 0);
                            let mel_per_day = mel_per_day as f64 / 10000.0;

                            let summary = match wallet.summary().await {
                                Ok(s) => {
                                    if let Some(_) = disconnect_started {
                                        disconnect_started = None;
                                    }

                                    s
                                },
                                Err(e) => {
                                    if let None = disconnect_started {
                                        log::error!("Failed to get wallet summary: {:?}", e);
                                        log::warn!("Cannot connect to the melwalletd daemon! Melminter will try again until connected... and the mint progress still continue, BUT PLEASE NOTE: your mint incomes will be ZERO if the daemon connection cannot recovered. For save your CPU computing resources, the program will exit if disconnected a long time (timeout is {:?})", disconnect_timeout);
                                        disconnect_started = Some(Instant::now());
                                    }

                                    {
                                        // display error info.
                                        let mut new = worker.lock().unwrap().add_child(format!("Failed to connect daemon: {:?}", e));
                                        new.init(None, None);
                                        _space = Some(new);
                                    }

                                    // this check is mainly to prevent un-necessary CPU-time waste.
                                    if disconnect_started.unwrap().elapsed() > disconnect_timeout {
                                        log::error!("the daemon connection recovery failed because timeout-ed! ({:?})", disconnect_timeout);

                                        panic_exit!(90, "because still does not recovery the daemon connection, so exit minting to avoid waste the CPU computing resources.");
                                    }

                                    // to retry...
                                    continue;
                                }
                            };
                            let mel_balance = summary.detailed_balance.get("6d").unwrap();

                            let mut new = worker.lock().unwrap().add_child(format!(
                                "current progress: {} | expected daily return: {:.3} DOSC ≈ {:.3} ERG ≈ {:.3} MEL | fee reserve: {} MEL",
                                if curr_sum <= 0.0 { "N/A".to_string() } else { format!("{:.2} %", 100.0/(total_sum/curr_sum)) },
                                dosc_per_day, erg_per_day, mel_per_day, mel_balance
                            ));
                            new.init(None, None);
                            _space = Some(new);
                        }
                    }))
                };

                async move {
                    let started = Instant::now();

                    let res = mint_state.mint_batch(
                        my_difficulty,
                        move |a, b| {
                            let mut subworker = subworkers.entry(a).or_insert_with(|| {
                                let mut child = worker
                                    .lock()
                                    .unwrap()
                                    .add_child(format!("subworker {}", a));
                                child.init(
                                    Some(total),
                                    Some(prodash::unit::dynamic_and_mode(
                                        "kH",
                                        Mode::with_throughput(),
                                    )),
                                );
                                child
                            });
                            subworker.set(((total as f64) * b) as usize);
                        },
                        threads,
                    ).await?;

                    let ended = started.elapsed().as_secs_f64();
                    println!("Proof Completed {} kH (total {:.3} threads) in time {:.3}s | offset: (approx){:.3}s - (real){:.3}s = {:.3}s",
                        total * threads, threads, ended,

                        // calculating deviation for improve the accuracy of predicted time spent...
                        approx_iter,
                        ended,
                        approx_iter - ended,
                    );

                    std::mem::drop(speed_task);
                    Ok::<_, surf::Error>(res)
                }
            }).await;

            worker.lock().unwrap().message(
                MessageLevel::Info,
                format!("built batch of {} future proofs", batch.len()),
            );

            // Time to submit the proofs. For every proof in the batch, we attempt to submit it. If the submission fails, we move on, because there might be some weird race condition with melwalletd going on.
            // We also attempt to submit transactions in parallel. This is done by retrying always (with max count 10).
            let mut waits = vec![];
            {
                // a retry queue for submit proofs
                let mut submits = VecDeque::new();
                for (coin, data, proof) in batch {
                    submits.push_back((coin, data, proof));
                }
                let txs = submits.len();

                let mut sub = worker.lock().unwrap().add_child("submitting proof");
                sub.init(Some(txs), None);

                let mut retry_lefts = txs * 10; // retry limit
                while submits.len() > 0 {
                    let (coin, data, proof) = submits.pop_front().unwrap();

                    let snap = client.snapshot().await?;
                    let reward_speed = 2u128.pow(my_difficulty as u32) / (snap.current_header().height.0 + 40 - data.height.0) as u128;
                    let reward = themelio_stf::calculate_reward(reward_speed * 100, snap.current_header().dosc_speed, my_difficulty as u32, true);
                    let reward_ergs = themelio_stf::dosc_to_erg(snap.current_header().height, reward);

                    match mint_state.send_mint_transaction(coin, my_difficulty, proof.clone(), reward_ergs.into()).await {
                        Err(err) => {
                            log::error!("FAILED a proof submission for some reason: {:?}", err);

                            if err.to_string().contains("preparation") || err.to_string().contains("timeout") {
                                retry_lefts += 1; // make sure try again
                                let mut sub = sub.add_child("waiting for available coins");
                                sub.init(None, None);
                                smol::Timer::after(Duration::from_secs(10)).await;
                            }

                            if retry_lefts > 0 {
                                retry_lefts -= 1;
                                submits.push_back((coin, data, proof));
                            } else {
                                log::warn!("dropping proof {:?} because reach max retry limit", (coin, data));
                            }
                        },
                        Ok(res) => {
                            waits.push(res);

                            sub.inc();
                            sub.info(format!("(proof submit successfully) minted {} ERG", CoinValue(reward_ergs)));
                        }
                    }

                    smol::Timer::after(Duration::from_secs(1)).await;
                }

                assert!(waits.len() <= txs);
                if waits.len() != txs {
                    log::error!("failed to submit {} proofs", txs - waits.len());
                }
            }

            let mut sub = worker
                .lock()
                .unwrap()
                .add_child("waiting for confirmation of proof");
            sub.init(Some(waits.len()), None);
            for to_wait in waits {
                sub.inc();
                opts.wallet.wait_transaction(to_wait).await?;
            }
        }

        return Ok::<_, surf::Error>(()); // this unreachable code used to infer generic types of function repeat_fallible
    })
    .await;
    Ok(())
}

// Computes difficulty
async fn compute_speed() -> f64 {
    for difficulty in 1.. {
        let start = Instant::now();
        smol::unblock(move || melpow::Proof::generate(&[], difficulty, Tip910MelPowHash)).await;
        let elapsed = start.elapsed();
        let speed = 2.0f64.powi(difficulty as _) / elapsed.as_secs_f64();
        if elapsed.as_secs_f64() > 0.5 {
            return speed;
        }
    }
    unreachable!()
}

async fn get_valclient(testnet: bool, connect: SocketAddr) -> anyhow::Result<ValClient> {
    let client = themelio_nodeprot::ValClient::new(
        if testnet {
            NetID::Testnet
        } else {
            NetID::Mainnet
        },
        connect,
    );
    if testnet {
        client.trust(themelio_bootstrap::checkpoint_height(NetID::Testnet).unwrap());
    } else {
        client.trust(themelio_bootstrap::checkpoint_height(NetID::Mainnet).unwrap());
    }
    Ok(client)
}
