use std::collections::HashMap;
use std::time::Duration;

use chrono::Utc;
use rusqlite::Connection;
use tcg_scanner::api::{DetailedPriceHistory, HistoryRange, SearchResult, SearchTermFilters, SkuPriceHistory};
use tcg_scanner::TcgClient;

use super::common::{init_db, load_profile, retry_with_backoff, Profile};

const PAGE_SIZE: u32 = 50;
const REQUEST_DELAY_MS: u64 = 350;
const MAX_RETRIES: u32 = 3;
const RETRY_BACKOFF_MS: u64 = 5000;

/// How recent the DB data must be to skip the API (in hours).
const FRESHNESS_HOURS: i64 = 12;

/// Top N cards per rarity to show.
const TOP_N: usize = 5;

pub(crate) struct VolumeAlert {
    pub(crate) product_id: u64,
    pub(crate) product_name: String,
    pub(crate) rarity: String,
    pub(crate) avg_daily_qty: f64,
    pub(crate) today_qty: u32,
    pub(crate) yesterday_qty: u32,
    pub(crate) spike_ratio: f64,
    pub(crate) today_low: f64,
    pub(crate) today_high: f64,
}

/// Unified entry for graphic generation from either data source.
pub(crate) struct CardEntry {
    pub(crate) product_id: u64,
    pub(crate) product_name: String,
    /// Display name for the graphic (may include card number for promos).
    pub(crate) display_name: String,
    pub(crate) total_qty: u32,
    pub(crate) rarity: String,
    /// Fallback product_id for image lookup (e.g. base card for alt art variants).
    pub(crate) fallback_product_id: Option<u64>,
}

/// Card selection for the graphic: top overall + top per rarity.
pub(crate) struct GraphicCards {
    pub(crate) top_overall: Vec<CardEntry>,
    pub(crate) top_by_rarity: Vec<CardEntry>,
}

/// Variant suffixes that indicate an alternate version of a base card.
const VARIANT_SUFFIXES: &[&str] = &[
    " (Alternate Art)",
    " (Overnumbered)",
    " (Signature)",
];

/// Strip variant suffix from a card name to get the base card name.
fn base_card_name(name: &str) -> Option<&str> {
    for suffix in VARIANT_SUFFIXES {
        if let Some(stripped) = name.strip_suffix(suffix) {
            return Some(stripped);
        }
    }
    None
}

/// Look up a fallback product_id for image lookup from any of the provided DB connections.
///
/// Tries two strategies:
/// 1. Strip variant suffixes (e.g. "(Alternate Art)") and look up the base card name
/// 2. Find a same-named card in a different set (for promos that share names with regular cards)
fn lookup_fallback_product_id(conns: &[&Connection], product_id: u64, card_name: &str) -> Option<u64> {
    // Strategy 1: strip variant suffix
    if let Some(base_name) = base_card_name(card_name) {
        for conn in conns {
            let result: Option<i64> = conn
                .query_row(
                    "SELECT product_id FROM cards WHERE product_name = ?1 LIMIT 1",
                    [base_name],
                    |row| row.get(0),
                )
                .ok();
            if let Some(pid) = result {
                return Some(pid as u64);
            }
        }
    }

    // Strategy 2: same name, different product_id (promo → regular set)
    for conn in conns {
        let result: Option<i64> = conn
            .query_row(
                "SELECT product_id FROM cards WHERE product_name = ?1 AND product_id != ?2 LIMIT 1",
                rusqlite::params![card_name, product_id as i64],
                |row| row.get(0),
            )
            .ok();
        if let Some(pid) = result {
            return Some(pid as u64);
        }
    }

    None
}

/// Look up a card's number from any of the provided DB connections.
fn lookup_card_number(conns: &[&Connection], product_id: u64) -> Option<String> {
    for conn in conns {
        let result: Option<String> = conn
            .query_row(
                "SELECT card_number FROM cards WHERE product_id = ?1",
                [product_id as i64],
                |row| row.get(0),
            )
            .ok()?;
        if result.is_some() {
            return result;
        }
    }
    None
}

