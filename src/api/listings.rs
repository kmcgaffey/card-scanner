use reqwest::Client;
use serde::Serialize;

use crate::error::{Result, TcgError};
use crate::models::{Listing, ListingsApiOuterResponse};

#[derive(Debug, Serialize)]
struct ListingsRequest {
    filters: ListingsFilters,
    from: u32,
    size: u32,
    sort: ListingsSort,
    context: ListingsContext,
    aggregations: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ListingsFilters {
    term: ListingsTermFilters,
    range: ListingsRangeFilters,
    exclude: ListingsExcludeFilters,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ListingsTermFilters {
    seller_status: String,
    channel_id: u32,
    language: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ListingsRangeFilters {
    quantity: QuantityRange,
}

#[derive(Debug, Serialize)]
struct QuantityRange {
    gte: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ListingsExcludeFilters {
    channel_exclusion: u32,
}

#[derive(Debug, Serialize)]
struct ListingsSort {
    field: String,
    order: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ListingsContext {
    shipping_country: String,
    cart: serde_json::Value,
}

/// Fetch product listings from mp-search-api.
pub async fn fetch_listings(
    client: &Client,
    product_id: u64,
    from: u32,
    size: u32,
) -> Result<(Vec<Listing>, u32)> {
    let url = format!(
        "https://mp-search-api.tcgplayer.com/v1/product/{}/listings",
        product_id
    );

    let body = ListingsRequest {
        filters: ListingsFilters {
            term: ListingsTermFilters {
                seller_status: "Live".into(),
                channel_id: 0,
                language: vec!["English".into()],
            },
            range: ListingsRangeFilters {
                quantity: QuantityRange { gte: 1 },
            },
            exclude: ListingsExcludeFilters {
                channel_exclusion: 0,
            },
        },
        from,
        size,
        sort: ListingsSort {
            field: "price+shipping".into(),
            order: "asc".into(),
        },
        context: ListingsContext {
            shipping_country: "US".into(),
            cart: serde_json::json!({}),
        },
        aggregations: vec!["listingType".into()],
    };

    let response = client
        .post(&url)
        .header("accept", "application/json")
        .header("content-type", "application/json")
        .header("origin", "https://www.tcgplayer.com")
        .json(&body)
        .send()
        .await?;

    let outer: ListingsApiOuterResponse = response.json().await?;

    let inner = outer
        .results
        .into_iter()
        .next()
        .ok_or_else(|| TcgError::Parse("No results in listings response".into()))?;

    let total = inner.total_results.unwrap_or(0.0) as u32;

    let listings: Vec<Listing> = inner
        .results
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect();

    Ok((listings, total))
}
