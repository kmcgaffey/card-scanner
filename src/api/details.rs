use reqwest::Client;

use crate::error::Result;
use crate::models::ProductDetails;

/// Fetch product details from mp-search-api.
pub async fn fetch_product_details(
    client: &Client,
    product_id: u64,
) -> Result<ProductDetails> {
    let url = format!(
        "https://mp-search-api.tcgplayer.com/v1/product/{}/details",
        product_id
    );

    let response = client
        .get(&url)
        .header("accept", "application/json")
        .send()
        .await?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(crate::error::TcgError::NotFound(product_id));
    }

    let details: ProductDetails = response.json().await?;
    Ok(details)
}
