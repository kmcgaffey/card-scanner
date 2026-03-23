# tcg-scanner

Rust library and CLI for scraping price data from [tcgplayer.com](https://www.tcgplayer.com) via their internal JSON APIs. Collects product details, seller listings, price points, sales history, and market prices per SKU. Stores time-series data in SQLite for analytics.

## Quick start

```bash
cargo build --release
./target/release/tcg-scanner --help
```

## CLI commands

```bash
# Collect all price data for a set (defined in profiles.toml)
tcg-scanner collect <profile>

# Scan for volume spikes across a set
tcg-scanner alerts <profile> [-t threshold] [-r range]

# Fetch and display a single product by TCGPlayer product ID
tcg-scanner fetch <product_id>

# List available profiles
tcg-scanner profiles
```

### `collect`

Runs a full price collection for every card in a profile's set. Phases:
1. Discovers all cards via the search API
2. Fetches the cheapest English listing per card (filters out non-English custom listings)
3. Fetches price points per printing type (Normal/Foil)
4. Fetches latest sales and deduplicates into the DB
5. Computes 2-day rolling average sale price

Each run appends new `price_snapshots` and `price_points` rows timestamped with `captured_at`, building a history over repeated runs.

### `alerts`

Scans every card in a set for volume spikes by comparing recent daily sales to the rolling average from the price history API.

- `--threshold` (`-t`): spike multiplier, default `2.0` (alerts when daily volume >= 2x average)
- `--range` (`-r`): averaging window — `month`, `quarter`/`3m`, `semi`/`6m`, `annual`/`1y`

### `fetch`

Displays all available data for a single product: details, attributes, SKUs, price points, per-SKU market prices, current listings, and recent sales.

## Configuration

### `profiles.toml`

Defines named collection profiles. Each profile maps to a TCGPlayer product line + set and a local SQLite database file.

```toml
[spiritforged]
product_line = "Riftbound: League of Legends Trading Card Game"
set_name = "Spiritforged"
product_type = "Cards"
db_path = "spiritforged_prices.db"

[origins]
product_line = "Riftbound: League of Legends Trading Card Game"
set_name = "Origins"
product_type = "Cards"
db_path = "origins_prices.db"
```

To add a new set, add a new TOML section. The `product_line`, `set_name`, and `product_type` values must match what TCGPlayer uses in their search API (visible on the TCGPlayer website URL structure or via `tcg-scanner fetch`).

## Database schema

Each profile's `.db` file is SQLite with four tables:

### `cards`
Static card metadata, upserted each run.

| Column | Type | Description |
|---|---|---|
| product_id | INTEGER PK | TCGPlayer product ID |
| product_name | TEXT | Full display name |
| clean_name | TEXT | URL-safe name |
| set_name | TEXT | Set name |
| product_line | TEXT | Product line name |
| rarity | TEXT | Rarity tier (Common, Uncommon, Rare, Epic, Showcase) |
| card_number | TEXT | Collector number (e.g. "236/221") |
| card_type | TEXT | Card type (e.g. "Champion Unit") |
| domain | TEXT | Domain/region (e.g. "Order") |
| energy_cost | TEXT | Energy cost |
| power_cost | TEXT | Power cost |
| might | TEXT | Might stat |
| tag | TEXT | Tags, semicolon-separated |
| foil_only | INTEGER | 1 if card only exists in foil |
| normal_only | INTEGER | 1 if card only exists in normal |
| updated_at | TEXT | ISO 8601 timestamp of last update |

### `price_snapshots`
One row per card per collection run. Primary table for tracking price changes over time.

| Column | Type | Description |
|---|---|---|
| id | INTEGER PK | Auto-increment |
| product_id | INTEGER FK | References cards.product_id |
| captured_at | TEXT | ISO 8601 timestamp of this snapshot |
| tcg_market_price | REAL | TCGPlayer's algorithmic "Market Price" (often inaccurate) |
| tcg_lowest_price | REAL | Absolute lowest listing (may include non-English cards) |
| tcg_median_price | REAL | TCGPlayer median price |
| tcg_lowest_with_shipping | REAL | Lowest price including shipping |
| total_listings | INTEGER | Number of active listings |
| lowest_english_price | REAL | Cheapest listing confirmed as English (filters out Chinese/Japanese/etc.) |
| lowest_english_with_ship | REAL | Same, including shipping |
| lowest_english_seller | TEXT | Seller name for the lowest English listing |
| avg_2day_sale_price | REAL | Average purchase price of sales in the last 48 hours |
| sales_2day_count | INTEGER | Number of sales in the last 48 hours |

**Pricing notes:** `tcg_market_price` is TCGPlayer's spotlight price and is often not what cards actually sell for. `lowest_english_price` filters out cards listed as "English" but marked as Chinese/Japanese/etc. in the custom listing title. `avg_2day_sale_price` is computed from actual completed transactions.

### `price_points`
Per-printing-type price data (Normal vs Foil). One row per printing type per card per run.

| Column | Type | Description |
|---|---|---|
| id | INTEGER PK | Auto-increment |
| product_id | INTEGER FK | References cards.product_id |
| captured_at | TEXT | ISO 8601 timestamp |
| printing_type | TEXT | "Normal" or "Foil" |
| market_price | REAL | Market price for this printing |
| buylist_price | REAL | Buylist market price |
| listed_median | REAL | Median of active listings |

### `sales`
Individual completed transactions. Deduplicated via unique constraint across runs.

| Column | Type | Description |
|---|---|---|
| id | INTEGER PK | Auto-increment |
| product_id | INTEGER FK | References cards.product_id |
| order_date | TEXT | ISO 8601 timestamp of sale |
| purchase_price | REAL | Sale price |
| shipping_price | REAL | Shipping cost |
| condition | TEXT | Card condition (Near Mint, Lightly Played, etc.) |
| variant | TEXT | Printing variant (Foil, Normal) |
| language | TEXT | Language |
| quantity | INTEGER | Number of copies in this transaction |
| listing_type | TEXT | "standard" or "custom" |

Unique on: `(product_id, order_date, purchase_price, condition, variant, quantity)`

## Project structure

```
├── Cargo.toml              # Package manifest and dependencies
├── profiles.toml            # Collection profile definitions (not in git)
├── src/
│   ├── main.rs              # CLI entry point (clap subcommands)
│   ├── lib.rs               # Library re-exports
│   ├── client.rs            # TcgClient — high-level API wrapper
│   ├── error.rs             # Error types (TcgError enum)
│   ├── cmd/                 # CLI subcommand implementations
│   │   ├── mod.rs
│   │   ├── common.rs        # Shared: profile loading, retry logic
│   │   ├── collect.rs       # `collect` subcommand: full price collection pipeline
│   │   ├── alerts.rs        # `alerts` subcommand: volume spike detection
│   │   └── fetch.rs         # `fetch` subcommand: single product display
│   ├── api/                 # TCGPlayer API endpoint wrappers
│   │   ├── mod.rs
│   │   ├── details.rs       # GET  mp-search-api.tcgplayer.com/v1/product/{id}/details
│   │   ├── listings.rs      # POST mp-search-api.tcgplayer.com/v1/product/{id}/listings
│   │   ├── search.rs        # POST mp-search-api.tcgplayer.com/v1/search/request
│   │   ├── price_points.rs  # GET  mpapi.tcgplayer.com/v2/product/{id}/pricepoints
│   │   ├── latest_sales.rs  # POST mpapi.tcgplayer.com/v2/product/{id}/latestsales
│   │   ├── market_prices.rs # POST mpgateway.tcgplayer.com/v1/pricepoints/marketprice/skus/search
│   │   └── price_history.rs # GET  infinite-api.tcgplayer.com/price/history/{id}/detailed
│   └── models/              # Data structures for API responses
│       ├── mod.rs
│       ├── product.rs       # ProductDetails, ProductPage, PricePoint, SkuMarketPrice, Sku, CustomAttributes
│       ├── listing.rs       # Listing, CustomListingData, ListingsApiOuterResponse
│       ├── price.rs         # Volatility
│       └── sale.rs          # Sale, LatestSalesApiResponse, LatestSaleEntry
└── *.db                     # SQLite databases (gitignored, created by `collect`)
```

## TCGPlayer API endpoints used

| Endpoint | Method | Auth | Purpose |
|---|---|---|---|
| `mp-search-api.tcgplayer.com/v1/product/{id}/details` | GET | None | Product details, attributes, SKUs |
| `mp-search-api.tcgplayer.com/v1/product/{id}/listings` | POST | Origin header | Seller listings with pagination |
| `mp-search-api.tcgplayer.com/v1/search/request?q=...` | POST | Origin header | Product search with filters |
| `mpapi.tcgplayer.com/v2/product/{id}/pricepoints` | GET | None | Price points per printing type |
| `mpapi.tcgplayer.com/v2/product/{id}/latestsales` | POST | Origin header | Recent completed sales |
| `mpgateway.tcgplayer.com/v1/pricepoints/marketprice/skus/search` | POST | None | Market prices per SKU |
| `infinite-api.tcgplayer.com/price/history/{id}/detailed` | GET | None | Per-SKU daily price history buckets |

## Dependencies

- **reqwest** — HTTP client
- **serde / serde_json** — JSON serialization
- **clap** — CLI argument parsing
- **rusqlite** — SQLite storage (bundled, no system dependency)
- **chrono** — Timestamp handling
- **toml** — Profile config parsing
- **thiserror** — Error type derivation
- **urlencoding** — URL query parameter encoding

## Example queries

```sql
-- Top cards by 2-day average sale price
SELECT c.product_name, c.rarity, p.lowest_english_price, p.avg_2day_sale_price
FROM price_snapshots p
JOIN cards c ON c.product_id = p.product_id
WHERE p.avg_2day_sale_price IS NOT NULL
ORDER BY p.avg_2day_sale_price DESC LIMIT 10;

-- Price change between two collection runs
SELECT c.product_name,
       p1.lowest_english_price as before,
       p2.lowest_english_price as after,
       ROUND(p2.lowest_english_price - p1.lowest_english_price, 2) as change
FROM price_snapshots p1
JOIN price_snapshots p2 ON p1.product_id = p2.product_id
JOIN cards c ON c.product_id = p1.product_id
WHERE p1.captured_at = '<earlier_timestamp>'
  AND p2.captured_at = '<later_timestamp>'
ORDER BY change DESC LIMIT 10;

-- Top 5 most-sold cards by rarity in last 48 hours
SELECT c.rarity, c.product_name, SUM(s.quantity) as total_qty,
       ROUND(AVG(s.purchase_price), 2) as avg_price
FROM sales s
JOIN cards c ON c.product_id = s.product_id
WHERE s.order_date >= datetime('now', '-48 hours')
GROUP BY s.product_id
ORDER BY c.rarity, total_qty DESC;
```
