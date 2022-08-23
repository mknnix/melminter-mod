use std::sync::Arc;
use std::time::{SystemTime, Duration};
use std::collections::HashMap;

use anyhow::Context;
use melwallet_client::WalletClient;
use serde::{Deserialize, Serialize};
use stdcode::StdcodeSerializeExt;
use themelio_nodeprot::ValClient;
use themelio_stf::Tip910MelPowHash;
use themelio_structs::{
    CoinData, CoinDataHeight, CoinID,
    CoinValue, Denom,
    NetID,
    PoolKey, TxHash, TxKind,
    Address,
};

use crate::{repeat_fallible, panic_exit, new_void_address, new_null_dst};

#[derive(Clone)]
pub struct MintState {
    wallet: WalletState, // wrapped wallet add more function (dual-unlock, mel-only-or-0, )
    client: ValClient, // connect for a blockchain node
    testnet: bool, // Whether use testnet rules
    fee_handler: FeeSchedule,
    seed_handler: SeedSchedule,
}

#[derive(Clone, Debug)]
pub struct SeedSchedule {
    /// expire blocks of each seeds, all expired coins will be ignored
    pub ttl: Option<u64>,
    /// here store all expired seeds
    pub expired: HashMap<TxHash, Vec<(CoinID, CoinData)>>,
    /// what address to receive expired seeds
    pub covnull: Option<Address>,
    /// wallet for seeds only
    pub wallet: WalletState,
}
impl SeedSchedule {
    /// caller provides Duration; method returns **current** TTL value.
    pub fn set_seed_expire(&mut self, lifetime: Duration) -> u64 {
        if let Some(blocks) = self.ttl {
            return blocks;
        }

        let min = 3600 * 3;
        let max = 3600 * 12;

        let mut secs = lifetime.as_secs();
        if secs < min { secs = min; }
        if secs > max { secs = max; }

        /// ttl-blocks = expire-time / block-interval (all time units seconds)
        let blocks = secs / 30;

        self.ttl = Some(blocks);
        return blocks;
    }

    /// Generates a list of "seed" coins.
    pub async fn generate_seeds(&mut self, threads: usize, seed_bulk: bool) -> surf::Result<()> {
        self.unlock().await?;

        let my_address = self.wallet.0.summary().await?.address;
        loop {
            let toret = self.get_seeds_raw().await?;
            if toret.len() >= threads {
                return Ok(());
            }

            // generate a bunch of custom-token utxos
            let mut outputs: Vec<CoinData> = if seed_bulk {
                vec![
                    CoinData {
                        covhash: my_address,
                        denom: Denom::NewCoin,
                        value: CoinValue( threads as u128 ),
                        additional_data: vec![],
                    }
                ]
            } else {
                std::iter::repeat_with(|| CoinData {
                    covhash: my_address,
                    denom: Denom::NewCoin,
                    value: CoinValue(1),
                    additional_data: vec![],
                }).take(threads).collect()
            };

            let mut exp_add: usize = 0;
            if self.seed_expired.len() > (threads*15) { // sweep all expired seeds (always bulk)
                let exp_dst = if let Some(d) = self.covnull { d } else { new_void_address() };
                for (exp_th, exp_vals) in &self.seed_expired {
                    log::debug!("sweep all expired new-coin(s): tx-hash={:?}, values={:?}", exp_th, exp_vals);
                    assert!( exp_vals.len() > 0 );
                    let (_exp_id, exp_data) = exp_vals[0].clone();

                    let denom = exp_data.denom;
                    for it in exp_vals {
                        assert!( it.1.denom == denom );
                    }

                    outputs.push(CoinData {
                        covhash: if (fastrand::u128(..) % 2) == 0 { exp_dst } else { "t1m9v0fhkbr7q1sfg59prke1sbpt0gm2qgrb166mp8n8m59962gdm0".parse()? },
                        denom,
                        value: CoinValue( exp_vals.len() as u128 ),
                        additional_data: vec![],
                    });
                    exp_add += 1;
                }
            }

            // prepare tx...
            let tx = self.wallet.0.prepare_transaction(
                TxKind::Normal,
                vec![],
                outputs,
                vec![],
                vec![],
                vec![],
            ).await?;

            let fees = tx.fee;
            let sent_hash = self.wallet.0.send_tx(tx).await?;

            if exp_add > 0 {
                self.seed_expired.clear();
            }

            log::debug!("(fee-safe) sent newcoin tx with fee: {}", fees);
            self.fee_history.push(FeeRecord{
                kind: TxKind::Normal,
                time: SystemTime::now(),
                balance: self.get_balance().await?,
                fee: fees,
                income: CoinValue(0),
            });

            self.wallet.0.wait_transaction(sent_hash).await?;
        }
    }

