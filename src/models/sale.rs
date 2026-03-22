use serde::{Deserialize, Serialize};

/// A processed sale record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sale {
    pub condition: String,
    pub variant: String,
    pub language: String,
    pub quantity: u32,
    pub title: String,
    pub listing_type: String,
    pub purchase_price: f64,
    pub shipping_price: f64,
    pub order_date: String,
}

/// Raw API response for the latest sales endpoint.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestSalesApiResponse {
    pub data: Vec<LatestSaleEntry>,
    pub result_count: Option<u32>,
    pub total_results: Option<u32>,
}

/// A single sale entry from the mpapi response.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestSaleEntry {
    pub condition: String,
    pub variant: String,
    pub language: String,
    pub quantity: u32,
    pub title: String,
    pub listing_type: String,
    #[serde(default)]
    pub custom_listing_id: String,
    pub purchase_price: f64,
    pub shipping_price: f64,
    pub order_date: String,
}

impl From<LatestSaleEntry> for Sale {
    fn from(e: LatestSaleEntry) -> Self {
        Self {
            condition: e.condition,
            variant: e.variant,
            language: e.language,
            quantity: e.quantity,
            title: e.title,
            listing_type: e.listing_type,
            purchase_price: e.purchase_price,
            shipping_price: e.shipping_price,
            order_date: e.order_date,
        }
    }
}