/// A row from the SQLite top-purchased query.
struct TopPurchased {
    product_id: u64,
    product_name: String,
    set_name: String,
    rarity: String,
    total_qty: u32,
    txn_count: u32,
    avg_price: f64,
    low_price: f64,
    high_price: f64,
}

fn detect_spikes(
    product_id: u64,
    product_name: &str,
    rarity: &str,
    history: &[SkuPriceHistory],
    threshold: f64,
) -> Vec<VolumeAlert> {
    let mut alerts = Vec::new();

    for sku in history {
        let avg_daily = sku.avg_daily_qty();
        if avg_daily <= 0.0 || sku.buckets.is_empty() {
            continue;
        }

        let recent = &sku.buckets[0];
        let prior = sku.buckets.get(1);

        let today_qty = recent.qty_sold();
        let yesterday_qty = prior.map(|b| b.qty_sold()).unwrap_or(0);

        let today_ratio = today_qty as f64 / avg_daily;
        let yesterday_ratio = yesterday_qty as f64 / avg_daily;
        let best_ratio = today_ratio.max(yesterday_ratio);

        if best_ratio >= threshold && (today_qty >= 3 || yesterday_qty >= 3) {
            alerts.push(VolumeAlert {
                product_id,
                product_name: product_name.to_string(),
                rarity: rarity.to_string(),
                avg_daily_qty: avg_daily,
                today_qty,
                yesterday_qty,
                spike_ratio: best_ratio,
                today_low: recent.low_price(),
                today_high: recent.high_price(),
            });
        }
    }

    alerts
}

/// Upsert card metadata from a search result into the DB.
fn upsert_card(conn: &Connection, card: &SearchResult, profile: &Profile, now: &str) {
    conn.execute(
        "INSERT INTO cards (product_id, product_name, clean_name, set_name, product_line, rarity, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(product_id) DO UPDATE SET
             product_name = excluded.product_name,
             clean_name = excluded.clean_name,
             set_name = excluded.set_name,
             product_line = excluded.product_line,
             rarity = excluded.rarity,
             updated_at = excluded.updated_at",
        rusqlite::params![
            card.product_id as i64,
            card.product_name,
            card.clean_name,
            profile.set_name,
            profile.product_line,
            card.rarity_name,
            now,
        ],
    )
    .ok();
}

/// Insert a price snapshot from search result data.
fn insert_snapshot_from_search(conn: &Connection, card: &SearchResult, now: &str) {
    conn.execute(
        "INSERT INTO price_snapshots (product_id, captured_at, tcg_market_price, tcg_lowest_price, tcg_median_price, tcg_lowest_with_shipping, total_listings)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            card.product_id as i64,
            now,
            card.market_price,
            card.lowest_price,
            card.median_price,
            card.lowest_price_with_shipping,
            card.total_listings,
        ],
    )
    .ok();
}

/// Insert daily volume data from price history into the daily_volume table.
fn insert_daily_volume(
    conn: &Connection,
    product_id: u64,
    history: &DetailedPriceHistory,
) {
    for sku in &history.result {
        for bucket in &sku.buckets {
            conn.execute(
                "INSERT OR REPLACE INTO daily_volume
                 (product_id, bucket_date, variant, condition, quantity_sold, market_price, low_price, high_price)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    product_id as i64,
                    &bucket.bucket_start_date,
                    &sku.variant,
                    &sku.condition,
                    bucket.qty_sold(),
                    bucket.market_price_f64(),
                    bucket.low_price(),
                    bucket.high_price(),
                ],
            )
            .ok();
        }
    }
}

