use std::path::{Path, PathBuf};
use std::time::SystemTime;

use boringdb;
use dirs;
use anyhow::Context;

use serde::{Serialize, Deserialize};
use themelio_structs::{CoinID, CoinDataHeight};

pub const DB_FILENAME: &str = "melminterdb_sqlite3";

pub const TABLE_PROOF_LIST: &str = "try_send_proofs";
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrySendProof {
    pub coin: CoinID,
    pub data: CoinDataHeight,
    pub proof: Vec<u8>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrySendProofState {
    pub fails: u8,
    pub created: SystemTime,
    pub sent: bool,
    pub failed: bool,
    pub errors: Vec<String>,
}

pub fn db_path() -> anyhow::Result< Box<Path> > {
    if let Some(mut dir) = confdir() {
        dir.push(DB_FILENAME);
        return Ok( dir.into_boxed_path() );
    }
    
    let mut path = std::env::current_exe().context("Unexpected program itself path undefined...")?;
    path.set_extension(DB_FILENAME);
    return Ok( path.into_boxed_path() );
}

pub fn confdir() -> Option<PathBuf> {
    if let Some(mut dir) = dirs::config_dir() {
        dir.push(env!("CARGO_PKG_NAME"));
        Some(dir)
    } else {
        None
    }
}

pub fn db_open() -> anyhow::Result<boringdb::Database> {
    if let Some(dir) = confdir() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .create(dir)?;
    }

    Ok( boringdb::Database::open( db_path()? )? )
}

pub fn dict_open(name: &str) -> anyhow::Result<boringdb::Dict> {
    let db = db_open()?;
    Ok( db.open_dict(name)? )
}

