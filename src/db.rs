use std::path::Path;
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
    let conf = dirs::config_dir();
    if let Some(mut dir) = conf {
        dir.push(env!("CARGO_PKG_NAME"));
        dir.push(DB_FILENAME);
        return Ok( dir.into_boxed_path() );
    }
    
    let mut path = std::env::current_exe().context("Unexpected program itself path undefined...")?;
    path.set_extension(DB_FILENAME);
    return Ok( path.into_boxed_path() );
}

pub fn db_open() -> anyhow::Result<boringdb::Database> {
    Ok( boringdb::Database::open( db_path()? )? )
}

