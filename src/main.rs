//! rpc2sqlite — download the Swiss Produkteregister Chemikalien (RPC) into SQLite.
//!
//! 1. Page through the public product search (empty filter, `size` items per page)
//!    and store every hit in `products`.
//! 2. Fetch the detail record of every cpId with a pool of worker threads and
//!    store it in `product_details` plus the normalised child tables.
//!
//! Both steps are resumable: re-running against the same database only
//! fetches details that are still missing (unless `--refresh` is given).

mod db;
mod download;

use clap::Parser;
use rusqlite::Connection;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

const APP_DIR_NAME: &str = "rpc2sqlite";

/// `~/rpc2sqlite/` (or `%USERPROFILE%\rpc2sqlite\`), created on demand.
fn app_data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    let home = std::env::var_os("USERPROFILE");
    #[cfg(not(target_os = "windows"))]
    let home = std::env::var_os("HOME");
    let dir = home
        .map(|h| PathBuf::from(h).join(APP_DIR_NAME))
        .unwrap_or_else(|| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn default_db_path() -> String {
    let dir = app_data_dir().join("db");
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!("rpc_{}.db", chrono::Local::now().format("%d.%m.%Y")))
        .to_string_lossy()
        .into_owned()
}

#[derive(Parser, Debug)]
#[command(name = "rpc2sqlite", version, about)]
struct Args {
    /// SQLite file to write (default: ~/rpc2sqlite/db/rpc_DD.MM.YYYY.db)
    #[arg(long)]
    db: Option<String>,

    /// Items per search page
    #[arg(long, default_value_t = 500)]
    page_size: u32,

    /// Parallel detail downloads
    #[arg(long, default_value_t = 8)]
    threads: usize,

    /// Skip the search list; only fetch details still missing in the DB
    #[arg(long)]
    details_only: bool,

    /// Skip detail pages; only download the search list
    #[arg(long)]
    list_only: bool,

    /// Re-download details even if already present
    #[arg(long)]
    refresh: bool,

    /// Stop after this many products (for testing)
    #[arg(long)]
    limit: Option<usize>,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn run() -> Result<(), db::Error> {
    let args = Args::parse();
    let db_path = args.db.clone().unwrap_or_else(default_db_path);
    eprintln!("Database: {}", db_path);
    let mut conn = db::open(&db_path)?;

    let client = download::RpcClient::new()?;
    eprintln!("XSRF token obtained.");

    if !args.details_only {
        download_list(&client, &mut conn, &args)?;
    }
    if !args.list_only {
        download_details(&client, &mut conn, &args)?;
    }

    eprintln!("\nSummary for {}:", db_path);
    for t in [
        "products",
        "product_details",
        "components",
        "classifications",
        "h_phrases",
        "p_phrases",
        "symbols",
        "trade_names",
    ] {
        eprintln!("  {:<16} {:>8}", t, db::count(&conn, t)?);
    }
    Ok(())
}

fn download_list(client: &download::RpcClient, conn: &mut Connection, args: &Args) -> Result<(), db::Error> {
    let start = Instant::now();
    let mut stored = 0usize;
    let limit = args.limit.unwrap_or(usize::MAX);
    let page_size = args.page_size.min(limit.try_into().unwrap_or(u32::MAX)).max(1);

    let mut done = false;
    download::download_all_products(client, page_size, |items| {
        if done {
            return Ok(());
        }
        let tx = conn.transaction()?;
        for p in items {
            if stored >= limit {
                done = true;
                break;
            }
            db::insert_product(&tx, p)?;
            stored += 1;
        }
        tx.commit()?;
        if done {
            // Signal the pager to stop by pretending the page was empty.
            return Err("__limit_reached__".into());
        }
        Ok(())
    })
    .or_else(|e| {
        if e.to_string() == "__limit_reached__" {
            Ok(0)
        } else {
            Err(e)
        }
    })?;

    eprintln!(
        "[list] stored {} products in {:.0?}",
        stored,
        start.elapsed()
    );
    Ok(())
}

fn download_details(client: &download::RpcClient, conn: &mut Connection, args: &Args) -> Result<(), db::Error> {
    let mut ids: Vec<String> = if args.refresh {
        let mut stmt = conn.prepare("SELECT cp_id FROM products ORDER BY cp_id")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<Result<_, _>>()?
    } else {
        db::missing_details(conn)?
    };
    if let Some(l) = args.limit {
        ids.truncate(l);
    }
    let total = ids.len();
    if total == 0 {
        eprintln!("[detail] nothing to fetch.");
        return Ok(());
    }
    eprintln!("[detail] fetching {} detail records with {} threads", total, args.threads);

    let start = Instant::now();
    let queue = Arc::new(Mutex::new(ids.into_iter()));
    let (tx_res, rx_res) = mpsc::channel::<(String, Result<Value, String>)>();

    let mut workers = Vec::new();
    for _ in 0..args.threads.max(1) {
        let queue = Arc::clone(&queue);
        let client = client.clone();
        let tx_res = tx_res.clone();
        workers.push(thread::spawn(move || loop {
            let next = queue.lock().unwrap().next();
            let Some(cp_id) = next else { break };
            let res = client.product_detail(&cp_id).map_err(|e| e.to_string());
            if tx_res.send((cp_id, res)).is_err() {
                break;
            }
        }));
    }
    drop(tx_res);

    let mut ok = 0usize;
    let mut failed: Vec<(String, String)> = Vec::new();
    let mut batch: Vec<(String, Value)> = Vec::new();
    let flush = |conn: &mut Connection, batch: &mut Vec<(String, Value)>| -> Result<(), db::Error> {
        if batch.is_empty() {
            return Ok(());
        }
        let now = chrono::Local::now().to_rfc3339();
        let tx = conn.transaction()?;
        for (cp_id, d) in batch.drain(..) {
            db::insert_detail(&tx, &cp_id, &d, &now)?;
        }
        tx.commit()?;
        Ok(())
    };

    for (cp_id, res) in rx_res {
        match res {
            Ok(d) => {
                ok += 1;
                batch.push((cp_id, d));
            }
            Err(e) => {
                eprintln!("[detail] {} failed: {}", cp_id, e);
                failed.push((cp_id, e));
            }
        }
        if batch.len() >= 200 {
            flush(conn, &mut batch)?;
        }
        let done = ok + failed.len();
        if done % 1000 == 0 || done == total {
            let el = start.elapsed().as_secs_f64();
            let rate = done as f64 / el.max(0.001);
            let eta = (total - done) as f64 / rate.max(0.001);
            eprintln!(
                "[detail] {}/{} ({:.1}/s, {} failed, eta {:.0} min)",
                done,
                total,
                rate,
                failed.len(),
                eta / 60.0
            );
        }
    }
    flush(conn, &mut batch)?;
    for w in workers {
        let _ = w.join();
    }

    eprintln!(
        "[detail] done: {} ok, {} failed in {:.0?}",
        ok,
        failed.len(),
        start.elapsed()
    );
    if !failed.is_empty() {
        eprintln!("[detail] failed cpIds (re-run to retry):");
        for (cp, e) in failed.iter().take(50) {
            eprintln!("  {}  {}", cp, e);
        }
    }
    Ok(())
}
