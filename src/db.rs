use std::path::{Path, PathBuf};
use std::hash::Hash;
use std::time::SystemTime;
use std::collections::{HashSet, HashMap};
use std::sync::Arc;

use sqlite3;
//use boringdb;
use dirs;
use anyhow::Context;

use serde::{Serialize, Deserialize, de::DeserializeOwned};
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
pub fn db_open() -> anyhow::Result< Arc<sqlite3::Connection> > {
    if let Some(dir) = confdir() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .create(dir)?;
    }

    Ok(Arc::new( sqlite3::open( db_path()? )? ))
}

/// a helper for automatic serde
#[derive(Clone)]
pub struct Dict {
    db: Option<Arc<sqlite3::Connection>>,
    table: String,
}
impl Dict {
    pub fn open(table: &str) -> anyhow::Result<Self> {
        let table = table.to_string();

        let db = db_open()?;
        // create table using caller-specified name
        let mut cu = db.prepare("CREATE TABLE IF NOT EXISTS ? (key BLOB NOT NULL UNIQUE, value BLOB NOT NULL)")?.cursor();
        cu.bind(&[ sqlite3::Value::Binary(table.as_bytes().to_vec()) ]);
        cu.next()?;

        Ok(Self {
            db: Some(db),
            table,
        })
    }

    pub fn close(&mut self) -> bool {
        if self.db.is_some() {
            self.db = None;
            true
        } else {
            false
        }
    }
    pub fn is_closed(&self) -> bool {
        self.db.is_none()
    }

    pub fn name(&self) -> String {
        self.table.clone()
    }

    fn _insert(&self, key: &[u8], value: &[u8]) -> anyhow::Result<()> {
        let mut cu = self.db.unwrap().prepare("INSERT INTO ? VALUES (?, ?)")?.cursor();
        cu.bind( &[ sqlite3::Value::String(self.table.clone()), sqlite3::Value::Binary(key), sqlite3::Value::Binary(value) ] );
    }

    pub fn _get(&self, key: &[u8]) -> anyhow::Result< Option<Vec<u8>> > {
        if self.is_closed() {
            return Err(anyhow::Error::msg("try access to a closed dict map."));
        }

        let ks = self.keys()?;
        if ks.len() <= 0 {
            // empty table...
            return Ok(None);
        }

        if ks.get(key).is_some() {
            self.db.execute
//            let dict = self.dict.as_ref().unwrap().as_ref();
            if let Some(res) = dict.get(key)? {
                Ok(Some( res.to_vec() ))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }
    pub fn _set(&mut self, key: &[u8], value: &[u8]) -> anyhow::Result< Option<()> > {
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

    pub fn get<K: Serialize, V: DeserializeOwned>(&self, key: K) -> anyhow::Result<Option< Box<V> >> {
        let k = bincode::serialize(&key)?;
        if let Some(v) = self._get(&k)? {
            let out: V = bincode::deserialize(&v)?;
            Ok(Some( Box::new(out) ))
        } else {
            Ok(None)
        }
    }
    pub fn set<K: Serialize, V: Serialize>(&mut self, key: K, value: V) -> anyhow::Result<()> {
        let k = bincode::serialize(&key)?;
        let v = bincode::serialize(&value)?;
        self._set( &k, &v );
        Ok(())
    }

    pub fn items<K: DeserializeOwned + Eq + Hash, V: DeserializeOwned + PartialEq>(&self) -> anyhow::Result< HashMap<K, V> > {
        let mut cu = self.db.unwrap().prepare("SELECT * FROM ?")?.cursor();
        cu.bind(&[ sqlite3::Value::String(self.table.clone()) ]);

        let mut map: HashMap<K, V> = HashMap::new();
        while let Some(line) = cu.next()? {
            let key: K = bincode::deserialize( line[0].as_binary().unwrap() )?;
            let value: V = bincode::deserialize( line[1].as_binary().unwrap() )?;
            map.insert( key, value );
        }
        Ok(map)
    }

    pub fn keys<K: DeserializeOwned + Eq + Hash>(&self) -> anyhow::Result< HashSet<K> > {
        let mut set = HashSet::new();
        for key in self.items::<K, _>::()?.keys() {
            set.insert(*key);
        }
        Ok(set)
    }
/*
    pub fn flush(&self) -> anyhow::Result<()> {
        let dict = self.dict.as_ref().unwrap().as_ref();
        dict.flush()?;
        Ok(())
    }
*/
}

/*
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

    /// get current DictMap (Arc-pointer just clone)
    pub fn cur(&self) -> DictMap {
        if let Some(ptr) = self.curr {
            self.dicts[ptr].clone()
        } else {
            panic!("no current map sets!");
        }
    }

    pub fn to_key(&self, key: &str) -> anyhow::Result<Vec<u8>> {
        let mut k = key.to_string();
        if self.lowercase {
            k = k.to_ascii_lowercase();
        }
        let k = bincode::serialize(&k)?;
        DictMap::to_key(&k)
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

    /// unset current mapping (NOTE this does not "free" the object, please see self.clean() if you needs.
    pub fn no_dict(&mut self) {
        self.curr = None;
    }

    /// try flush all dicts
    pub fn flush(&mut self) -> anyhow::Result<()> {
        for dict in &mut self.dicts {
            if ! dict.is_closed() {
                dict.flush()?;
            }
        }

        Ok(())
    }

    /// delete all-or-one [object of mapping], give some name to option, otherwise all.
    pub fn clean(&mut self, name: Option<&str>) {
        if let Some(n) = name {
            if self.cur().name == n {
                self.no_dict();
            }
            for it in &mut self.dicts {
                if it.name == n {
                    it.close();
                }
            }
        } else {
            self.no_dict();
            for it in &mut self.dicts {
                it.close();
            }
            self.dicts.clear();
        }
    }
}
*/

