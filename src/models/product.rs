use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

/// Deserialize a number that might come as float or integer into u64.
fn deserialize_number_as_u64<'de, D>(deserializer: D) -> std::result::Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let v: f64 = Deserialize::deserialize(deserializer)?;
    Ok(v as u64)
}

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

/// Product details from the mp-search-api /v1/product/{id}/details endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductDetails {
    #[serde(deserialize_with = "deserialize_number_as_u64")]
    pub product_id: u64,
    pub product_name: String,
    pub clean_name: Option<String>,
    pub set_name: String,
    #[serde(default, deserialize_with = "deserialize_option_number_as_u64")]
    pub set_id: Option<u64>,
    pub set_code: Option<String>,
    pub product_line_name: String,
    #[serde(default, deserialize_with = "deserialize_option_number_as_u64")]
    pub product_line_id: Option<u64>,
    pub product_type_name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_number_as_u64")]
    pub product_type_id: Option<u64>,
    pub rarity_name: Option<String>,
    pub market_price: Option<f64>,
    pub lowest_price: Option<f64>,
    pub lowest_price_with_shipping: Option<f64>,
    pub median_price: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_option_number_as_u32")]
    pub listings: Option<u32>,
    #[serde(default, deserialize_with = "deserialize_option_number_as_u32")]
    pub sellers: Option<u32>,
    #[serde(default, deserialize_with = "deserialize_option_number_as_u32")]
    pub image_count: Option<u32>,
    pub foil_only: Option<bool>,
    pub normal_only: Option<bool>,
    pub sealed: Option<bool>,
    pub custom_attributes: Option<CustomAttributes>,
    pub formatted_attributes: Option<HashMap<String, String>>,
    pub skus: Option<Vec<Sku>>,
    pub set_url_name: Option<String>,
    pub product_url_name: Option<String>,
    pub product_line_url_name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_number_as_u64")]
    pub shipping_category_id: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_option_number_as_u32")]
    pub max_fulfillable_quantity: Option<u32>,
    pub score: Option<f64>,
}

/// Custom attributes embedded in the product details response.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomAttributes {
    pub description: Option<String>,
    pub release_date: Option<String>,
    pub number: Option<String>,
    pub card_type: Option<serde_json::Value>,
    pub energy_cost: Option<String>,
    pub power_cost: Option<String>,
    pub might: Option<String>,
    pub tag: Option<String>,
    pub domain: Option<String>,
    pub artist: Option<String>,
    pub flavor_text: Option<String>,
    pub rarity_db_name: Option<String>,
    pub detail_note: Option<String>,
}

impl CustomAttributes {
    /// Get card_type as a list of strings.
    pub fn card_type_list(&self) -> Vec<String> {
        match &self.card_type {
            Some(serde_json::Value::Array(arr)) => arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            Some(serde_json::Value::String(s)) => vec![s.clone()],
            _ => Vec::new(),
        }
    }
}

/// SKU (stock keeping unit) for a specific condition/variant/language.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sku {
    #[serde(deserialize_with = "deserialize_number_as_u64")]
    pub sku_id: u64,
    #[serde(default, deserialize_with = "deserialize_option_number_as_u64")]
    pub condition_id: Option<u64>,
    pub condition_name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_number_as_u64")]
    pub printing_id: Option<u64>,
    pub printing_name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_number_as_u64")]
    pub language_id: Option<u64>,
    pub language_name: Option<String>,
    pub language_abbreviation: Option<String>,
}

/// All data assembled from multiple API calls for a product.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductPage {
    pub details: ProductDetails,
    pub listings: Vec<super::Listing>,
    pub price_points: Vec<PricePoint>,
    pub market_prices: Vec<SkuMarketPrice>,
}

/// Price point per printing type from /v2/product/{id}/pricepoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PricePoint {
    pub printing_type: Option<String>,
    pub market_price: Option<f64>,
    pub buylist_market_price: Option<f64>,
    pub listed_median_price: Option<f64>,
}

/// Market price for a specific SKU from mpgateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkuMarketPrice {
    #[serde(deserialize_with = "deserialize_number_as_u64")]
    pub sku_id: u64,
    pub market_price: Option<f64>,
    pub lowest_price: Option<f64>,
    pub highest_price: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_option_number_as_u32")]
    pub price_count: Option<u32>,
    pub calculated_at: Option<String>,
}
