use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchRequest {
    algorithm: String,
    from: u32,
    size: u32,
    filters: SearchFilters,
    listing_search: ListingSearch,
    context: SearchContext,
}

#[derive(Debug, Serialize)]
struct SearchFilters {
    term: serde_json::Value,
    range: serde_json::Value,
    #[serde(rename = "match")]
    match_: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct ListingSearch {
    filters: ListingSearchFilters,
    context: ListingSearchContext,
}

#[derive(Debug, Serialize)]
struct ListingSearchFilters {
    term: ListingSearchTerm,
    range: ListingSearchRange,
    exclude: ListingSearchExclude,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ListingSearchTerm {
    seller_status: String,
    channel_id: u32,
}

#[derive(Debug, Serialize)]
struct ListingSearchRange {
    quantity: QuantityRange,
}

#[derive(Debug, Serialize)]
struct QuantityRange {
    gte: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ListingSearchExclude {
    channel_exclusion: u32,
}

#[derive(Debug, Serialize)]
struct ListingSearchContext {
    cart: serde_json::Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchContext {
    cart: serde_json::Value,
    shipping_country: String,
}

/// A search result entry.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub product_id: u64,
    pub product_name: String,
    pub clean_name: Option<String>,
    pub set_name: Option<String>,
    pub product_line_name: Option<String>,
    pub rarity_name: Option<String>,
    pub market_price: Option<f64>,
    pub lowest_price: Option<f64>,
    pub image_count: Option<u32>,
    pub foil_only: Option<bool>,
    pub normal_only: Option<bool>,
}

/// Response from the search endpoint.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchApiResponse {
    pub total_results: Option<u32>,
    pub results: Vec<SearchResult>,
}

/// Search for products by query string.
pub async fn search_products(
    client: &Client,
    query: &str,
    from: u32,
    size: u32,
) -> Result<(Vec<SearchResult>, u32)> {
    let url = format!(
        "https://mp-search-api.tcgplayer.com/v1/search/request?q={}&isList=false",
        urlencoding::encode(query)
    );

    let body = SearchRequest {
        algorithm: "sales_synonym_v2".into(),
        from,
        size,
        filters: SearchFilters {
            term: serde_json::json!({}),
            range: serde_json::json!({}),
            match_: serde_json::json!({}),
        },
        listing_search: ListingSearch {
            filters: ListingSearchFilters {
                term: ListingSearchTerm {
                    seller_status: "Live".into(),
                    channel_id: 0,
                },
                range: ListingSearchRange {
                    quantity: QuantityRange { gte: 1 },
                },
                exclude: ListingSearchExclude {
                    channel_exclusion: 0,
                },
            },
            context: ListingSearchContext {
                cart: serde_json::json!({}),
            },
        },
        context: SearchContext {
            cart: serde_json::json!({}),
            shipping_country: "US".into(),
        },
    };

    let response = client
        .post(&url)
        .header("accept", "application/json")
        .header("content-type", "application/json")
        .header("origin", "https://www.tcgplayer.com")
        .json(&body)
        .send()
        .await?;

    let api_response: SearchApiResponse = response.json().await?;
    let total = api_response.total_results.unwrap_or(0);
    Ok((api_response.results, total))
}
