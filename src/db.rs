use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::collections::{HashSet, HashMap};
use std::sync::Arc;

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

/// a raw dict map for bytes -> bytes mapping
#[derive(Clone)]
pub struct DictMap {
    md_keylist: Vec<u8>, // store name of metadata storage
    dict: Option< Arc<boringdb::Dict> >,
    name: String,
}
impl DictMap {
    pub fn open(name: &str) -> anyhow::Result<Self> {
        Ok(Self {
            md_keylist: (TABLE_METADATA.clone().to_owned()+".keys").as_bytes().to_vec(),
            dict: Some( Arc::new(dict_open(name)?) ),
            name: name.to_string(),
        })
    }
    pub fn close(&mut self) -> bool {
        if self.dict.is_some() {
            self.dict = None;
            true
        } else {
            false
        }
    }
    pub fn is_closed(&self) -> bool {
        self.dict.is_none()
    }

    pub fn name(&self) -> String {
        self.name.clone()
    }

    pub fn get(&self, key: &[u8]) -> anyhow::Result< Option<Vec<u8>> > {
        if self.is_closed() {
            return Err(anyhow::Error::msg("try access to a closed dict map."));
        }

        let ks = self.keys()?;
        if ks.len() <= 0 {
            return Ok(None);
        }

        if ks.get(key).is_some() {
            let dict = self.dict.as_ref().unwrap().as_ref();
            if let Some(res) = dict.get(key)? {
                Ok(Some( res.to_vec() ))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    // checked key valid value. returns .to_vec value if valid, or anyhow::Error as rejected.
    // 1. key starts with '.' or '_' is keep to internal used (.field / _data); forbidden use these as general data store.
    //        (also allow suffix _ or middle . for user-specified)
    // 2. current only limited for write access
    pub fn to_key(key: &[u8]) -> anyhow::Result<Vec<u8>> {
        let k = key.to_vec();

        if k.starts_with(TABLE_METADATA.clone().as_bytes()) {
            // Disallow modify to metadata
            return Err(anyhow::Error::msg("Try read/write to Internal metadata!"));
        }
        match k[0] as char {
            '_' => {
                return Err(anyhow::Error::msg("Try access to Internal key prefix '_'"));
            },
            '.' => {
                return Err(anyhow::Error::msg("invalid key starts with '.'"));
            },
            _ => {
                // passing normal key
            }
        }

        Ok(k)
    }

    pub fn set(&mut self, key: &[u8], value: &[u8]) -> anyhow::Result< Option<()> > {
        if self.is_closed() {
            return Err(anyhow::Error::msg("try write to a closed dict map."));
        }

        let k = Self::to_key(key)?;
        let mut ks = self.keys()?;

        let dict = &mut Arc::pin(self.dict.as_ref().unwrap());
        dict.insert(k.clone(), value.to_vec())?;
        ks.insert(k);

        dict.insert(self.md_keylist.clone(), bincode::serialize(&ks)?)?;
        Ok( Some(()) )
    }

    pub fn keys(&self) -> anyhow::Result< HashSet<Vec<u8>> > {
        let dict = self.dict.as_ref().unwrap().as_ref();
        if let Some(mdata) = dict.get(&self.md_keylist)? {
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

/// a public interface for outside
#[derive(Clone)]
pub struct Map {
    dicts: Vec<DictMap>,
    curr: Option<usize>,
    lowercase: bool,
}
impl Map {
    pub fn new() -> Self {
        Self {
            dicts: vec![],
            curr: None,
            lowercase: false,
        }
    }

    /// auto convert all key to lowercase
    /// Defaults to false.
    pub fn lower(&mut self) {
        self.lowercase = true;
    }

    /// changes current dict mapping to specified name
    pub fn dict(&mut self, name: &str) -> anyhow::Result<()> {
        for i in 0 .. self.dicts.len() {
            let d = &self.dicts[i];
            if d.name() == name {
                self.curr = Some(i);
                return Ok(());
            }
        }

        let dm = DictMap::open(name)?;
        assert_eq!(dm.name(), name);

        let n = self.dicts.len();
        self.dicts.push(dm);
        assert_eq!(self.dicts[n].name, name);

        self.curr = Some(n);
        Ok(())
    }

    /// unset current mapping (NOTE this does not to "free" the object, please see self.clean() if you needs.
    pub fn no_dict(&mut self) {
        self.curr = None;
    }

    /// delete all-or-one [object of mapping], give some name to option, otherwise all.
    pub fn clean(&mut self, name: Option<&str>) {
        if let Some(n) = name {
            for it in &mut self.dicts {
                if it.name == n {
                    it.close();
                }
            }
        } else {
            for it in &mut self.dicts {
                it.close();
            }
            self.dicts.clear();
        }
    }
}

