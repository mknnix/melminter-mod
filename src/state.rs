use std::sync::Arc;
use std::time::{SystemTime, Duration};

use anyhow::Context;
use melwallet_client::WalletClient;
use serde::{Deserialize, Serialize};
use stdcode::StdcodeSerializeExt;
use themelio_nodeprot::ValClient;
use themelio_stf::Tip910MelPowHash;
use themelio_structs::{
    CoinData, CoinDataHeight, CoinID, CoinValue, Denom, PoolKey, TxHash, TxKind,
};

use crate::repeat_fallible;

#[derive(Clone)]
pub struct MintState {
    wallet: WalletClient,
    client: ValClient,

    // store the fee paid history of MEL balance, for fail-safe (for example automatic exit if mint no profit)
    fee_history: Vec<FeeRecord>,
}

#[derive(Copy, Clone, Debug)]
struct FeeRecord {
    f: u8, // F1/F2/F3... first for newcoin; second for doscMint; three for swap.
    time: SystemTime,
    balance: CoinValue,
    fee: CoinValue,
    income: CoinValue, // ERG(should be convert and store MEL) for doscMint, or MEL for swap. any newcoin tx should be always 0.
}

#[derive(Serialize, Deserialize)]
struct PrepareReq {
    signing_key: String,
    outputs: Vec<CoinData>,
}

impl MintState {
    pub fn new(wallet: WalletClient, client: ValClient) -> Self {
        Self { wallet, client, fee_history: vec![] }
    }

    pub async fn get_balance(&self) -> surf::Result<CoinValue> {
        Ok( self.wallet.summary().await?.detailed_balance.get("6d").copied().unwrap_or(CoinValue(0)) )
    }

    #[allow(non_snake_case)]
    pub fn fee_failsafe(&self, max: Option<CoinValue>, quit: bool) {
        // 0.02 MEL as the default max costs for minting...
        let MAX_LOST = CoinValue::from_millions(1u8) / 50;
        // override if given custom value.
        let max_lost = if let Some(ml) = max { ml } else { MAX_LOST };

        let fh = self.fee_history.clone();
        log::debug!("(fee-safe) our balance history: {:?}", fh);

        let fh_len = fh.len();
        if fh_len < 2 {
            return;
        }

        let mut lost_coins = CoinValue(0);
        for it in &fh {
            assert!(it.time < SystemTime::now());
            assert!(it.f >= 1 && it.f <= 3);

            // skip any newcoin tx(s)
            if it.f == 1 { continue; }

            let profits: i128 = (it.income.0 as i128) - (it.fee.0 as i128);
            if profits == 0 { continue; }

            if profits < 0 {
                lost_coins += CoinValue((-profits) as u128);
            } else if lost_coins > CoinValue(0) {
                let p = CoinValue(profits as u128);
                if lost_coins <= p {
                    lost_coins = CoinValue(0);
                } else {
                    lost_coins -= p;
                }
            }
        }

        if lost_coins > CoinValue(0) {
            let first = fh[0];
            let last = fh[fh_len - 1];
            log::warn!("WARNING: our MEL coins losts in {:?}! the mint profit might be a negative! first coins: {} -> last coins: {} (lost coins: - {})", last.time.duration_since(first.time).unwrap_or(Duration::new(0, 0)), first.balance, last.balance, first.balance - last.balance);
        }

        if lost_coins >= max_lost {
            if quit {
                let orig_hook = std::panic::take_hook();
                std::panic::set_hook(Box::new(move |panic_info| {
                    orig_hook(panic_info);
                    std::process::exit(91);
                }));

                panic!("Melminter balance fail-safe started! total-lost-coins {} >= {}(max) ! quit minting to keep your balances!", lost_coins, max_lost);
            }
        }
    }

    /// unlock mint-wallet, first try plaintext, second try empty password if fails, final return error if still failed.
    pub async fn unlock(&self) -> surf::Result<()> {
        if self.wallet.summary().await?.locked {
            if let Err(_) = self.wallet.unlock(None).await {
                self.wallet.unlock(Some("".to_string())).await?;
            }
        }
        Ok(())
    }

