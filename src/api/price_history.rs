use reqwest::Client;
use serde::{Deserialize, Deserializer, Serialize};

use crate::error::{Result, TcgError};

/// Deserialize a JSON `null` as an empty Vec (serde `default` only handles missing fields).
fn deserialize_null_as_empty<'de, D, T>(deserializer: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let opt: Option<Vec<T>> = Option::deserialize(deserializer)?;
    Ok(opt.unwrap_or_default())
}

/// Time range for price history queries.
#[derive(Debug, Clone, Copy)]
pub enum HistoryRange {
    Month,
    Quarter,
    SemiAnnual,
    Annual,
}

impl HistoryRange {
    fn as_param(&self) -> &'static str {
        match self {
            Self::Month => "month",
            Self::Quarter => "quarter",
            Self::SemiAnnual => "semi-annual",
            Self::Annual => "annual",
        }
    }
}

/// Summary price history response (aggregated by variant).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PriceHistory {
    pub count: u32,
    /// Null when the product has no sales history (e.g. pre-release cards).
    #[serde(default, deserialize_with = "deserialize_null_as_empty")]
    pub result: Vec<PriceHistoryDay>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PriceHistoryDay {
    pub date: String,
    pub variants: Vec<PriceHistoryVariant>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceHistoryVariant {
    pub variant: String,
    pub average_sales_price: Option<String>,
    pub market_price: Option<String>,
    pub quantity: Option<String>,
}

/// Detailed price history response (per-SKU with daily buckets).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DetailedPriceHistory {
    pub count: u32,
    /// Null when the product has no sales history (e.g. pre-release cards).
    #[serde(default, deserialize_with = "deserialize_null_as_empty")]
    pub result: Vec<SkuPriceHistory>,
}

/// Price history for a single SKU.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkuPriceHistory {
    pub sku_id: String,
    pub variant: String,
    pub language: String,
    pub condition: String,
    pub average_daily_quantity_sold: String,
    pub average_daily_transaction_count: String,
    pub total_quantity_sold: String,
    pub total_transaction_count: String,
    pub buckets: Vec<DailyBucket>,
}

impl SkuPriceHistory {
    /// Parse average daily quantity sold as f64.
    pub fn avg_daily_qty(&self) -> f64 {
        self.average_daily_quantity_sold.parse().unwrap_or(0.0)
    }

    /// Parse total quantity sold as u32.
    pub fn total_qty(&self) -> u32 {
        self.total_quantity_sold.parse().unwrap_or(0)
    }

    /// Parse total transaction count as u32.
    pub fn total_txns(&self) -> u32 {
        self.total_transaction_count.parse().unwrap_or(0)
    }
}

/// A single day's sales data for a SKU.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyBucket {
    pub bucket_start_date: String,
    pub market_price: String,
    pub quantity_sold: String,
    pub low_sale_price: String,
    pub low_sale_price_with_shipping: String,
    pub high_sale_price: String,
    pub high_sale_price_with_shipping: String,
    pub transaction_count: String,
}

impl DailyBucket {
    pub fn qty_sold(&self) -> u32 {
        self.quantity_sold.parse().unwrap_or(0)
    }

    pub fn txn_count(&self) -> u32 {
        self.transaction_count.parse().unwrap_or(0)
    }

    pub fn market_price_f64(&self) -> f64 {
        self.market_price.parse().unwrap_or(0.0)
    }

    pub fn low_price(&self) -> f64 {
        self.low_sale_price.parse().unwrap_or(0.0)
    }

    pub fn high_price(&self) -> f64 {
        self.high_sale_price.parse().unwrap_or(0.0)
    }
}

/// Fetch summary price history for a product.
pub async fn fetch_price_history(
    client: &Client,
    product_id: u64,
    range: HistoryRange,
) -> Result<PriceHistory> {
    let url = format!(
        "https://infinite-api.tcgplayer.com/price/history/{}?range={}",
        product_id,
        range.as_param()
    );

    let response = client
        .get(&url)
        .header("accept", "application/json")
        .header("user-agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36")
        .send()
        .await?;

    let status = response.status();
    if status.as_u16() == 403 || status.as_u16() == 429 {
        return Err(TcgError::RateLimited(status.as_u16()));
    }
    if !status.is_success() {
        return Err(TcgError::Parse(format!(
            "Price history API returned HTTP {}",
            status
        )));
    }

    let history: PriceHistory = response.json().await?;
    Ok(history)
}

/// Fetch detailed price history (per-SKU daily buckets) for a product.
pub async fn fetch_detailed_price_history(
    client: &Client,
    product_id: u64,
    range: HistoryRange,
) -> Result<DetailedPriceHistory> {
    let url = format!(
        "https://infinite-api.tcgplayer.com/price/history/{}/detailed?range={}",
        product_id,
        range.as_param()
    );

    let response = client
        .get(&url)
        .header("accept", "application/json")
        .header("user-agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36")
        .send()
        .await?;

    let status = response.status();
    if status.as_u16() == 403 || status.as_u16() == 429 {
        return Err(TcgError::RateLimited(status.as_u16()));
    }
    if !status.is_success() {
        return Err(TcgError::Parse(format!(
            "Detailed price history API returned HTTP {}",
            status
        )));
    }

    let history: DetailedPriceHistory = response.json().await?;
    Ok(history)
}
