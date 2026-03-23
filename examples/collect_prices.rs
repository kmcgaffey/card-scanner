use std::path::PathBuf;
use std::time::Duration;

use chrono::Utc;
use rusqlite::Connection;
use tcg_scanner::api::SearchTermFilters;
use tcg_scanner::{Listing, TcgClient};

const PAGE_SIZE: u32 = 50;
const REQUEST_DELAY_MS: u64 = 350;
const MAX_RETRIES: u32 = 3;
const RETRY_BACKOFF_MS: u64 = 5000;

/// Listings to fetch per card when finding the lowest English price.
/// We grab a few extra to skip past non-English custom listings.
const LISTING_FETCH_SIZE: u32 = 10;

/// Keywords in custom listing titles that indicate a non-English card.
const NON_ENGLISH_KEYWORDS: &[&str] = &[
    "chinese",
    "japanese",
    "korean",
    "french",
    "german",
    "italian",
    "spanish",
    "portuguese",
    "thai",
    "simplified",
    "traditional",
    "cn ",
    "jp ",
    "kr ",
    " cn",
    " jp",
    " kr",
];

fn init_db(conn: &Connection) {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS cards (
            product_id      INTEGER PRIMARY KEY,
            product_name    TEXT NOT NULL,
            clean_name      TEXT,
            set_name        TEXT,
            product_line    TEXT,
            rarity          TEXT,
            card_number     TEXT,
            card_type       TEXT,
            domain          TEXT,
            energy_cost     TEXT,
            power_cost      TEXT,
            might           TEXT,
            tag             TEXT,
            foil_only       INTEGER,
            normal_only     INTEGER,
            updated_at      TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS price_snapshots (
            id                          INTEGER PRIMARY KEY AUTOINCREMENT,
            product_id                  INTEGER NOT NULL,
            captured_at                 TEXT NOT NULL,
            -- TCGPlayer algorithmic prices (for reference)
            tcg_market_price            REAL,
            tcg_lowest_price            REAL,
            tcg_median_price            REAL,
            tcg_lowest_with_shipping    REAL,
            total_listings              INTEGER,
            -- Accurate prices
            lowest_english_price        REAL,
            lowest_english_with_ship    REAL,
            lowest_english_seller       TEXT,
            avg_2day_sale_price         REAL,
            sales_2day_count            INTEGER,
            FOREIGN KEY (product_id) REFERENCES cards(product_id)
        );
        CREATE INDEX IF NOT EXISTS idx_snapshots_product_time
            ON price_snapshots(product_id, captured_at);

        CREATE TABLE IF NOT EXISTS price_points (
            id                  INTEGER PRIMARY KEY AUTOINCREMENT,
            product_id          INTEGER NOT NULL,
            captured_at         TEXT NOT NULL,
            printing_type       TEXT,
            market_price        REAL,
            buylist_price       REAL,
            listed_median       REAL,
            FOREIGN KEY (product_id) REFERENCES cards(product_id)
        );
        CREATE INDEX IF NOT EXISTS idx_pricepoints_product_time
            ON price_points(product_id, captured_at);

        CREATE TABLE IF NOT EXISTS sales (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            product_id      INTEGER NOT NULL,
            order_date      TEXT NOT NULL,
            purchase_price  REAL NOT NULL,
            shipping_price  REAL,
            condition       TEXT,
            variant         TEXT,
            language        TEXT,
            quantity        INTEGER,
            listing_type    TEXT,
            UNIQUE(product_id, order_date, purchase_price, condition, variant, quantity),
            FOREIGN KEY (product_id) REFERENCES cards(product_id)
        );
        CREATE INDEX IF NOT EXISTS idx_sales_product_date
            ON sales(product_id, order_date);
        CREATE INDEX IF NOT EXISTS idx_sales_date
            ON sales(order_date);
        ",
    )
    .expect("Failed to initialize database schema");
}

