use reqwest::Client;

use crate::error::Result;
use crate::models::PricePoint;

/// Fetch price points from mpapi.
pub async fn fetch_price_points(
    client: &Client,
    product_id: u64,
) -> Result<Vec<PricePoint>> {
    let url = format!(
        "https://mpapi.tcgplayer.com/v2/product/{}/pricepoints",
        product_id
    );

    let response = client
        .get(&url)
        .header("accept", "application/json")
        .send()
        .await?;

    let points: Vec<PricePoint> = response.json().await?;
    Ok(points)
}
