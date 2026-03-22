use reqwest::Client;
use serde::Serialize;

use crate::error::Result;
use crate::models::SkuMarketPrice;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SkuSearchRequest {
    sku_ids: Vec<u64>,
}

/// Fetch market prices for a set of SKU IDs from mpgateway.
pub async fn fetch_market_prices(
    client: &Client,
    sku_ids: &[u64],
) -> Result<Vec<SkuMarketPrice>> {
    if sku_ids.is_empty() {
        return Ok(Vec::new());
    }

    let url = "https://mpgateway.tcgplayer.com/v1/pricepoints/marketprice/skus/search";

    let body = SkuSearchRequest {
        sku_ids: sku_ids.to_vec(),
    };

    let response = client
        .post(url)
        .header("accept", "application/json")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await?;

    let prices: Vec<SkuMarketPrice> = response.json().await?;
    Ok(prices)
}