/// Check if a listing is likely non-English based on custom listing title.
fn is_non_english_listing(listing: &Listing) -> bool {
    if let Some(ref cd) = listing.custom_data {
        if let Some(ref title) = cd.title {
            let lower = title.to_lowercase();
            return NON_ENGLISH_KEYWORDS
                .iter()
                .any(|kw| lower.contains(kw));
        }
    }
    false
}

/// Find the lowest-priced listing that is actually English.
fn find_lowest_english(listings: &[Listing]) -> Option<&Listing> {
    listings
        .iter()
        .filter(|l| !is_non_english_listing(l))
        .min_by(|a, b| {
            let a_total = a.price + a.shipping_price.unwrap_or(0.0);
            let b_total = b.price + b.shipping_price.unwrap_or(0.0);
            a_total
                .partial_cmp(&b_total)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

fn upsert_card(conn: &Connection, r: &tcg_scanner::api::SearchResult, now: &str) {
    let (card_type, domain, energy_cost, power_cost, might, tag, card_number) =
        if let Some(ref attrs) = r.custom_attributes {
            (
                attrs.get("cardType").and_then(|v| {
                    if let Some(arr) = v.as_array() {
                        Some(
                            arr.iter()
                                .filter_map(|x| x.as_str())
                                .collect::<Vec<_>>()
                                .join(", "),
                        )
                    } else {
                        v.as_str().map(String::from)
                    }
                }),
                attrs.get("domain").and_then(|v| v.as_str()).map(String::from),
                attrs.get("energyCost").and_then(|v| v.as_str()).map(String::from),
                attrs.get("powerCost").and_then(|v| v.as_str()).map(String::from),
                attrs.get("might").and_then(|v| v.as_str()).map(String::from),
                attrs.get("tag").and_then(|v| v.as_str()).map(String::from),
                attrs.get("number").and_then(|v| v.as_str()).map(String::from),
            )
        } else {
            (None, None, None, None, None, None, None)
        };

    conn.execute(
        "INSERT INTO cards (product_id, product_name, clean_name, set_name, product_line,
                            rarity, card_number, card_type, domain, energy_cost, power_cost,
                            might, tag, foil_only, normal_only, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
         ON CONFLICT(product_id) DO UPDATE SET
            product_name = excluded.product_name,
            clean_name = excluded.clean_name,
            rarity = excluded.rarity,
            card_number = excluded.card_number,
            card_type = excluded.card_type,
            domain = excluded.domain,
            energy_cost = excluded.energy_cost,
            power_cost = excluded.power_cost,
            might = excluded.might,
            tag = excluded.tag,
            foil_only = excluded.foil_only,
            normal_only = excluded.normal_only,
            updated_at = excluded.updated_at",
        rusqlite::params![
            r.product_id,
            r.product_name,
            r.clean_name,
            r.set_name,
            r.product_line_name,
            r.rarity_name,
            card_number,
            card_type,
            domain,
            energy_cost,
            power_cost,
            might,
            tag,
            r.foil_only.unwrap_or(false) as i32,
            r.normal_only.unwrap_or(false) as i32,
            now,
        ],
    )
    .expect("Failed to upsert card");
}

fn insert_snapshot(
    conn: &Connection,
    r: &tcg_scanner::api::SearchResult,
    now: &str,
    lowest_eng: Option<(&Listing, f64)>,
) {
    let (eng_price, eng_with_ship, eng_seller) = match lowest_eng {
        Some((listing, total)) => (
            Some(listing.price),
            Some(total),
            Some(listing.seller_name.clone()),
        ),
        None => (None, None, None),
    };

    conn.execute(
        "INSERT INTO price_snapshots (product_id, captured_at,
            tcg_market_price, tcg_lowest_price, tcg_median_price, tcg_lowest_with_shipping,
            total_listings, lowest_english_price, lowest_english_with_ship, lowest_english_seller)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            r.product_id,
            now,
            r.market_price,
            r.lowest_price,
            r.median_price,
            r.lowest_price_with_shipping,
            r.total_listings,
            eng_price,
            eng_with_ship,
            eng_seller,
        ],
    )
    .expect("Failed to insert price snapshot");
}

fn insert_price_point(
    conn: &Connection,
    product_id: u64,
    now: &str,
    pp: &tcg_scanner::PricePoint,
) {
    conn.execute(
        "INSERT INTO price_points (product_id, captured_at, printing_type, market_price,
                                    buylist_price, listed_median)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            product_id,
            now,
            pp.printing_type,
            pp.market_price,
            pp.buylist_market_price,
            pp.listed_median_price,
        ],
    )
    .expect("Failed to insert price point");
}

