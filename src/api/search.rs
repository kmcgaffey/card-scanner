use reqwest::Client;
use serde::{Deserialize, Deserializer, Serialize};
use crate::error::Result;

fn deserialize_number_as_u64<'de, D>(deserializer: D) -> std::result::Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let v: f64 = Deserialize::deserialize(deserializer)?;
    Ok(v as u64)
}

fn deserialize_option_number_as_u32<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    let v: Option<f64> = Deserialize::deserialize(deserializer)?;
    Ok(v.map(|n| n as u32))
}

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
    #[serde(deserialize_with = "deserialize_number_as_u64")]
    pub product_id: u64,
    pub product_name: String,
    pub clean_name: Option<String>,
    pub set_name: Option<String>,
    pub product_line_name: Option<String>,
    pub rarity_name: Option<String>,
    pub market_price: Option<f64>,
    pub median_price: Option<f64>,
    pub lowest_price: Option<f64>,
    pub lowest_price_with_shipping: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_option_number_as_u32")]
    pub total_listings: Option<u32>,
    pub foil_only: Option<bool>,
    pub normal_only: Option<bool>,
    pub sealed: Option<bool>,
    pub custom_attributes: Option<serde_json::Value>,
}

/// Response from the search endpoint.
/// The outer response has a `results` array of inner result groups.
#[derive(Debug, Clone, Deserialize)]
pub struct SearchApiOuterResponse {
    pub results: Vec<SearchApiInnerResponse>,
}

/// Inner result group containing the actual search results.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchApiInnerResponse {
    pub total_results: Option<f64>,
    pub results: Vec<serde_json::Value>,
}

/// Term filters for narrowing search results.
#[derive(Debug, Clone, Default)]
pub struct SearchTermFilters {
    pub product_line_name: Option<Vec<String>>,
    pub set_name: Option<Vec<String>>,
    pub product_type_name: Option<Vec<String>>,
    pub rarity_name: Option<Vec<String>>,
}

impl SearchTermFilters {
    fn to_json(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        if let Some(ref v) = self.product_line_name {
            map.insert("productLineName".into(), serde_json::json!(v));
        }
        if let Some(ref v) = self.set_name {
            map.insert("setName".into(), serde_json::json!(v));
        }
        if let Some(ref v) = self.product_type_name {
            map.insert("productTypeName".into(), serde_json::json!(v));
        }
        if let Some(ref v) = self.rarity_name {
            map.insert("rarityName".into(), serde_json::json!(v));
        }
        serde_json::Value::Object(map)
    }
}

/// Search for products by query string.
pub async fn search_products(
    client: &Client,
    query: &str,
    from: u32,
    size: u32,
) -> Result<(Vec<SearchResult>, u32)> {
    search_products_filtered(client, query, from, size, None).await
}

/// Search for products with optional term filters.
pub async fn search_products_filtered(
    client: &Client,
    query: &str,
    from: u32,
    size: u32,
    filters: Option<&SearchTermFilters>,
) -> Result<(Vec<SearchResult>, u32)> {
    let url = format!(
        "https://mp-search-api.tcgplayer.com/v1/search/request?q={}&isList=false",
        urlencoding::encode(query)
    );

    let term_filters = filters
        .map(|f| f.to_json())
        .unwrap_or_else(|| serde_json::json!({}));

    let body = SearchRequest {
        algorithm: "sales_synonym_v2".into(),
        from,
        size,
        filters: SearchFilters {
            term: term_filters,
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

    let outer: SearchApiOuterResponse = response.json().await?;
    let inner = outer.results.into_iter().next().ok_or_else(|| {
        crate::error::TcgError::Parse("No results in search response".into())
    })?;

    let total = inner.total_results.unwrap_or(0.0) as u32;
    let results: Vec<SearchResult> = inner
        .results
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect();

    Ok((results, total))
}
