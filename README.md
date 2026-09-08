# rpc2sqlite

Download the complete Swiss **Produkteregister Chemikalien (RPC)** from
[gate.bag.admin.ch/rpc](https://www.gate.bag.admin.ch/rpc/ui/search/general)
and store it in a SQLite database: the full product list plus the detail
record of every product (formulation components, GHS classification,
H/P phrases, pictograms, trade names, licence holder).

Written in Rust, analogous to [swissdamed2sqlite](https://github.com/zdavatz/swissdamed2sqlite).

## Build

```bash
cargo build --release
```

The binary is `target/release/rpc2sqlite`.

## Usage

```bash
# Full download (list + all details) into ~/rpc2sqlite/db/rpc_DD.MM.YYYY.db
rpc2sqlite

# Write to a specific file
rpc2sqlite --db rpc.db

# Only the search list, no detail pages
rpc2sqlite --list-only

# Resume: fetch only details that are still missing in the DB
rpc2sqlite --details-only

# Re-download details already present
rpc2sqlite --refresh

# Tuning / testing
rpc2sqlite --threads 8 --page-size 500 --limit 100
```

Re-running against an existing database is safe: list rows are upserted
and only missing details are fetched.

## How it works

The public Angular UI calls a JSON API under `/rpc/public/api`:

| Step | Request |
|------|---------|
| XSRF cookie | `GET /rpc/ui/search/general` sets `XSRF-TOKEN` (Path `/rpc`) |
| Product list | `POST /rpc/public/api/products/advance-search?page=N&size=500`, body `{}` |
| Product detail | `GET /rpc/public/api/products/product/{cpId}` |

Every API call must send the cookie value as `X-XSRF-TOKEN` header
(plus `X-Requested-With: XMLHttpRequest`), otherwise the server returns 403.
The list is a Spring `Page`; paging stops when `last` is `true`.

## Database schema

| Table | Content |
|-------|---------|
| `products` | one row per cpId from the search list (name, type, status, licence holder address, UFIs, H sentences, pictograms, usages) |
| `product_details` | one row per cpId from the detail page (formulation type, classification flags, signal word, use categories, biocide/fertilizer fields) |
| `components` | formulation components with parameter id, name, CAS, EC number, concentrations |
| `classifications` | GHS hazard classes/categories with texts in de/fr/it/en |
| `h_phrases`, `p_phrases`, `symbols` | labeling phrases and pictograms per product |
| `trade_names` | additional trade names |

Every `products`, `product_details` and `components` row keeps the complete
original JSON in `raw_json`. A full run (08.09.2026) produced an 8.5 GB
database; most of that is `raw_json`.

## Known gaps

About 2 000 of the 290 921 listed products (mostly older `ALTSTOFF` entries
with cpIds in the 846xxx–848xxx range) have no detail record on the server:
the detail endpoint answers 404 `AggregateNotFoundException`. They remain in
`products` with their list data but have no `product_details` row. A few
requests fail transiently with 400/500; `rpc2sqlite --details-only` retries
everything that is still missing.

## Runtime

Full run with 8 threads: about 15 minutes for the list (582 pages of 500)
and about 80 minutes for the 290 921 detail pages.

## License

GPL-3.0-or-later