/// Check if a profile's DB has data collected within the freshness window.
fn db_is_fresh(profile: &Profile) -> Option<(Connection, String)> {
    if !profile.db_path.exists() {
        return None;
    }

    let conn = Connection::open(&profile.db_path).ok()?;

    let latest: Option<String> = conn
        .query_row(
            "SELECT MAX(captured_at) FROM price_snapshots",
            [],
            |r| r.get(0),
        )
        .ok()?;

    let latest = latest?;

    let captured = chrono::DateTime::parse_from_rfc3339(&latest).ok()?;
    let age = Utc::now().signed_duration_since(captured);

    if age.num_hours() < FRESHNESS_HOURS {
        Some((conn, latest))
    } else {
        None
    }
}

/// Query top purchased cards by rarity from daily volume data.
/// Falls back to the sales table if daily_volume is empty (older DBs).
fn query_top_purchased(conn: &Connection, hours: i64) -> Vec<TopPurchased> {
    let cutoff = (Utc::now() - chrono::Duration::hours(hours)).to_rfc3339();

    // Check if daily_volume table exists and has data
    let has_volume: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='daily_volume'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(false);

    if has_volume {
        let vol_count: u32 = conn
            .query_row("SELECT COUNT(*) FROM daily_volume", [], |r| r.get(0))
            .unwrap_or(0);

        if vol_count > 0 {
            return query_top_from_volume(conn, &cutoff);
        }
    }

    // Fall back to sales table for older DBs without daily_volume
    query_top_from_sales(conn, &cutoff)
}

/// Query top cards using the daily_volume table (accurate aggregate data).
fn query_top_from_volume(conn: &Connection, cutoff: &str) -> Vec<TopPurchased> {
    let mut stmt = conn
        .prepare(
            "SELECT v.product_id, c.product_name, c.set_name, c.rarity,
                    SUM(v.quantity_sold) as total_qty,
                    SUM(v.quantity_sold) as txn_count,
                    ROUND(AVG(v.market_price), 2) as avg_price,
                    ROUND(MIN(v.low_price), 2) as low_price,
                    ROUND(MAX(v.high_price), 2) as high_price
             FROM daily_volume v
             JOIN cards c ON c.product_id = v.product_id
             WHERE v.bucket_date >= ?1
             GROUP BY v.product_id
             ORDER BY c.rarity, total_qty DESC",
        )
        .expect("Failed to prepare volume query");

    let rows = stmt
        .query_map([cutoff], |row| {
            Ok(TopPurchased {
                product_id: row.get::<_, i64>(0)? as u64,
                product_name: row.get(1)?,
                set_name: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                rarity: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                total_qty: row.get(4)?,
                txn_count: row.get(5)?,
                avg_price: row.get(6)?,
                low_price: row.get(7)?,
                high_price: row.get(8)?,
            })
        })
        .expect("Failed to query volume");

    rows.filter_map(|r| r.ok()).collect()
}

/// Fallback: query top cards from individual sales records.
fn query_top_from_sales(conn: &Connection, cutoff: &str) -> Vec<TopPurchased> {
    let mut stmt = conn
        .prepare(
            "SELECT s.product_id, c.product_name, c.set_name, c.rarity,
                    SUM(s.quantity) as total_qty,
                    COUNT(*) as txn_count,
                    ROUND(AVG(s.purchase_price), 2) as avg_price,
                    ROUND(MIN(s.purchase_price), 2) as low_price,
                    ROUND(MAX(s.purchase_price), 2) as high_price
             FROM sales s
             JOIN cards c ON c.product_id = s.product_id
             WHERE s.order_date >= ?1
             GROUP BY s.product_id
             ORDER BY c.rarity, total_qty DESC",
        )
        .expect("Failed to prepare sales query");

    let rows = stmt
        .query_map([cutoff], |row| {
            Ok(TopPurchased {
                product_id: row.get::<_, i64>(0)? as u64,
                product_name: row.get(1)?,
                set_name: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                rarity: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                total_qty: row.get(4)?,
                txn_count: row.get(5)?,
                avg_price: row.get(6)?,
                low_price: row.get(7)?,
                high_price: row.get(8)?,
            })
        })
        .expect("Failed to query sales");

    rows.filter_map(|r| r.ok()).collect()
}

