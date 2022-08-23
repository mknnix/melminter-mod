use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};

use crate::{
    repeat_fallible,
    state::MintState,
    db::{TrySendProof, TrySendProofState},
    CmdOpts,
    panic_exit
};
use bincode;

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
    Address, CoinData,
    CoinDataHeight, CoinID,
    CoinValue, Denom,
    NetID,
    PoolKey, TxKind,
};

/// Worker configuration
#[derive(Clone, Debug)]
pub struct WorkerConfig {
    pub wallet: WalletClient,
    pub payout: Option<Address>,
    pub connect: SocketAddr,
    pub netid: NetID,
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

    #[allow(unreachable_code)]
    repeat_fallible(|| async {
        let cli_opts = opts.cli_opts.clone();
        let worker = tree.add_child("worker");
        let worker = Arc::new(Mutex::new(worker));
        let netid = opts.netid;
        let is_testnet = netid != NetID::Mainnet;
        let client = get_valclient(netid, opts.connect).await?;

        let mut mint_state = MintState::new(opts.wallet.clone(), client.clone());
        let quit_without_profit = if is_testnet { false } else { ! cli_opts.disable_profit_failsafe };
        let max_losts: CoinValue = if is_testnet { CoinValue(2000000) } else { cli_opts.balance_max_losts.parse().unwrap() };
        let bulk_seeds = if is_testnet { true } else { cli_opts.bulk_seeds };

        // establish a connection to local disk storage for saves un-sent proofs.
        let db = crate::db::db_open()?;
        let dict_proofs = db.open_dict(crate::db::TABLE_PROOF_LIST)?;

        let dict_proofs_key = b"\x7c\x9c\x69\x8b\x81\x00\x0f\x15..metadata/key_array".to_vec();
        let mut dict_proofs_key_raw: Vec< Vec<u8> > = vec![];

        if let Some(raw) = dict_proofs.get(&dict_proofs_key)? {
            dict_proofs_key_raw = bincode::deserialize(&raw)?;
            dict_proofs_key_raw.dedup();
        } else {
            let v = bincode::serialize(&dict_proofs_key_raw)?;
            dict_proofs.insert(dict_proofs_key.clone(), v)?;
        }

        // A queue for any proofs that waiting to submit (global store / also possible from disk...)
        let mut submit_proofs: VecDeque<(TrySendProof, TrySendProofState)> = VecDeque::new();
        for key in &dict_proofs_key_raw {
            if let Some(val) = dict_proofs.get(&key)? {
                let key = bincode::deserialize(&key)?;
                let val = bincode::deserialize(&val)?;
                submit_proofs.push_back( (key, val) );
            } else {
                log::warn!("missing hit a key {:?} of TABLE_PROOF_LIST: unexpected none value!", key);
            }
        }

        dict_proofs.flush()?;

        loop {
            for (k, _) in &submit_proofs {
                dict_proofs_key_raw.push( bincode::serialize(&k)? );
            }
            dict_proofs_key_raw.dedup();
            dict_proofs.insert( dict_proofs_key.clone(), bincode::serialize(&dict_proofs_key_raw)? )?;
            dict_proofs.flush()?;

            let my_speed = compute_speed().await;
            let my_difficulty = {
                let auto = (my_speed * if is_testnet { 120.0 } else { 30000.0 }).log2().ceil() as usize;
                match cli_opts.fixed_diff {
                    None => { auto },
                    Some(diff) => { diff }
                }
            };
            let approx_iter = 2.0f64.powi(my_difficulty as _) / my_speed;

            // Time to submit the proofs, first we store all proofs in disk. then for every proof in the batch, we attempt to submit it. If the submission fails, we move on, because there might be some weird race condition with melwalletd going on.
            // We also attempt to submit transactions in parallel. This is done by retrying always (with max count 3).
            let mut waits = vec![];
            if submit_proofs.len() > 0 {
                let txs = submit_proofs.len();

                let mut sub = worker.lock().unwrap().add_child("submitting proof");
                sub.init(Some(txs), None);

                //let mut retry_lefts = txs * 10; // retry limit
                let max_retry: u8 = 3;
                while submit_proofs.len() > 0 {
                    let (trys, mut tryst) = submit_proofs.pop_front().unwrap();
                    let (coin, data, proof) = (trys.coin, &trys.data, &trys.proof);

                    let snap = client.snapshot().await?;
                    let reward_speed = 2u128.pow(my_difficulty as u32) / (snap.current_header().height.0 + 40 - data.height.0) as u128;
                    let reward = themelio_stf::calculate_reward(reward_speed * 100, snap.current_header().dosc_speed, my_difficulty as u32, true);
                    let reward_ergs = themelio_stf::dosc_to_erg(snap.current_header().height, reward);

                    match mint_state.send_mint_transaction(coin, my_difficulty, proof.clone(), reward_ergs.into()).await {
                        Err(err) => {
                            /*
                            if err.to_string().contains("preparation") || err.to_string().contains("timeout") {
                                ; // make sure try again
                                let mut sub = sub.add_child("waiting for available coins");
                                sub.init(None, None);
                                smol::Timer::after(Duration::from_secs(10)).await;
                            }
                            */

                            tryst.fails += 1;
                            tryst.errors.push( format!("{:?} | {:?}", SystemTime::now(), err) );
                            log::warn!("FAILED a proof submission for some reason: {:?}", err);

                            if tryst.fails <= max_retry {
                                submit_proofs.push_back( ( trys.clone(), tryst.clone() ) );
                            } else {
                                log::error!("Dropping proof {:?} from submit queue, because reach max retry limit", (coin, data));
                            }
                        },
                        Ok(res) => {
                            tryst.sent = true;
                            waits.push(res);

                            sub.inc();
                            sub.info(format!("(proof sent) minted {} ERG", CoinValue(reward_ergs)));
                        }
                    }

                    {
                        let trys_key = bincode::serialize(&trys)?;
                        let trys_val = bincode::serialize(&tryst)?;
                        dict_proofs.insert(trys_key, trys_val)?;
                    }

                    smol::Timer::after(Duration::from_secs(1)).await;
                }

                assert!(waits.len() <= txs);
                if waits.len() != txs {
                    log::error!("failed to submit {} proofs", txs - waits.len());
                }
            }
            if waits.len() > 0 {
                let mut sub = worker
                    .lock()
                    .unwrap()
                    .add_child("waiting for confirmation of proof");
                sub.init(Some(waits.len()), None);
                for to_wait in waits {
                    opts.wallet.wait_transaction(to_wait).await?;
                    sub.inc();
                }
            }

            let snapshot = client.snapshot().await?;
            let erg_to_mel = snapshot
                .get_pool(PoolKey::mel_and(Denom::Erg))
                .await?
                .expect("must have erg-mel pool");

            let summary = opts.wallet.summary().await?;

            // If we have any erg, convert it all to mel.
            let our_ergs = summary.detailed_balance.get("64").copied().unwrap_or_default();
            if our_ergs > CoinValue(0) {
                worker
                    .lock()
                    .unwrap()
                    .message(MessageLevel::Info, format!("CONVERTING {} ERG!", our_ergs));
                mint_state.convert_doscs(our_ergs).await?;
            }

            // check profit status, and/or quitting without incomes
            mint_state.fee_failsafe(max_losts, quit_without_profit);

            // skipping transfer profits if without payout address.
            if let Some(payout) = opts.payout {
                //let our_mels = summary.detailed_balance.get("6d").copied().unwrap_or_default();
                let our_mels: CoinValue = summary.total_micromel;

                // If we have more than 1 MEL, transfer [half balance] to the backup wallet.
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

            worker.lock().unwrap().message(
                MessageLevel::Info,
                format!(
                    "Selected difficulty {}: {} (approx. {:.3}s / tx)",

                    if cli_opts.fixed_diff.is_none() { "[auto]" } else { "[fixed]" },
                    my_difficulty,
                    approx_iter,
                ),
            );

            let threads = opts.threads;
            let fastest_speed = client.snapshot().await?.current_header().dosc_speed as f64 / 30.0;
            worker.lock().unwrap().info(format!("Max speed on chain: {:.2} kH/s", fastest_speed / 1000.0));

            let seed_ttl = mint_state.set_seed_expire(Duration::from_secs_f64(approx_iter*2.0));
            worker.lock().unwrap().info(format!("Seed TTL: {} blocks ({}s)", seed_ttl, seed_ttl*30));
            worker.lock().unwrap().info(format!("Minter Address: {}", summary.address));

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
                mint_state.generate_seeds(threads, bulk_seeds).await?;
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
                        let mut _space;
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
                            let dosc_per_day = (per_core_speed / fastest_speed).powi(2) * (threads as f64);
                            let erg_per_day = dosc_per_day * (themelio_stf::dosc_to_erg(snapshot.current_header().height, 10000) as f64) / 10000.0;
                            let (_, mel_per_day) = erg_to_mel.clone().swap_many((erg_per_day * 10000.0) as u128, 0);
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
                                        log::warn!("Cannot connect to the melwalletd daemon! Melminter will try again until connected...");
                                        log::info!("the mint progress will still continue, BUT PLEASE NOTE: your mint incomes will be ZERO if the daemon connection cannot recovered.");
                                        log::info!("For save your CPU computing resources, the program will exit if disconnected a long time (timeout is {:?})", disconnect_timeout);
                                        disconnect_started = Some(Instant::now());
                                    }

                                    {
                                        // display error info.
                                        let mut new = worker.lock().unwrap().add_child(format!("(Failed to connect daemon: {:?})", e));
                                        new.init(None, None);
                                        _space = new;
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

                            let mut new = worker.lock().unwrap().add_child(
                                format!( "current progress: {:.2} % | fee reserve: {} MEL | expected daily return: {:.3} DOSC ≈ {:.3} ERG ≈ {:.3} MEL",
                                         (curr_sum/total_sum) * 100.0, mel_balance,
                                         dosc_per_day, erg_per_day, mel_per_day
                                )
                            );
                            new.init(None, None);
                            _space = new;
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
                    let kh = total * threads;
                    println!("Proof Completed {} kH (total {:.3} threads) in time {:.3}s | Average Speed: {:.3}kH/s | Offset: (approx){:.3}s - (real){:.3}s = {:.3}s",
                        kh, threads, ended,
                        (kh as f64) / ended,

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

            for (coin, data, proof) in batch {
                let trys = TrySendProof { coin, data, proof };
                let tryst = TrySendProofState {
                    fails: 0u8,
                    created: SystemTime::now(),
                    sent: false,
                    failed: false,
                    errors: vec![],
                };

                {
                    let trys_key: Vec<u8> = bincode::serialize(&trys)?;
                    let trys_val: Vec<u8> = bincode::serialize(&tryst)?;
                    dict_proofs.insert(trys_key, trys_val)?;
                }
                submit_proofs.push_back((trys, tryst));
            }
            dict_proofs.flush()?;
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
        if elapsed.as_secs_f64() > 1.0 {
            return speed;
        }
    }
    unreachable!()
}

async fn get_valclient(net: NetID, connect: SocketAddr) -> anyhow::Result<ValClient> {
    let client = themelio_nodeprot::ValClient::new(net, connect);

    if net == NetID::Testnet {
        client.trust(themelio_bootstrap::checkpoint_height(NetID::Testnet).unwrap());
    } else {
        client.trust(themelio_bootstrap::checkpoint_height(NetID::Mainnet).unwrap());
    }
    Ok(client)
}
