# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

rpc2sqlite is a Rust CLI that downloads the Swiss Produkteregister Chemikalien (RPC, ~291k products) from the public API behind https://www.gate.bag.admin.ch/rpc/ui and writes it to SQLite. It follows the structure of the sibling project `~/.software/swissdamed2sqlite` (reqwest blocking + rusqlite bundled + clap). GPLv3.

## Commands

```bash
cargo build --release                       # binary: target/release/rpc2sqlite
cargo run --release -- --db /tmp/t.db --limit 30 --threads 4   # quick end-to-end test
./target/release/rpc2sqlite                 # full run → ~/rpc2sqlite/db/rpc_DD.MM.YYYY.db
./target/release/rpc2sqlite --details-only  # resume missing details
```

There is no test suite; verify changes with `--limit` against a scratch DB and inspect with `sqlite3`.

## Architecture

- `src/download.rs` — `RpcClient`: obtains the `XSRF-TOKEN` cookie by GETting the UI page, then calls `/rpc/public/api`. Every request must carry `X-XSRF-TOKEN` and `X-Requested-With: XMLHttpRequest`, else 403. `search_page` posts `{}` to `products/advance-search?page&size` (Spring `Page`, stop on `last`); `product_detail` GETs `products/product/{cpId}`. Both retry on 5xx/network errors.
- `src/db.rs` — schema creation and `insert_product` / `insert_detail`. Columns are pulled from the JSON with small path helpers (`gs`, `gi`, `gb`, `join`); the full JSON is always stored in `raw_json`, so add columns rather than worrying about lost data. Detail insert deletes and rewrites child rows for that cpId.
- `src/main.rs` — CLI. List download runs single-threaded with one transaction per page. Details run on a worker pool (`--threads`) feeding an mpsc channel; the main thread batches 200 records per transaction. `missing_details()` makes re-runs resumable.

## API facts worth remembering

- `size=500` per page works (about 2 MB, under 1 s); 290 921 products → 582 pages.
- Detail fetch is ~0.15 s each; with 8 threads the full register took ~80 min (list ~15 min). Resulting DB is ~8.5 GB, mostly `raw_json`.
- ~2 000 listed products (mostly ALTSTOFF, cpIds 846xxx–848xxx) return 404 `AggregateNotFoundException` on the detail endpoint; this is server-side, not a client bug. Expect `product_details` to have ~289k rows, not 291k.
- The `/product/{cpId}/summary` endpoint returns 500 for public users; use `products/product/{cpId}`.
- Product types seen: GEMISCH, ALTSTOFF, BIOZID, FERTILIZER; lists contain only QUALIFIED products.
