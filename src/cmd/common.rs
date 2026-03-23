use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use rusqlite::Connection;

/// A collection profile loaded from profiles.toml.
#[derive(Debug, Clone)]
pub struct Profile {
    pub name: String,
    pub product_line: String,
    pub set_name: String,
    pub product_type: String,
    pub db_path: PathBuf,
}

/// Load a named profile from profiles.toml.
pub fn load_profile(profile_name: &str) -> Profile {
    let config_path = PathBuf::from("profiles.toml");
    let content = std::fs::read_to_string(&config_path).unwrap_or_else(|e| {
        eprintln!("Failed to read {}: {}", config_path.display(), e);
        eprintln!("Create a profiles.toml file with your collection profiles.");
        std::process::exit(1);
    });

    let table: HashMap<String, toml::Value> = toml::from_str(&content).unwrap_or_else(|e| {
        eprintln!("Failed to parse {}: {}", config_path.display(), e);
        std::process::exit(1);
    });

    let profile_value = table.get(profile_name).unwrap_or_else(|| {
        let available: Vec<&String> = table.keys().collect();
        eprintln!("Profile '{}' not found in profiles.toml", profile_name);
        eprintln!(
            "Available profiles: {}",
            available
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        std::process::exit(1);
    });

    let t = profile_value.as_table().unwrap_or_else(|| {
        eprintln!("Profile '{}' is not a valid table", profile_name);
        std::process::exit(1);
    });

    let get_str = |key: &str| -> String {
        t.get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| {
                eprintln!(
                    "Profile '{}' missing required field '{}'",
                    profile_name, key
                );
                std::process::exit(1);
            })
            .to_string()
    };

    Profile {
        name: profile_name.to_string(),
        product_line: get_str("product_line"),
        set_name: get_str("set_name"),
        product_type: get_str("product_type"),
        db_path: PathBuf::from(get_str("db_path")),
    }
}

/// List all profiles from profiles.toml.
pub fn list_profiles() {
    let config_path = PathBuf::from("profiles.toml");
    let content = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(_) => {
            eprintln!("No profiles.toml found.");
            return;
        }
    };
    let table: HashMap<String, toml::Value> = match toml::from_str(&content) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Failed to parse profiles.toml: {}", e);
            return;
        }
    };

    println!("Available profiles:");
    for (name, value) in &table {
        if let Some(t) = value.as_table() {
            let set = t.get("set_name").and_then(|v| v.as_str()).unwrap_or("?");
            let line = t
                .get("product_line")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let db = t.get("db_path").and_then(|v| v.as_str()).unwrap_or("?");
            println!("  {:<20} {} / {} -> {}", name, line, set, db);
        }
    }
}

/// Initialize the database schema (shared between collect and alerts).
pub fn init_db(conn: &Connection) {
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
            tcg_market_price            REAL,
            tcg_lowest_price            REAL,
            tcg_median_price            REAL,
            tcg_lowest_with_shipping    REAL,
            total_listings              INTEGER,
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

/// Retry an async operation with exponential backoff.
pub async fn retry_with_backoff<F, Fut, T>(
    label: &str,
    max_retries: u32,
    backoff_ms: u64,
    f: F,
) -> std::result::Result<T, String>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = tcg_scanner::Result<T>>,
{
    for attempt in 0..max_retries {
        match f().await {
            Ok(val) => return Ok(val),
            Err(e) => {
                let err_str = e.to_string();
                if attempt + 1 < max_retries {
                    let delay = backoff_ms * (attempt as u64 + 1);
                    eprintln!(
                        "  Retry {}/{} for {} (waiting {}s): {}",
                        attempt + 1,
                        max_retries,
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
