use serde::{Deserialize, Deserializer, Serialize};

fn deserialize_option_number_as_u64<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    let v: Option<f64> = Deserialize::deserialize(deserializer)?;
    Ok(v.map(|n| n as u64))
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

/// A seller listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    #[serde(default, deserialize_with = "deserialize_option_number_as_u64")]
    pub listing_id: Option<u64>,
    pub seller_id: Option<String>,
    pub seller_name: String,
    pub seller_key: Option<String>,
    pub seller_rating: Option<f64>,
    pub seller_sales: Option<String>,
    pub seller_shipping_price: Option<f64>,
    pub seller_price: Option<f64>,
    pub condition: String,
    pub printing: String,
    pub language: String,
    pub language_abbreviation: Option<String>,
    pub price: f64,
    pub shipping_price: Option<f64>,
    pub ranked_shipping_price: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_option_number_as_u32")]
    pub quantity: Option<u32>,
    #[serde(default)]
    pub gold_seller: bool,
    #[serde(default)]
    pub verified_seller: bool,
    #[serde(default)]
    pub direct_seller: bool,
    #[serde(default)]
    pub direct_product: bool,
    #[serde(default)]
    pub direct_listing: bool,
    #[serde(default)]
    pub forward_freight: bool,
    pub listing_type: Option<String>,
    pub score: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_option_number_as_u64")]
    pub product_condition_id: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_option_number_as_u64")]
    pub product_id: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_option_number_as_u64")]
    pub condition_id: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_option_number_as_u64")]
    pub channel_id: Option<u64>,
    pub listed_date: Option<String>,
    #[serde(default)]
    pub seller_programs: Vec<String>,
    pub custom_data: Option<CustomListingData>,
}

/// Custom listing data (photos, title, description).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomListingData {
    pub title: Option<String>,
    pub description: Option<String>,
    pub images: Option<Vec<String>>,
    pub link_id: Option<String>,
}

/// Top-level API response wrapper for the listings endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct ListingsApiOuterResponse {
    pub results: Vec<ListingsApiInnerResponse>,
}

/// Inner result containing the actual listings.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingsApiInnerResponse {
    pub total_results: Option<f64>,
    pub results: Vec<serde_json::Value>,
}
