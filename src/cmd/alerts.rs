use std::time::Duration;

use tcg_scanner::api::{HistoryRange, SearchTermFilters, SkuPriceHistory};
use tcg_scanner::TcgClient;

use super::common::{load_profile, retry_with_backoff};

const PAGE_SIZE: u32 = 50;
const REQUEST_DELAY_MS: u64 = 350;
const MAX_RETRIES: u32 = 3;
const RETRY_BACKOFF_MS: u64 = 5000;

struct VolumeAlert {
    product_name: String,
    rarity: String,
    avg_daily_qty: f64,
    today_qty: u32,
    yesterday_qty: u32,
    spike_ratio: f64,
    today_low: f64,
    today_high: f64,
}

fn detect_spikes(
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

pub async fn run(
    profile_name: &str,
    threshold: f64,
    range: HistoryRange,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let profile = load_profile(profile_name);

    println!("=== TCG Volume Spike Detector ===");
    println!(
        "Profile: {} ({} / {})",
        profile.name, profile.product_line, profile.set_name
    );
    println!("Threshold: {:.1}x average daily volume", threshold);
    println!(
        "Range: {} (for computing daily average)",
        match range {
            HistoryRange::Month => "1 month",
            HistoryRange::Quarter => "3 months",
            HistoryRange::SemiAnnual => "6 months",
            HistoryRange::Annual => "1 year",
        }
    );

    let client = TcgClient::new()?;

    // Discover all cards
    println!("\nDiscovering cards in {} set...", profile.set_name);
    let filters = SearchTermFilters {
        product_line_name: Some(vec![profile.product_line.clone()]),
        set_name: Some(vec![profile.set_name.clone()]),
        product_type_name: Some(vec![profile.product_type.clone()]),
        ..Default::default()
    };

    let mut all_cards = Vec::new();
    let mut from = 0u32;
    loop {
        let (results, total) = client.search_filtered("", from, PAGE_SIZE, &filters).await?;
        let count = results.len() as u32;
        all_cards.extend(results);
        from += count;
        if from >= total || count == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    println!(
        "Found {} cards. Scanning for volume spikes...\n",
        all_cards.len()
    );

    // Scan each card for volume spikes
    let mut all_alerts: Vec<VolumeAlert> = Vec::new();
    let mut errors = 0u32;

    for (i, card) in all_cards.iter().enumerate() {
        let pid = card.product_id;
        let name = &card.product_name;
        let rarity = card.rarity_name.as_deref().unwrap_or("?");

        match retry_with_backoff(name, MAX_RETRIES, RETRY_BACKOFF_MS, || {
            client.get_detailed_price_history(pid, range)
        })
        .await
        {
            Ok(history) => {
                let mut spikes = detect_spikes(name, rarity, &history.result, threshold);
                all_alerts.append(&mut spikes);
            }
            Err(e) => {
                eprintln!("  FAILED: {} ({}): {}", name, pid, e);
                errors += 1;
            }
        }

        if (i + 1) % 50 == 0 {
            println!(
                "  Scanned {}/{} cards ({} alerts so far)",
                i + 1,
                all_cards.len(),
                all_alerts.len()
            );
        }
        tokio::time::sleep(Duration::from_millis(REQUEST_DELAY_MS)).await;
    }

    // Sort by spike ratio
    all_alerts.sort_by(|a, b| {
        b.spike_ratio
            .partial_cmp(&a.spike_ratio)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Print results
    println!("\n{}", "=".repeat(60));
    if all_alerts.is_empty() {
        println!(
            "\nNo volume spikes detected above {:.1}x threshold.",
            threshold
        );
    } else {
        println!(
            "\n{} VOLUME SPIKE ALERTS (>= {:.1}x avg daily volume)\n",
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

            let name = if alert.product_name.len() > 38 {
                format!("{}...", &alert.product_name[..37])
            } else {
                alert.product_name.clone()
            };

            println!(
                "{} {:<40} {:>8} {:>6} {:>6} {:>7.1} {:>6.1}x {:>10} {:>10}",
                spike_marker,
                name,
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

    println!("\nScanned {} cards ({} errors)", all_cards.len(), errors);

    Ok(())
}
