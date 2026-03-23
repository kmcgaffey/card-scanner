use reqwest::Client;

use crate::api;
use crate::error::Result;
use crate::models::{Listing, PricePoint, ProductDetails, ProductPage, Sale, SkuMarketPrice};

/// Main client for interfacing with tcgplayer.com APIs.
pub struct TcgClient {
    client: Client,
}

impl TcgClient {
    /// Create a new TcgClient with default settings.
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
            .build()?;

        Ok(Self { client })
    }

    /// Fetch all product data by calling multiple API endpoints.
    pub async fn get_product(&self, product_id: u64) -> Result<ProductPage> {
        // Fetch details, listings, and price points concurrently
        let (details_result, listings_result, price_points_result) = tokio::join!(
            api::fetch_product_details(&self.client, product_id),
            api::fetch_listings(&self.client, product_id, 0, 10),
            api::fetch_price_points(&self.client, product_id),
        );

        let details = details_result?;
        let (listings, _total) = listings_result?;
        let price_points = price_points_result.unwrap_or_default();

        // Collect SKU IDs from details and listings
        let mut sku_ids: Vec<u64> = details
            .skus
            .as_ref()
            .map(|skus| skus.iter().map(|s| s.sku_id).collect())
            .unwrap_or_default();

        for listing in &listings {
            if let Some(id) = listing.product_condition_id {
                if !sku_ids.contains(&id) {
                    sku_ids.push(id);
                }
            }
        }

        let market_prices = if !sku_ids.is_empty() {
            api::fetch_market_prices(&self.client, &sku_ids)
                .await
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        Ok(ProductPage {
            details,
            listings,
            price_points,
            market_prices,
        })
    }

    /// Fetch product details only.
    pub async fn get_product_details(&self, product_id: u64) -> Result<ProductDetails> {
        api::fetch_product_details(&self.client, product_id).await
    }

    /// Fetch product listings with pagination.
    pub async fn get_product_listings(
        &self,
        product_id: u64,
        from: u32,
        size: u32,
    ) -> Result<(Vec<Listing>, u32)> {
        api::fetch_listings(&self.client, product_id, from, size).await
    }

    /// Fetch price points for a product.
    pub async fn get_price_points(&self, product_id: u64) -> Result<Vec<PricePoint>> {
        api::fetch_price_points(&self.client, product_id).await
    }

    /// Fetch market prices for specific SKU IDs.
    pub async fn get_market_prices(&self, sku_ids: &[u64]) -> Result<Vec<SkuMarketPrice>> {
        api::fetch_market_prices(&self.client, sku_ids).await
    }

    /// Fetch latest sales data.
    pub async fn get_latest_sales(
        &self,
        product_id: u64,
        limit: Option<u32>,
    ) -> Result<Vec<Sale>> {
        api::fetch_latest_sales(&self.client, product_id, limit).await
    }

    /// Search for products by query string.
    pub async fn search(
        &self,
        query: &str,
        from: u32,
        size: u32,
    ) -> Result<(Vec<api::SearchResult>, u32)> {
        api::search_products(&self.client, query, from, size).await
    }

    /// Search for products with term filters (set, product line, rarity, etc.).
    pub async fn search_filtered(
        &self,
        query: &str,
        from: u32,
        size: u32,
        filters: &api::SearchTermFilters,
    ) -> Result<(Vec<api::SearchResult>, u32)> {
        api::search_products_filtered(&self.client, query, from, size, Some(filters)).await
    }

    /// Fetch detailed price history (per-SKU daily buckets).
    pub async fn get_detailed_price_history(
        &self,
        product_id: u64,
        range: api::HistoryRange,
    ) -> Result<api::DetailedPriceHistory> {
        api::fetch_detailed_price_history(&self.client, product_id, range).await
    }
}
