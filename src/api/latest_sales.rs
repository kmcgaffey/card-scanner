use reqwest::Client;
use serde::Serialize;

use crate::error::Result;
use crate::models::{LatestSalesApiResponse, Sale};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LatestSalesRequest {
    variants: Vec<String>,
    conditions: Vec<String>,
    languages: Vec<String>,
    listing_type: String,
    limit: u32,
    offset: u32,
}

impl Default for LatestSalesRequest {
    fn default() -> Self {
        Self {
            variants: Vec::new(),
            conditions: Vec::new(),
            languages: Vec::new(),
            listing_type: "All".to_string(),
            limit: 25,
            offset: 0,
        }
    }
}

/// Fetch latest sales from the mpapi endpoint.
pub async fn fetch_latest_sales(
    client: &Client,
    product_id: u64,
    limit: Option<u32>,
) -> Result<Vec<Sale>> {
    let url = format!(
        "https://mpapi.tcgplayer.com/v2/product/{}/latestsales",
        product_id
    );

    let body = LatestSalesRequest {
        limit: limit.unwrap_or(25),
        ..Default::default()
    };

    let response = client
        .post(&url)
        .header("accept", "application/json")
        .header("content-type", "application/json")
        .header("origin", "https://www.tcgplayer.com")
        .json(&body)
        .send()
        .await?;

    let api_response: LatestSalesApiResponse = response.json().await?;
    Ok(api_response.data.into_iter().map(Sale::from).collect())
}