    /// Generates a list of "seed" coins.
    pub async fn generate_seeds(&mut self, threads: usize) -> surf::Result<()> {
        self.unlock().await?;

        let my_address = self.wallet.summary().await?.address;
        loop {
            let toret = self.get_seeds_raw().await?;
            if toret.len() >= threads {
                return Ok(());
            }
            // generate a bunch of custom-token utxos
            let tx = self
                .wallet
                .prepare_transaction(
                    TxKind::Normal,
                    vec![],
                    std::iter::repeat_with(|| CoinData {
                        covhash: my_address,
                        denom: Denom::NewCoin,
                        value: CoinValue(1),
                        additional_data: vec![],
                    })
                    .take(threads)
                    .collect(),
                    vec![],
                    vec![],
                    vec![],
                )
                .await?;

            let fees = tx.fee;
            self.fee_history.push(FeeRecord{
                f: 1,
                time: SystemTime::now(),
                balance: self.get_balance().await?,
                fee: fees,
                income: CoinValue(0),
            });

            let sent_hash = self.wallet.send_tx(tx).await?;
            log::debug!("(fee-safe) sent newcoin tx with fee: {}", fees);
            self.wallet.wait_transaction(sent_hash).await?;
        }
    }

    async fn get_seeds_raw(&self) -> surf::Result<Vec<CoinID>> {
        let unspent_coins = self.wallet.get_coins().await?;

        Ok(unspent_coins
            .iter()
            .filter_map(|(k, v)| {
                if matches!(v.denom, Denom::Custom(_)) {
                    Some((k, v))
                } else {
                    None
                }
            })
            .map(|d| *d.0)
            .collect())
    }

    /// Creates a partially-filled-in transaction, with the given difficulty, that's neither signed nor feed. The caller should fill in the DOSC output.
    pub async fn mint_batch(
        &self,
        difficulty: usize,
        on_progress: impl Fn(usize, f64) + Sync + Send + 'static,
        threads: usize,
    ) -> surf::Result<Vec<(CoinID, CoinDataHeight, Vec<u8>)>> {
        let seeds = self.get_seeds_raw().await?;
        let on_progress = Arc::new(on_progress);
        let mut proofs = Vec::new();
        for (idx, seed) in seeds.iter().copied().take(threads).enumerate() {
            let tip_cdh =
                repeat_fallible(|| async { self.client.snapshot().await?.get_coin(seed).await })
                    .await
                    .context("transaction's input spent from behind our back")?;
            // log::debug!("tip_cdh = {:#?}", tip_cdh);
            let snapshot = self.client.snapshot().await?;
            // log::debug!("snapshot height = {}", snapshot.current_header().height);
            let tip_header_hash = repeat_fallible(|| snapshot.get_older(tip_cdh.height))
                .await
                .current_header()
                .hash();
            let chi = tmelcrypt::hash_keyed(&tip_header_hash, &seed.stdcode());
            let on_progress = on_progress.clone();
            // let core_ids = core_affinity::get_core_ids().unwrap();
            // let core_id = core_ids[idx % core_ids.len()];
            let proof_fut = std::thread::spawn(move || {
                // core_affinity::set_for_current(core_id);
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
            proofs.push(proof_fut);
        }
        let mut out = vec![];
        for (seed, proof) in seeds.into_iter().zip(proofs.into_iter()) {
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

        let own_cov = self.wallet.summary().await?.address;
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
            log::warn!("WARNING: This doscMint fee({} MEL) great-than-or-equal to income({} MEL) amount! you should check your difficulty or a melnet issue.", fees, mels);
        }

        self.fee_history.push(FeeRecord{
            f: 2,
            time: SystemTime::now(),
            balance: self.get_balance().await?,
            fee: fees,
            income: mels,
        });

        let txhash = self.wallet.send_tx(tx).await?;
        log::debug!("(fee-safe) sent DoscMint tx with fee: {}", fees);
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

        let my_address = self.wallet.summary().await?.address;
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
            log::warn!("WARNING: This ERG-to-MEL swap fee({} MEL) great-than-or-equal to income({} MEL) amount! you should check your difficulty or a melnet issue.", fees, mels);
        }

        self.fee_history.push(FeeRecord{
            f: 3,
            time: SystemTime::now(),
            balance: self.get_balance().await?,
            fee: fees,
            income: mels,
        });

        let txhash = self.wallet.send_tx(tx).await?;
        log::debug!("(fee-safe) sent ERG-to-MEL swap tx with fee: {}", fees);
        self.wallet.wait_transaction(txhash).await?;
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
