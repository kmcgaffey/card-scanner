use tcg_scanner::TcgClient;

#[tokio::main]
async fn main() -> tcg_scanner::Result<()> {
    let product_id: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(665644); // Default: Darius - Executioner (Overnumbered)

    println!("Fetching product {}...\n", product_id);

    let client = TcgClient::new()?;

    // Fetch all product data
    let product = client.get_product(product_id).await?;
    let details = &product.details;

    println!("=== Product Details ===");
    println!("Name: {}", details.product_name);
    println!("Set: {} ({})", details.set_name, details.set_code.as_deref().unwrap_or("?"));
    println!("Product Line: {}", details.product_line_name);
    if let Some(ref rarity) = details.rarity_name {
        println!("Rarity: {}", rarity);
    }
    if let Some(ref attrs) = details.custom_attributes {
        if let Some(ref num) = attrs.number {
            println!("Number: {}", num);
        }
        if let Some(ref desc) = attrs.description {
            println!("Description: {}", desc);
        }
    }
    if let Some(mp) = details.market_price {
        println!("Market Price: ${:.2}", mp);
    }
    if let Some(lp) = details.lowest_price {
        println!("Lowest Price: ${:.2}", lp);
    }
    if let Some(listings) = details.listings {
        println!("Listings: {}", listings);
    }
    if let Some(sellers) = details.sellers {
        println!("Sellers: {}", sellers);
    }

    // Attributes
    if let Some(ref attrs) = details.custom_attributes {
        println!("\n=== Attributes ===");
        if let Some(ref v) = attrs.energy_cost { println!("Energy Cost: {}", v); }
        if let Some(ref v) = attrs.power_cost { println!("Power Cost: {}", v); }
        if let Some(ref v) = attrs.might { println!("Might: {}", v); }
        let ct = attrs.card_type_list();
        if !ct.is_empty() { println!("Card Type: {}", ct.join(", ")); }
        if let Some(ref v) = attrs.tag { println!("Tag: {}", v); }
        if let Some(ref v) = attrs.domain { println!("Domain: {}", v); }
        if let Some(ref v) = attrs.artist { println!("Artist: {}", v); }
        if let Some(ref v) = attrs.flavor_text { println!("Flavor Text: {}", v); }
    }

    // SKUs
    if let Some(ref skus) = details.skus {
        println!("\n=== SKUs ===");
        for sku in skus {
            println!(
                "  SKU {}: {} / {} / {}",
                sku.sku_id,
                sku.condition_name.as_deref().unwrap_or("?"),
                sku.printing_name.as_deref().unwrap_or("?"),
                sku.language_name.as_deref().unwrap_or("?"),
            );
        }
    }

    // Price points
    println!("\n=== Price Points ===");
    for pp in &product.price_points {
        println!(
            "  {}: market={} median={}",
            pp.printing_type.as_deref().unwrap_or("?"),
            pp.market_price.map(|p| format!("${:.2}", p)).unwrap_or_else(|| "N/A".into()),
            pp.listed_median_price.map(|p| format!("${:.2}", p)).unwrap_or_else(|| "N/A".into()),
        );
    }

    // Market prices per SKU
    println!("\n=== Market Prices (per SKU) ===");
    for mp in &product.market_prices {
        println!(
            "  SKU {}: market={} low={} high={} (count: {})",
            mp.sku_id,
            mp.market_price.map(|p| format!("${:.2}", p)).unwrap_or_else(|| "N/A".into()),
            mp.lowest_price.map(|p| format!("${:.2}", p)).unwrap_or_else(|| "N/A".into()),
            mp.highest_price.map(|p| format!("${:.2}", p)).unwrap_or_else(|| "N/A".into()),
            mp.price_count.unwrap_or(0),
        );
    }

    // Listings
    println!("\n=== Listings (first page) ===");
    for (i, listing) in product.listings.iter().enumerate() {
        let shipping = listing
            .shipping_price
            .map(|s| format!("+ ${:.2} shipping", s))
            .unwrap_or_default();

        println!(
            "  {}. {} ({}) - {} {} ${:.2} {} (qty: {})",
            i + 1,
            listing.seller_name,
            listing.seller_rating.map(|r| format!("{:.1}%", r)).unwrap_or("N/A".into()),
            listing.condition,
            listing.printing,
            listing.price,
            shipping,
            listing.quantity.unwrap_or(1),
        );
    }

    // Latest sales
    println!("\n=== Latest Sales ===");
    match client.get_latest_sales(product_id, Some(10)).await {
        Ok(sales) => {
            for sale in &sales {
                println!(
                    "  {} - {} {} ${:.2} (qty: {})",
                    sale.order_date, sale.condition, sale.variant, sale.purchase_price, sale.quantity,
                );
            }
        }
        Err(e) => println!("  Failed to fetch sales: {}", e),
    }

    Ok(())
}
