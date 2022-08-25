use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::collections::{HashSet, HashMap};

use boringdb;
use dirs;
use anyhow::Context;

use serde::{Serialize, Deserialize};
use themelio_structs::{CoinID, CoinDataHeight};

// filename of database, all db.rs logic need store to one file, does not create others unless major changes to format/function/goals (then these need split to a new .rs file)
pub const DB_FILENAME: &str = "melmintdb_sqlite3";

// table name for metadatas, for now store key list
// format: [table]: (kind) -> (value)
pub const TABLE_METADATA: &str = "_metadata";

// here store the proofs wait queue (please marked done for each sent tx, and auto clean any expires completes they useless)
pub const TABLE_PROOF_LIST: &str = "try_send_proofs";

// next format for log recording, few types (each type each table in most case, otherwise join to related table)
pub const TABLE_LOGS:       &str = "mod_logs";
pub const TABLE_NEWCOINS:   &str = "new_coin_txs";
pub const TABLE_SWAPS:      &str = "erg2mel_swaps";
pub const TABLE_BALANCES:   &str = "balance_history";

/* All data formats:
 * No Any functions about to format/serde/generating-value.
 * just struct only. */

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Metadata {
    table: String, // the name of table, or empty for global metadata table
    kind: MetadataKind, // what type of this data, and that enum also data provided
    info: String, // extra info for this
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum MetadataKind {
    Log(LogRecord),
    Nothing,
    KeyList(HashSet<Vec<u8>>),
    // come soon...
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogRecord {
    id: String, // any identifier for logging, for example the wallet name, user-specified, or empty in case "id-independence does not need marks"
    kind: WhatLog, // what type of this log record

    time: SystemTime, // happen time, all timezone UTC/GMT.

    backtrace: Option<Vec<u8>>, // a dumps of Backtrace(with/without Frame), keep it Nothing if not a nightly build.
    /*[deprecate: unable to Serialize/Debug/Clone]
        backtrace_raw: Option<std::backtrace::Backtrace>, // stack traceback [none with non-program-error]: line number, file name, or extrnal deps info.
    */

    event: String, // which event name to logging (the more info of .kind)
    text: String, // details log content for what's happen

    msg: Vec<u8>, // if required, specify a message (bincode encoded) for more about to this. otherwise lefts empty (length zero)
}
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub enum WhatLog {
    BalanceHistory, // balance history. such as new-coin tx in/out/fee, doscMint size and fee-per-KB, swap transactions fee usage.
    Failsafe, // any fail-safe tiggered

    Log, // General logging (log crate macros info/debug/error/warn) and normal events
    Fatal, // A large of ::Exception, logging any bugs causes program cannot continue.
    Exception, // Any bugs or "panic!" or programming issue, that's cannot resolved without source code changes.

    Quit, // These expected case for must exit process.
    KeyboardInterrupt, // Ctrl+C and handle log

    NewCoin, // new coin tx sent info (any input/output/fees please see balance history)
    Swap, // Swap from ERG to MEL...
    Proof, // A proof generated: take how many time / processors usage / how many hashrate and total-hashes
    StorageProblem, // database storage issue (such as disk full, format corrupt, or sqlite3/boringdb error)

    Test, // for debugging of test.rs
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct TrySendProof {
    // Proof submitting format
    pub coin: CoinID,
    pub data: CoinDataHeight,
    pub proof: Vec<u8>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct TrySendProofState {
    pub fails: u8, // total failed count, only add ops (no any other)
    pub created: SystemTime, // tx prepare time (but not sent)
    pub sent: bool, // is this tx send-ed?
    pub failed: bool, // is this tx complete-failed? other files specify a max retry number and no longer try-again if limit-reach.
    pub errors: Vec<String>, // a list of possible error(s), any error about to this tx should saves here (do not logging to other tables)
}

// Get (read-only) the database file should be located path. (no any writes)
pub fn db_path() -> anyhow::Result< Box<Path> > {
    if let Some(mut dir) = confdir() {
        dir.push(DB_FILENAME);
        return Ok( dir.into_boxed_path() );
    }
    
    let mut path = std::env::current_exe().context("Unexpected program itself path undefined...")?;
    path.set_extension(DB_FILENAME);
    return Ok( path.into_boxed_path() );
}

// Get the current OS configuration directory
pub fn confdir() -> Option<PathBuf> {
    if let Some(mut dir) = dirs::config_dir() {
        dir.push(env!("CARGO_PKG_NAME"));
        Some(dir)
    } else {
        None
    }
}

// Opens the database with fixed location, will creates file/dir if not found.
pub fn db_open() -> anyhow::Result<boringdb::Database> {
    if let Some(dir) = confdir() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .create(dir)?;
    }

    Ok( boringdb::Database::open( db_path()? )? )
}

// just a "shortcut" for db_open().open_dict
pub fn dict_open(name: &str) -> anyhow::Result<boringdb::Dict> {
    let db = db_open()?;
    Ok( db.open_dict(name)? )
}

/*
pub struct Meta {
    inner: boringdb::Dict,
    table: String,
}
impl Meta {
    pub fn new() -> anyhow::Result<boringdb::Dict> {
        db_open(TABLE_METADATA)?
    }

    pub fn list_keys(&self) -> HashSet<Vec<u8>> {
        let mut out = HashSet::new();
        for md in self.inner.get()
    }
}
*/

#[derive(Clone)]
pub struct DictMap {
    md_keylist: Vec<u8>,
    dict: boringdb::Dict,
    name: String,
}
impl DictMap {
    pub fn open(name: &str) -> anyhow::Result<Self> {
        Ok(Self {
            md_keylist: (TABLE_METADATA+".keys").as_bytes().to_vec(),
            dict: dict_open(name)?,
            name: name.to_string(),
        })
    }

    pub fn name(&self) -> String {
        self.name.clone()
    }

    pub fn get(&self, key: &[u8]) -> anyhow::Result< Option<Vec<u8>> > {
        let ks = self.keys()?;
        if ks.len() <= 0 {
            return Ok(None);
        }

        if ks.get(key).is_some() {
            if let Some(res) = self.dict.get(key)? {
                Ok(Some( res.to_vec() ))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }
    pub fn set(&mut self, key: &[u8], value: &[u8]) -> anyhow::Result< Option<()> > {
        let k = key.to_vec();
        let mut ks = self.keys()?;

        self.dict.insert(k.clone(), value.to_vec())?;
        ks.insert(k);

        self.dict.insert(self.md_keylist.clone(), bincode::serialize(&ks)?)?;
        Ok( Some(()) )
    }

    pub fn keys(&self) -> anyhow::Result< HashSet<Vec<u8>> > {
        if let Some(mdata) = self.dict.get(&self.md_keylist)? {
            let mdata: Metadata = bincode::deserialize(&mdata)?;
            match mdata.kind {
                MetadataKind::KeyList(kl) => {
                    return Ok(kl.clone());
                },
                _ => {
                    return Err(anyhow::Error::msg("metadata type not equal KeyList"));
                },
            }
        } else {
            return Ok(HashSet::new());
        }
    }
}