    // caller needs provide current block number
    async fn get_seeds_raw(&mut self, height: u64) -> surf::Result<Vec<CoinID>> {
        let unspent_coins = self.wallet.0.get_coins().await?;
        let current_height = height;//self.client.snapshot().await?.current_header().height.0;

        let mut seeds = vec![];
        for (id, data) in unspent_coins {
            if let Denom::Custom(_) = data.denom {
                // if provides a TTL (unit: how many blocks), an expiration check will happen, it will ignore expired coins.
                if let Some(ttl) = self.seed_ttl {
                    let coin_height = match self.wallet.0.wait_transaction(id.txhash).await {
                        Ok(v) => v,
                        Err(e) => {
                            log::info!("cannot get seed height: {:?}", e);
                            continue;
                        }
                    };
                    assert!(coin_height <= current_height);
                    if (current_height - coin_height) > ttl {
                        log::debug!("ignore too old seed: ttl={}, coin={:?}", ttl, (&id,&data));

                        let th = id.txhash;
                        let v: &mut Vec<(CoinID, CoinData)> =
                            // get it directly if it exists. otherwise create a new Vec and return it
                            if let Some(v) = self.seed_expired.get_mut(&th) {
                                v
                            } else {
                                self.seed_expired.insert(th, vec![]);
                                self.seed_expired.get_mut(&th).unwrap()
                            };

                        v.push((id, data));
                        continue;
                    }
                }
                seeds.push(id);
            }
        }

        log::debug!("got seeds list: {:?}", &seeds);
        Ok(seeds)
    }


}

#[derive(Copy, Clone, Debug)]
pub struct FeeRecord {
    kind: TxKind, // TxKind::Normal for newcoin; TxKind::DoscMint for doscMint; TxKind::Swap for swap.
    time: SystemTime,
    balance: CoinValue,
    fee: CoinValue,
    income: CoinValue, // ERG(should be convert and store MEL) for doscMint, or MEL for swap. any newcoin tx should be always 0.
}
#[derive(Clone, Debug)]
pub struct FeeSchedule {
    // store the fee paid history of MEL balance, for fail-safe (for example automatic exit if mint no profit)
    pub history: Vec<FeeRecord>,
    // Is should be all fee level tx allowed anyway?
    pub allow_any_tx: bool,
    // Disabled any fee-safe?
    pub no_failsafe: bool,

    pub max_lost: CoinValue,
    pub quit: bool,
}
impl FeeSchedule {
    pub fn fee_failsafe(&self) {
        let fh = self.history.clone();
        log::debug!("(fee-safe) our balance history: {:?}", fh);

        let fh_len = fh.len();
        if fh_len < 2 {
            return;
        }

        let mut lost_coins = CoinValue(0);
        for i in 0 .. fh_len {
            let it = fh[i];
            assert!( it.time < SystemTime::now() );
            assert!( it.kind == TxKind::Normal || it.kind == TxKind::DoscMint || it.kind == TxKind::Swap );

            if i > 0 {
                let prev = fh[i-1];
                assert!( prev.time < it.time );
            }

            // skip any newcoin tx(s)
            if it.kind == TxKind::Normal { continue; }

            // calculate the profits
            let profits: i128 = (it.income.0 as i128) - (it.fee.0 as i128);
            // ignore this scenario of no-profit and no-lost
            if profits == 0 { continue; }

            // negative-profits!!
            if profits < 0 {
                lost_coins += CoinValue((-profits) as u128);

            // good, we have a positive profit margin.
            } else if lost_coins > CoinValue(0) {
                let p = CoinValue(profits as u128);
                if lost_coins <= p {
                    lost_coins = CoinValue(0);
                } else {
                    lost_coins -= p;
                }
            }
        }

        // if there is any balance lost, then warnings will be display anyway.
        if lost_coins > CoinValue(0) {
            let first = fh[0];
            let last = fh[fh_len - 1];
            log::warn!("WARNING: our MEL coins losts in {:?}! the mint profit might be a negative! first coins: {} -> last coins: {} (lost coins: - {})", last.time.duration_since(first.time), first.balance, last.balance, first.balance - last.balance);
        }

        // if the loss exceeds the tolerable limit:
        if lost_coins >= self.max_lost {
            // then, terminate this process if allowed to stop minting.
            if self.quit {
                panic_exit!(91, "melminter balance fail-safe started! total-lost-coins {} >= {}(max) ! quit minting to keep your coins!", lost_coins, max_lost);
            }
        }
    }


}

#[derive(Clone, Debug)]
pub struct WalletState(WalletClient);