/// Print top N per rarity from a flat list (already sorted by rarity, total_qty desc).
fn print_top_by_rarity(rows: &[TopPurchased], top_n: usize) {
    // Rarity display order
    let rarity_order = ["Common", "Uncommon", "Rare", "Epic", "Showcase"];

    // Group by rarity
    let mut by_rarity: HashMap<&str, Vec<&TopPurchased>> = HashMap::new();
    for row in rows {
        by_rarity.entry(&row.rarity).or_default().push(row);
    }

    for rarity in &rarity_order {
        if let Some(cards) = by_rarity.get(rarity) {
            println!("\n  {} ({} cards with sales)", rarity, cards.len());
            println!(
                "    {:<45} {:>7} {:>5} {:>9} {:>9} {:>9} {}",
                "Card", "Copies", "Txns", "Avg", "Low", "High", "Set"
            );
            println!("    {}", "-".repeat(100));

            for card in cards.iter().take(top_n) {
                let name = if card.product_name.len() > 44 {
                    format!("{}...", &card.product_name[..41])
                } else {
                    card.product_name.clone()
                };
                let set = if card.set_name.len() > 15 {
                    format!("{}...", &card.set_name[..12])
                } else {
                    card.set_name.clone()
                };
                println!(
                    "    {:<45} {:>7} {:>5} {:>9} {:>9} {:>9} {}",
                    name,
                    card.total_qty,
                    card.txn_count,
                    format!("${:.2}", card.avg_price),
                    format!("${:.2}", card.low_price),
                    format!("${:.2}", card.high_price),
                    set,
                );
            }
        }
    }

    // Catch any rarities not in our display order
    for (rarity, cards) in &by_rarity {
        if !rarity_order.contains(rarity) && !cards.is_empty() {
            println!("\n  {} ({} cards with sales)", rarity, cards.len());
            println!(
                "    {:<45} {:>7} {:>5} {:>9} {:>9} {:>9}",
                "Card", "Copies", "Txns", "Avg", "Low", "High"
            );
            println!("    {}", "-".repeat(90));
            for card in cards.iter().take(top_n) {
                let name = if card.product_name.len() > 44 {
                    format!("{}...", &card.product_name[..41])
                } else {
                    card.product_name.clone()
                };
                println!(
                    "    {:<45} {:>7} {:>5} {:>9} {:>9} {:>9}",
                    name,
                    card.total_qty,
                    card.txn_count,
                    format!("${:.2}", card.avg_price),
                    format!("${:.2}", card.low_price),
                    format!("${:.2}", card.high_price),
                );
            }
        }
    }
}

/// Build a CardEntry from a TopPurchased row, with fallback image lookup and promo display name.
fn make_card_entry(row: &TopPurchased, conn_refs: &[&Connection]) -> CardEntry {
    let fallback = lookup_fallback_product_id(conn_refs, row.product_id, &row.product_name);

    // For promo cards, include the card number in the display name
    let display_name = if row.rarity == "Promo" {
        if let Some(num) = lookup_card_number(conn_refs, row.product_id) {
            format!("{} ({})", row.product_name, num)
        } else {
            row.product_name.clone()
        }
    } else {
        row.product_name.clone()
    };

    CardEntry {
        product_id: row.product_id,
        product_name: row.product_name.clone(),
        display_name,
        total_qty: row.total_qty,
        rarity: row.rarity.clone(),
        fallback_product_id: fallback,
    }
}

