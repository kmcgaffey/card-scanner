use serde::{Deserialize, Serialize};

/// Volatility data for a SKU.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Volatility {
    pub sku_id: u64,
    pub z_score: Option<f64>,
    pub volatility: Option<String>,
}
