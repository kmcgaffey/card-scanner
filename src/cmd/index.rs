use std::collections::HashMap;
use std::path::PathBuf;

use rusqlite::Connection;

use super::common::load_profile;

/// A single rarity tier within a set index.
struct RarityBucket {
    rarity: String,
    cards: u32,
    total_value: f64,
    avg_value: f64,
}

/// A set-level index result.
struct SetIndex {
    set_name: String,
    captured_at: String,
    total_cards: u32,
    total_value: f64,
    buckets: Vec<RarityBucket>,
}

/// Signature cards are excluded by default — detect them by name pattern.
fn is_signature(product_name: &str) -> bool {
    product_name.contains("(Signature)")
}

/// Compute the index for a single profile's database.
fn compute_set_index(profile_name: &str) -> std::result::Result<SetIndex, Box<dyn std::error::Error>> {
    let profile = load_profile(profile_name);
    let conn = Connection::open(&profile.db_path)?;

    // Get the latest snapshot timestamp
    let captured_at: String = conn.query_row(
        "SELECT MAX(captured_at) FROM price_snapshots",
        [],
        |r| r.get(0),
    )?;

    // Query all cards with their latest English price, excluding Signatures
    let mut stmt = conn.prepare(
        "SELECT c.product_name, c.rarity, p.lowest_english_price
         FROM price_snapshots p
         JOIN cards c ON c.product_id = p.product_id
         WHERE p.captured_at = ?1
           AND p.lowest_english_price IS NOT NULL",
    )?;

    let mut rarity_map: HashMap<String, (u32, f64)> = HashMap::new();

    let rows = stmt.query_map([&captured_at], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, f64>(2)?,
        ))
    })?;

    for row in rows {
        let (name, rarity, price) = row?;
        if is_signature(&name) {
            continue;
        }
        let entry = rarity_map.entry(rarity).or_insert((0, 0.0));
        entry.0 += 1;
        entry.1 += price;
    }

    let mut buckets: Vec<RarityBucket> = rarity_map
        .into_iter()
        .map(|(rarity, (cards, total))| RarityBucket {
            rarity,
            cards,
            total_value: total,
            avg_value: if cards > 0 { total / cards as f64 } else { 0.0 },
        })
        .collect();

    // Sort by total value descending
    buckets.sort_by(|a, b| {
        b.total_value
            .partial_cmp(&a.total_value)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let total_cards: u32 = buckets.iter().map(|b| b.cards).sum();
    let total_value: f64 = buckets.iter().map(|b| b.total_value).sum();

    Ok(SetIndex {
        set_name: profile.set_name,
        captured_at,
        total_cards,
        total_value,
        buckets,
    })
}

fn print_set_index(idx: &SetIndex) {
    println!(
        "\n{} Index: ${:.2}  ({} cards, excl. Signatures)",
        idx.set_name, idx.total_value, idx.total_cards
    );
    println!("  Snapshot: {}", idx.captured_at);
    println!(
        "  {:<18} {:>6} {:>12} {:>10}",
        "Rarity", "Cards", "Total", "Avg/Card"
    );
    println!("  {}", "-".repeat(50));
    for b in &idx.buckets {
        println!(
            "  {:<18} {:>6} {:>12} {:>10}",
            b.rarity,
            b.cards,
            format!("${:.2}", b.total_value),
            format!("${:.2}", b.avg_value),
        );
    }
}

pub fn run(profiles: &[String]) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let profile_names: Vec<String> = if profiles.is_empty() {
        // Load all profiles from profiles.toml
        let config_path = PathBuf::from("profiles.toml");
        let content = std::fs::read_to_string(&config_path)?;
        let table: HashMap<String, toml::Value> = toml::from_str(&content)?;
        table.keys().cloned().collect()
    } else {
        profiles.to_vec()
    };

    println!("=== TCG Price Index ===");
    println!("(Signature cards excluded as outliers)");

    let mut indices: Vec<SetIndex> = Vec::new();

    for name in &profile_names {
        match compute_set_index(name) {
            Ok(idx) => {
                print_set_index(&idx);
                indices.push(idx);
            }
            Err(e) => {
                eprintln!("\nFailed to compute index for '{}': {}", name, e);
            }
        }
    }

    // Combined index if multiple sets
    if indices.len() > 1 {
        let combined_cards: u32 = indices.iter().map(|i| i.total_cards).sum();
        let combined_value: f64 = indices.iter().map(|i| i.total_value).sum();

        // Merge rarity buckets across sets
        let mut combined_map: HashMap<String, (u32, f64)> = HashMap::new();
        for idx in &indices {
            for b in &idx.buckets {
                let entry = combined_map
                    .entry(b.rarity.clone())
                    .or_insert((0, 0.0));
                entry.0 += b.cards;
                entry.1 += b.total_value;
            }
        }

        let mut combined_buckets: Vec<RarityBucket> = combined_map
            .into_iter()
            .map(|(rarity, (cards, total))| RarityBucket {
                rarity,
                cards,
                total_value: total,
                avg_value: if cards > 0 { total / cards as f64 } else { 0.0 },
            })
            .collect();
        combined_buckets.sort_by(|a, b| {
            b.total_value
                .partial_cmp(&a.total_value)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        println!(
            "\n{}\nRiftbound Combined Index: ${:.2}  ({} cards across {} sets, excl. Signatures)",
            "=".repeat(60),
            combined_value,
            combined_cards,
            indices.len()
        );
        println!(
            "  {:<18} {:>6} {:>12} {:>10}",
            "Rarity", "Cards", "Total", "Avg/Card"
        );
        println!("  {}", "-".repeat(50));
        for b in &combined_buckets {
            println!(
                "  {:<18} {:>6} {:>12} {:>10}",
                b.rarity,
                b.cards,
                format!("${:.2}", b.total_value),
                format!("${:.2}", b.avg_value),
            );
        }

        println!("\n  Per-set breakdown:");
        for idx in &indices {
            println!(
                "    {:<20} ${:>10.2}  ({} cards)",
                idx.set_name, idx.total_value, idx.total_cards
            );
        }
    }

    Ok(())
}