/// Build the card selections for the graphic: top 5 overall + top 1 per rarity.
fn build_graphic_cards(rows: &[TopPurchased], conn_refs: &[&Connection]) -> GraphicCards {
    // Top 5 overall by volume
    let mut sorted: Vec<&TopPurchased> = rows.iter().collect();
    sorted.sort_by(|a, b| b.total_qty.cmp(&a.total_qty));

    let top_overall: Vec<CardEntry> = sorted
        .iter()
        .take(5)
        .map(|r| make_card_entry(r, conn_refs))
        .collect();

    // Top 1 per rarity (skip cards already in top_overall)
    let rarity_order = ["Common", "Uncommon", "Rare", "Epic", "Showcase", "Promo"];
    let top_ids: Vec<u64> = top_overall.iter().map(|c| c.product_id).collect();

    let mut by_rarity: HashMap<&str, Vec<&TopPurchased>> = HashMap::new();
    for row in rows {
        by_rarity.entry(&row.rarity).or_default().push(row);
    }

    let mut top_by_rarity: Vec<CardEntry> = Vec::new();
    for rarity in &rarity_order {
        if let Some(cards) = by_rarity.get(rarity) {
            // Find the top card in this rarity that isn't already in top_overall
            if let Some(best) = cards.iter().max_by_key(|c| c.total_qty) {
                if !top_ids.contains(&best.product_id) {
                    top_by_rarity.push(make_card_entry(best, conn_refs));
                }
            }
        }
    }

    GraphicCards {
        top_overall,
        top_by_rarity,
    }
}