fn insert_sale(conn: &Connection, product_id: u64, sale: &tcg_scanner::Sale) -> bool {
    let result = conn.execute(
        "INSERT OR IGNORE INTO sales (product_id, order_date, purchase_price, shipping_price,
                                       condition, variant, language, quantity, listing_type)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![
            product_id,
            sale.order_date,
            sale.purchase_price,
            sale.shipping_price,
            sale.condition,
            sale.variant,
            sale.language,
            sale.quantity,
            sale.listing_type,
        ],
    );
    match result {
        Ok(changes) => changes > 0,
        Err(_) => false,
    }
}

/// Compute the 2-day average sale price for each card and update the snapshot.
fn compute_2day_averages(conn: &Connection, now: &str) {
    // Calculate the cutoff time (48 hours ago)
    let cutoff = chrono::DateTime::parse_from_rfc3339(now)
        .map(|dt| (dt - chrono::Duration::hours(48)).to_rfc3339())
        .unwrap_or_else(|_| now.to_string());

    conn.execute_batch(&format!(
        "
        UPDATE price_snapshots
        SET avg_2day_sale_price = sub.avg_price,
            sales_2day_count = sub.sale_count
        FROM (
            SELECT
                s.product_id,
                AVG(s.purchase_price) as avg_price,
                COUNT(*) as sale_count
            FROM sales s
            WHERE s.order_date >= '{cutoff}'
            GROUP BY s.product_id
        ) sub
        WHERE price_snapshots.product_id = sub.product_id
          AND price_snapshots.captured_at = '{now}'
        "
    ))
    .expect("Failed to compute 2-day averages");
}