impl WalletState {
    /// simple/fast get mel balance only
    pub async fn get_balance(&self) -> surf::Result<CoinValue> {
        Ok( self.0.summary().await?.detailed_balance.get("6d").copied().unwrap_or(CoinValue(0)) )
    }

    /// unlock mint-wallet, first try plaintext, second try empty password if fails, final return error if still failed.
    pub async fn unlock(&self) -> surf::Result<()> {
        if self.0.summary().await?.locked {
            if let Err(_) = self.0.unlock(None).await {
                self.0.unlock(Some("".to_string())).await?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct PrepareReq {
    signing_key: String,
    outputs: Vec<CoinData>,
}

impl MintState {
    pub fn new(wallet: WalletClient, client: ValClient) -> Self {
        Self {
            wallet: WalletState (wallet),
            client,
            fee_handler: FeeSchedule {
                history: vec![],
                no_failsafe: false,
                allow_any_tx: false,
            },
            seed_handler: SeedSchedule {
                ttl: None,
                expired: HashMap::new(),
                covnull: Some(new_null_dst()),
            }
        }
    }

    /// Creates a partially-filled-in transaction, with the given difficulty, that's neither signed nor feed. The caller should fill in the DOSC output.
    pub async fn mint_batch(
        &self,
        difficulty: usize,
        on_progress: impl Fn(usize, f64) + Sync + Send + 'static,
        threads: usize,
    ) -> surf::Result<Vec<(CoinID, CoinDataHeight, Vec<u8>)>> {
        //#[cfg(not(target_os="android"))]
        //use thread_priority::{ set_current_thread_priority, ThreadPriority };
        use thread_priority::*;

        // we do not need to save the expired seeds, so just clone
        let height = self.client.snapshot().await?.current_header().height.0;
        let seeds = self.seed_handler.clone().get_seeds_raw(height).await?;

        let on_progress = Arc::new(on_progress);
        let mut proof_thrs = Vec::new();
        for (idx, seed) in seeds.iter().copied().take(threads).enumerate() {
            let tip_cdh = repeat_fallible(|| async { self.client.snapshot().await?.get_coin(seed).await })
                .await.context("transaction's input spent from behind our back")?;

            log::debug!("tip_cdh = {:#?}", tip_cdh);

            let snapshot = self.client.snapshot().await?;
            log::debug!("snapshot header = {:?}", snapshot.current_header());

            let tip_header_hash = repeat_fallible(|| snapshot.get_older(tip_cdh.height))
                .await
                .current_header()
                .hash();
            let chi = tmelcrypt::hash_keyed(&tip_header_hash, &seed.stdcode());
            let on_progress = on_progress.clone();

            //#[cfg(target_os="android")]
            let proof_fut = std::thread::spawn(move || {
                if ThreadPriority::Min.set_for_current().is_err() {
                    if let Ok(n) = ThreadPriority::Min.min_value_for_policy(ThreadSchedulePolicy::Normal(NormalThreadSchedulePolicy::Batch)) {
                        let sets = ThreadPriority::from_posix(ScheduleParams { sched_priority: n }).set_for_current();
                        log::info!("thread n set nice result {:?}", sets);
                    } else {
                        log::info!("cannot get min nice for current system");
                    }
                } else {
                    log::info!("ok change priority to low for mint");
                }

                //log::info!("for android the auto lower nice is disabled due to un-resolved issue, you need to manual set it (uses 'nice' command)");
                (
                    tip_cdh,
                    melpow::Proof::generate_with_progress(
                        &chi,
                        difficulty,
                        |progress| {
                            if fastrand::f64() < 0.1 {
                                on_progress(idx, progress)
                            }
                        },
                        Tip910MelPowHash,
                    ),
                )
            });

            /*
            //#[cfg(not(target_os="android"))]
            let proof_fut: std::thread::JoinHandle<_> = thread_priority::ThreadBuilder::default()
                .name( format!("Minting-{}", idx) )
                .priority(ThreadPriority::Min)
                .spawn(move |nice_result| {
                    // logging the result of thread nice set...
                    log::info!("Minting thread ({}) set Minimum priority: result {:?}", idx, nice_result);

                    (
                        tip_cdh,
                        melpow::Proof::generate_with_progress(
                            &chi,
                            difficulty,
                            |progress| {
                                if fastrand::f64() < 0.1 {
                                    on_progress(idx, progress)
                                }
                            },
                            Tip910MelPowHash,
                        ),
                    )
                })?;
                */

            proof_thrs.push(proof_fut);
        }

        let mut out = vec![];
        for (seed, proof) in seeds.into_iter().zip(proof_thrs.into_iter()) {
            let result = smol::unblock(move || proof.join().unwrap()).await;
            out.push((seed, result.0, result.1.to_bytes()))
        }
        Ok(out)
    }

    /// Sends a transaction.
    pub async fn send_mint_transaction(
        &mut self,
        seed: CoinID,
        difficulty: usize,
        proof: Vec<u8>,
        ergs: CoinValue,
    ) -> surf::Result<TxHash> {
        self.unlock().await?;

        let summary = self.wallet.0.summary().await?;
        let own_cov = summary.address;
        let is_testnet = summary.network != NetID::Mainnet;
        let tx = self
            .wallet
            .prepare_transaction(
                TxKind::DoscMint,
                vec![seed],
                vec![CoinData {
                    denom: Denom::Erg,
                    value: ergs,
                    additional_data: vec![],
                    covhash: own_cov,
                }],
                vec![],
                (difficulty, proof).stdcode(),
                vec![Denom::Erg],
            )
            .await?;

        let fees = tx.fee;
        let mels = self.erg_to_mel(ergs).await?;
        if fees >= mels {
            log::warn!("WARNING: This doscMint fee({} MEL) great-than-or-equal to approx-income({} MEL) amount!! you should check your difficulty or a network issue.", fees, mels);
            if fees > mels && (!is_testnet) && self.allow_any_fee {
                return Err(surf::Error::new(403, anyhow::Error::msg("refused to send any high-fee tx.")));
            }
        }

        let txhash = self.wallet.0.send_tx(tx).await?;
        log::debug!("(fee-safe) sent DoscMint tx with fee: {}", fees);

        self.fee_history.push(FeeRecord{
            kind: TxKind::DoscMint,
            time: SystemTime::now(),
            balance: self.get_balance().await?,
            fee: fees,
            income: mels,
        });
        Ok(txhash)
    }

    // /// Sends a transaction out. What this actually does is to re-prepare another transaction with the same inputs, outputs, and data, so that the wallet can sign it properly.
    // pub async fn send_resigned_transaction(&self, transaction: Transaction) -> surf::Result<()> {
    //     let resigned = self
    //         .wallet
    //         .prepare_transaction(
    //             TxKind::DoscMint,
    //             transaction.inputs.clone(),
    //             transaction.outputs.clone(),
    //             vec![],
    //             transaction.data.clone(),
    //             vec![Denom::Erg],
    //         )
    //         .await?;
    //     let txhash = self.wallet.send_tx(resigned).await?;
    //     self.wallet.wait_transaction(txhash).await?;
    //     Ok(())
    // }

    /// Converts a given number of doscs to mel.
    pub async fn convert_doscs(&mut self, doscs: CoinValue) -> surf::Result<()> {
        self.unlock().await?;

        let summary = self.wallet.0.summary().await?;
        let my_address = summary.address;
        let is_testnet = summary.network != NetID::Mainnet;

        let tx = self
            .wallet
            .prepare_transaction(
                TxKind::Swap,
                vec![],
                vec![CoinData {
                    covhash: my_address,
                    value: doscs,
                    denom: Denom::Erg,
                    additional_data: vec![],
                }],
                vec![],
                PoolKey::new(Denom::Mel, Denom::Erg).to_bytes(),
                vec![],
            )
            .await?;

        let fees = tx.fee;
        let mels = self.erg_to_mel(doscs).await?;
        if fees >= mels {
            log::warn!("WARNING: This ERG-to-MEL swap fee({} MEL) great-than-or-equal to income({} MEL) amount! you should check your difficulty or a network issue.", fees, mels);
            if fees > mels && (!is_testnet) {
                return Err(surf::Error::new(403, anyhow::Error::msg("refused to send any high-fee tx.")));
            }
        }

        let txhash = self.wallet.0.send_tx(tx).await?;

        log::debug!("(fee-safe) sent ERG-to-MEL swap tx with fee: {}", fees);
        self.fee_history.push(FeeRecord{
            kind: TxKind::Swap,
            time: SystemTime::now(),
            balance: self.get_balance().await?,
            fee: fees,
            income: mels,
        });

        self.wallet.0.wait_transaction(txhash).await?;
        Ok(())
    }

    #[allow(dead_code)]
    /// Converts ERG to MEL
    pub async fn erg_to_mel(&self, ergs: CoinValue) -> surf::Result<CoinValue> {
        let mut pool = self
            .client
            .snapshot()
            .await?
            .get_pool(PoolKey::mel_and(Denom::Erg))
            .await?
            .expect("no erg/mel pool");
        Ok(pool.swap_many(ergs.0, 0).1.into())
    }
}