/// Run alerts for multiple profiles, using SQLite when fresh.
pub async fn run(
    profile_names: &[String],
    threshold: f64,
    range: HistoryRange,
    chart: bool,
    chart_output: &str,
    post: bool,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    println!("=== TCG Volume & Sales Report ===\n");

    // Collect fresh DB results and stale profiles separately
    let mut all_sqlite_rows: Vec<TopPurchased> = Vec::new();
    let mut stale_profiles: Vec<String> = Vec::new();
    let mut fresh_sets: Vec<String> = Vec::new();
    let mut fresh_conns: Vec<Connection> = Vec::new();

    for name in profile_names {
        let profile = load_profile(name);

        if let Some((conn, captured_at)) = db_is_fresh(&profile) {
            let age = {
                let captured = chrono::DateTime::parse_from_rfc3339(&captured_at).unwrap();
                let dur = Utc::now().signed_duration_since(captured);
                format!("{}h {}m ago", dur.num_hours(), dur.num_minutes() % 60)
            };
            println!(
                "  {} — using cached data (collected {})",
                profile.set_name, age
            );
            fresh_sets.push(profile.set_name.clone());

            let rows = query_top_purchased(&conn, 48);
            all_sqlite_rows.extend(rows);
            fresh_conns.push(conn);
        } else {
            println!(
                "  {} — no fresh data (>{} hours old or missing)",
                profile.set_name, FRESHNESS_HOURS
            );
            stale_profiles.push(name.clone());
        }
    }

    let mut graphic_generated = false;

    // Print SQLite-based results for fresh profiles
    if !all_sqlite_rows.is_empty() {
        println!(
            "\n=== Top {} Most Purchased Cards (last 48h) ===",
            TOP_N
        );
        println!("Sets: {}", fresh_sets.join(", "));
        print_top_by_rarity(&all_sqlite_rows, TOP_N);

        // Build GraphicCards: top 5 overall + top 1 per rarity
        if chart || post {
            let conn_refs: Vec<&Connection> = fresh_conns.iter().collect();
            let graphic_cards = build_graphic_cards(&all_sqlite_rows, &conn_refs);

            let end_date = Utc::now().format("%b %d");
            let start_date = (Utc::now() - chrono::Duration::hours(48)).format("%b %d");
            let title = format!("Riftbound Top Sellers — {} to {}", start_date, end_date);
            let path = std::path::PathBuf::from(chart_output);
            match super::graphic::generate_graphic(&graphic_cards, &title, &path).await {
                Ok(()) => {
                    println!("\n  Graphic saved to {}", path.display());
                    graphic_generated = true;
                }
                Err(e) => eprintln!("\n  Failed to generate graphic: {}", e),
            }

            if post && path.exists() {
                let tweet_text = format!(
                    "Top selling cards (last 48h): {}",
                    graphic_cards
                        .top_overall
                        .iter()
                        .map(|e| e.product_name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                match super::x_post::post_graphic(&path, &tweet_text).await {
                    Ok(url) => println!("  Posted to X: {}", url),
                    Err(e) => eprintln!("  Failed to post to X: {}", e),
                }
            }
        }
    }

    // Fall back to API-based spike detection for stale profiles
    if !stale_profiles.is_empty() {
        println!("\n=== API-Based Volume Spike Scan (stale profiles) ===");
        println!("Threshold: {:.1}x average daily volume", threshold);

        let client = TcgClient::new()?;

        for name in &stale_profiles {
            let profile = load_profile(name);
            println!(
                "\nScanning {} ({} / {})...",
                profile.name, profile.product_line, profile.set_name
            );

            // Open/create DB for this profile to persist data we fetch
            let conn = Connection::open(&profile.db_path)?;
            conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
            init_db(&conn);
            let now = Utc::now().to_rfc3339();

            let filters = SearchTermFilters {
                product_line_name: Some(vec![profile.product_line.clone()]),
                set_name: Some(vec![profile.set_name.clone()]),
                product_type_name: Some(vec![profile.product_type.clone()]),
                ..Default::default()
            };

            let mut all_cards = Vec::new();
            let mut from = 0u32;
            loop {
                let (results, total) =
                    client.search_filtered("", from, PAGE_SIZE, &filters).await?;
                let count = results.len() as u32;
                all_cards.extend(results);
                from += count;
                if from >= total || count == 0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            println!("  Found {} cards, scanning...", all_cards.len());

            // Persist card metadata and price snapshots from search results
            for card in &all_cards {
                upsert_card(&conn, card, &profile, &now);
                insert_snapshot_from_search(&conn, card, &now);
            }

            let mut all_alerts: Vec<VolumeAlert> = Vec::new();
            let mut errors = 0u32;
            let mut current_delay = REQUEST_DELAY_MS;

            for (i, card) in all_cards.iter().enumerate() {
                let pid = card.product_id;
                let card_name = &card.product_name;
                let rarity = card.rarity_name.as_deref().unwrap_or("?");

                match retry_with_backoff(card_name, MAX_RETRIES, RETRY_BACKOFF_MS, || {
                    client.get_detailed_price_history(pid, range)
                })
                .await
                {
                    Ok(history) => {
                        // Persist price history data to DB
                        insert_daily_volume(&conn, pid, &history);

                        let mut spikes =
                            detect_spikes(pid, card_name, rarity, &history.result, threshold);
                        all_alerts.append(&mut spikes);
                        // Gradually reduce delay back to normal after success
                        if current_delay > REQUEST_DELAY_MS {
                            current_delay = (current_delay * 3 / 4).max(REQUEST_DELAY_MS);
                        }
                    }
                    Err(e) => {
                        // Check if this was a rate limit error
                        let is_rate_limit = e.contains("Rate limited") || e.contains("HTTP 403") || e.contains("HTTP 429");
                        if is_rate_limit {
                            // Back off significantly on rate limits
                            current_delay = (current_delay * 2).min(5000);
                            eprintln!(
                                "  Rate limited on {} ({}), increasing delay to {}ms",
                                card_name, pid, current_delay
                            );
                        } else {
                            eprintln!("  FAILED: {} ({}): {}", card_name, pid, e);
                        }
                        errors += 1;
                    }
                }

                if (i + 1) % 50 == 0 {
                    println!(
                        "  Scanned {}/{} ({} alerts)",
                        i + 1,
                        all_cards.len(),
                        all_alerts.len()
                    );
                }
                tokio::time::sleep(Duration::from_millis(current_delay)).await;
            }

            all_alerts.sort_by(|a, b| {
                b.spike_ratio
                    .partial_cmp(&a.spike_ratio)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            if all_alerts.is_empty() {
                println!(
                    "  No volume spikes above {:.1}x threshold.",
                    threshold
                );
            } else {
                println!(
                    "\n  {} ALERTS (>= {:.1}x avg daily volume)\n",
                    all_alerts.len(),
                    threshold
                );
                println!(
                    "  {:<40} {:>8} {:>6} {:>6} {:>7} {:>7} {:>10} {:>10}",
                    "Card", "Rarity", "Today", "Yday", "AvgDay", "Spike", "Low", "High"
                );
                println!("  {}", "-".repeat(100));

                for alert in &all_alerts {
                    let spike_marker = if alert.spike_ratio >= 5.0 {
                        "!!"
                    } else if alert.spike_ratio >= 3.0 {
                        "! "
                    } else {
                        "  "
                    };
                    let aname = if alert.product_name.len() > 38 {
                        format!("{}...", &alert.product_name[..37])
                    } else {
                        alert.product_name.clone()
                    };
                    println!(
                        "{} {:<40} {:>8} {:>6} {:>6} {:>7.1} {:>6.1}x {:>10} {:>10}",
                        spike_marker,
                        aname,
                        alert.rarity,
                        alert.today_qty,
                        alert.yesterday_qty,
                        alert.avg_daily_qty,
                        alert.spike_ratio,
                        if alert.today_low > 0.0 {
                            format!("${:.2}", alert.today_low)
                        } else {
                            "N/A".into()
                        },
                        if alert.today_high > 0.0 {
                            format!("${:.2}", alert.today_high)
                        } else {
                            "N/A".into()
                        },
                    );
                }
            }

            let saved = all_cards.len() as u32 - errors;
            println!(
                "  Scanned {} cards ({} errors), saved {} to {}",
                all_cards.len(),
                errors,
                saved,
                profile.db_path.display()
            );

            // Make this conn available for fallback lookups
            fresh_conns.push(conn);

            if (chart || post) && !all_alerts.is_empty() && !graphic_generated {
                let conn_refs: Vec<&Connection> = fresh_conns.iter().collect();

                // Convert alerts to TopPurchased-like entries for build_graphic_cards
                let alert_rows: Vec<TopPurchased> = all_alerts
                    .iter()
                    .map(|a| TopPurchased {
                        product_id: a.product_id,
                        product_name: a.product_name.clone(),
                        set_name: String::new(),
                        rarity: a.rarity.clone(),
                        total_qty: a.today_qty.max(a.yesterday_qty),
                        txn_count: 0,
                        avg_price: 0.0,
                        low_price: a.today_low,
                        high_price: a.today_high,
                    })
                    .collect();

                let graphic_cards = build_graphic_cards(&alert_rows, &conn_refs);

                let end_date = Utc::now().format("%b %d");
                let start_date = (Utc::now() - chrono::Duration::hours(48)).format("%b %d");
                let title = format!("Volume Spikes — {} — {} to {}", profile.set_name, start_date, end_date);
                let path = std::path::PathBuf::from(chart_output);
                match super::graphic::generate_graphic(&graphic_cards, &title, &path).await {
                    Ok(()) => println!("\n  Graphic saved to {}", path.display()),
                    Err(e) => eprintln!("\n  Failed to generate graphic: {}", e),
                }

                if post && path.exists() {
                    let tweet_text = format!(
                        "Volume spike alert for {}: {}",
                        profile.set_name,
                        graphic_cards
                            .top_overall
                            .iter()
                            .map(|e| e.product_name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                    match super::x_post::post_graphic(&path, &tweet_text).await {
                        Ok(url) => println!("  Posted to X: {}", url),
                        Err(e) => eprintln!("  Failed to post to X: {}", e),
                    }
                }
            }
        }
    }

    Ok(())
}