async fn retry_with_backoff<F, Fut, T>(label: &str, f: F) -> std::result::Result<T, String>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = tcg_scanner::Result<T>>,
{
    for attempt in 0..MAX_RETRIES {
        match f().await {
            Ok(val) => return Ok(val),
            Err(e) => {
                let err_str = e.to_string();
                if attempt + 1 < MAX_RETRIES {
                    let delay = RETRY_BACKOFF_MS * (attempt as u64 + 1);
                    eprintln!(
                        "  Retry {}/{} for {} (waiting {}s): {}",
                        attempt + 1,
                        MAX_RETRIES,
                        label,
                        delay / 1000,
                        err_str
                    );
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                } else {
                    return Err(err_str);
                }
            }
        }
    }
    unreachable!()
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let db_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("spiritforged_prices.db"));

    println!("Database: {}", db_path.display());

    let conn = Connection::open(&db_path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
    init_db(&conn);

    let client = TcgClient::new()?;
    let now = Utc::now().to_rfc3339();

    // --- Phase 1: Discover all cards in the set ---
    println!("\n=== Phase 1: Discovering cards in Spiritforged set ===");

    let filters = SearchTermFilters {
        product_line_name: Some(vec![
            "Riftbound: League of Legends Trading Card Game".into(),
        ]),
        set_name: Some(vec!["Spiritforged".into()]),
        product_type_name: Some(vec!["Cards".into()]),
        ..Default::default()
    };

    let mut all_cards = Vec::new();
    let mut from = 0u32;

    loop {
        let (results, total) = client.search_filtered("", from, PAGE_SIZE, &filters).await?;
        let count = results.len() as u32;
        all_cards.extend(results);
        println!(
            "  Fetched cards {}-{} of {}",
            from + 1,
            from + count,
            total
        );

        from += count;
        if from >= total || count == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    println!("Found {} cards total\n", all_cards.len());

    // --- Phase 2: Store cards, fetch lowest English listing, and store snapshots ---
    println!("=== Phase 2: Storing cards and fetching lowest English listings ===");

    let mut eng_found = 0u32;
    let mut eng_filtered = 0u32;
    let mut listing_errors = 0u32;

    for (i, card) in all_cards.iter().enumerate() {
        upsert_card(&conn, card, &now);

        // Fetch cheapest listings to find lowest English price
        let lowest_eng = match retry_with_backoff(&card.product_name, || {
            client.get_product_listings(card.product_id, 0, LISTING_FETCH_SIZE)
        })
        .await
        {
            Ok((listings, _total)) => {
                let non_eng_count = listings.iter().filter(|l| is_non_english_listing(l)).count();
                if non_eng_count > 0 {
                    eng_filtered += non_eng_count as u32;
                }
                find_lowest_english(&listings).map(|l| {
                    let total = l.price + l.shipping_price.unwrap_or(0.0);
                    (l.clone(), total)
                })
            }
            Err(e) => {
                eprintln!(
                    "  FAILED: listings for {} ({}): {}",
                    card.product_name, card.product_id, e
                );
                listing_errors += 1;
                None
            }
        };

        if lowest_eng.is_some() {
            eng_found += 1;
        }

        // Store snapshot with lowest English price
        insert_snapshot(
            &conn,
            card,
            &now,
            lowest_eng.as_ref().map(|(l, t)| (l, *t)),
        );

        if (i + 1) % 50 == 0 {
            println!(
                "  Listings: {}/{} cards processed ({} English prices found)",
                i + 1,
                all_cards.len(),
                eng_found
            );
        }
        tokio::time::sleep(Duration::from_millis(REQUEST_DELAY_MS)).await;
    }
    println!(
        "  Stored {} snapshots ({} with English prices, {} non-English listings filtered, {} errors)",
        all_cards.len(),
        eng_found,
        eng_filtered,
        listing_errors
    );

    // --- Phase 3: Fetch price points ---
    println!("\n=== Phase 3: Fetching price points ===");

    let mut pp_count = 0u32;
    let mut pp_errors = 0u32;
    for (i, card) in all_cards.iter().enumerate() {
        let pid = card.product_id;
        match retry_with_backoff(&card.product_name, || client.get_price_points(pid)).await {
            Ok(points) => {
                for pp in &points {
                    insert_price_point(&conn, pid, &now, pp);
                    pp_count += 1;
                }
            }
            Err(e) => {
                eprintln!(
                    "  FAILED: price points for {} ({}): {}",
                    card.product_name, pid, e
                );
                pp_errors += 1;
            }
        }

        if (i + 1) % 50 == 0 {
            println!(
                "  Price points: {}/{} cards processed",
                i + 1,
                all_cards.len()
            );
        }
        tokio::time::sleep(Duration::from_millis(REQUEST_DELAY_MS)).await;
    }
    println!(
        "  Stored {} price point records ({} errors)",
        pp_count, pp_errors
    );

    // --- Phase 4: Fetch latest sales ---
    println!("\n=== Phase 4: Fetching latest sales ===");

    let mut total_processed = 0u32;
    let mut new_sales = 0u32;
    let mut sales_errors = 0u32;

    for (i, card) in all_cards.iter().enumerate() {
        let pid = card.product_id;
        match retry_with_backoff(&card.product_name, || {
            client.get_latest_sales(pid, Some(25))
        })
        .await
        {
            Ok(sales) => {
                total_processed += sales.len() as u32;
                for sale in &sales {
                    if insert_sale(&conn, pid, sale) {
                        new_sales += 1;
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "  FAILED: sales for {} ({}): {}",
                    card.product_name, pid, e
                );
                sales_errors += 1;
            }
        }

        if (i + 1) % 50 == 0 {
            println!(
                "  Sales: {}/{} cards processed ({} new sales so far)",
                i + 1,
                all_cards.len(),
                new_sales
            );
        }
        tokio::time::sleep(Duration::from_millis(REQUEST_DELAY_MS)).await;
    }
    println!(
        "  Processed {} sale records ({} new, {} errors)",
        total_processed, new_sales, sales_errors
    );

    // --- Phase 5: Compute 2-day average sale prices ---
    println!("\n=== Phase 5: Computing 2-day average sale prices ===");
    compute_2day_averages(&conn, &now);

    let avg_count: u32 = conn
        .query_row(
            "SELECT COUNT(*) FROM price_snapshots WHERE captured_at = ?1 AND avg_2day_sale_price IS NOT NULL",
            [&now],
            |r| r.get(0),
        )
        .unwrap_or(0);
    println!(
        "  Computed 2-day sale averages for {} cards (with recent sales)",
        avg_count
    );

    // --- Summary ---
    let card_count: u32 = conn.query_row("SELECT COUNT(*) FROM cards", [], |r| r.get(0))?;
    let snapshot_count: u32 =
        conn.query_row("SELECT COUNT(*) FROM price_snapshots", [], |r| r.get(0))?;
    let pp_total: u32 = conn.query_row("SELECT COUNT(*) FROM price_points", [], |r| r.get(0))?;
    let sale_total: u32 = conn.query_row("SELECT COUNT(*) FROM sales", [], |r| r.get(0))?;

    println!("\n=== Collection Complete ===");
    println!("  Cards in DB:          {}", card_count);
    println!("  Price snapshots:      {}", snapshot_count);
    println!("  Price point records:  {}", pp_total);
    println!("  Total sale records:   {}", sale_total);

    // Show a price comparison for a few high-value cards
    println!("\n=== Price Comparison (top cards by English listing) ===");
    println!(
        "  {:<45} {:>10} {:>10} {:>10}",
        "Card", "TCG Mkt", "Eng Low", "2d Avg"
    );
    println!("  {}", "-".repeat(77));

    let mut stmt = conn.prepare(
        "SELECT c.product_name,
                p.tcg_market_price,
                p.lowest_english_price,
                p.avg_2day_sale_price
         FROM price_snapshots p
         JOIN cards c ON c.product_id = p.product_id
         WHERE p.captured_at = ?1
           AND p.lowest_english_price IS NOT NULL
         ORDER BY p.lowest_english_price DESC
         LIMIT 15",
    )?;

    let rows = stmt.query_map([&now], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<f64>>(1)?,
            row.get::<_, Option<f64>>(2)?,
            row.get::<_, Option<f64>>(3)?,
        ))
    })?;

    for row in rows {
        let (name, tcg, eng, avg2d) = row?;
        let name_trunc = if name.len() > 44 {
            format!("{}…", &name[..43])
        } else {
            name
        };
        println!(
            "  {:<45} {:>10} {:>10} {:>10}",
            name_trunc,
            tcg.map(|p| format!("${:.2}", p))
                .unwrap_or_else(|| "N/A".into()),
            eng.map(|p| format!("${:.2}", p))
                .unwrap_or_else(|| "N/A".into()),
            avg2d
                .map(|p| format!("${:.2}", p))
                .unwrap_or_else(|| "N/A".into()),
        );
    }

    println!("\nDatabase saved to: {}", db_path.display());
    println!("\nPricing columns in price_snapshots:");
    println!("  tcg_market_price        - TCGPlayer's algorithmic price (often inaccurate)");
    println!("  lowest_english_price    - Cheapest listing confirmed as English");
    println!("  lowest_english_with_ship - Same, including shipping cost");
    println!("  avg_2day_sale_price     - Average of actual sales in last 48 hours");

    Ok(())
}
