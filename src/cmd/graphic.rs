use std::path::Path;

use ab_glyph::{FontRef, PxScale};
use image::imageops::FilterType;
use image::{DynamicImage, Rgba, RgbaImage};
use imageproc::drawing::{draw_filled_rect_mut, draw_text_mut};
use imageproc::rect::Rect;

use super::alerts::{CardEntry, GraphicCards, PriceMover};

// Canvas dimensions (Twitter-optimized 16:9)
const WIDTH: u32 = 1200;
const HEIGHT: u32 = 675;

// Colors
const BG: Rgba<u8> = Rgba([18, 18, 30, 255]);
const ACCENT: Rgba<u8> = Rgba([99, 102, 241, 255]);
const TEXT_PRIMARY: Rgba<u8> = Rgba([240, 240, 245, 255]);
const TEXT_SECONDARY: Rgba<u8> = Rgba([160, 160, 180, 255]);
const PRICE_UP: Rgba<u8> = Rgba([80, 200, 120, 255]);    // green
const PRICE_DOWN: Rgba<u8> = Rgba([240, 80, 80, 255]);   // red
const PRICE_FLAT: Rgba<u8> = Rgba([160, 160, 180, 255]); // gray
const BAR_COLORS: [Rgba<u8>; 5] = [
    Rgba([99, 102, 241, 255]),  // indigo
    Rgba([79, 140, 255, 255]),  // blue
    Rgba([56, 178, 226, 255]),  // cyan
    Rgba([45, 190, 180, 255]),  // teal
    Rgba([60, 200, 150, 255]),  // green
];

// Rarity colors for the "Top by Rarity" section
const RARITY_COLORS: [(&str, Rgba<u8>); 6] = [
    ("Common",   Rgba([140, 140, 155, 255])),   // gray
    ("Uncommon",  Rgba([80, 180, 100, 255])),    // green
    ("Rare",      Rgba([70, 130, 230, 255])),    // blue
    ("Epic",      Rgba([180, 80, 220, 255])),    // purple
    ("Showcase",  Rgba([230, 170, 50, 255])),    // gold
    ("Promo",     Rgba([220, 100, 100, 255])),   // red
];

fn rarity_color(rarity: &str) -> Rgba<u8> {
    RARITY_COLORS
        .iter()
        .find(|(r, _)| *r == rarity)
        .map(|(_, c)| *c)
        .unwrap_or(TEXT_SECONDARY)
}

// Fonts embedded at compile time
const FONT_BOLD: &[u8] = include_bytes!("../../assets/Inter-Bold.ttf");
const FONT_REGULAR: &[u8] = include_bytes!("../../assets/Inter-Regular.ttf");

// TCGPlayer CDN image URL pattern
const CDN_BASE: &str = "https://product-images.tcgplayer.com/fit-in";

async fn download_card_image(client: &reqwest::Client, product_id: u64) -> Option<DynamicImage> {
    let url = format!("{}/400x400/{}.jpg", CDN_BASE, product_id);
    let resp = client.get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let bytes = resp.bytes().await.ok()?;
    image::load_from_memory(&bytes).ok()
}

/// Resize an image to fit within target dimensions, preserving aspect ratio.
fn resize_fit(img: &DynamicImage, target_w: u32, target_h: u32) -> RgbaImage {
    let (iw, ih) = (img.width() as f32, img.height() as f32);
    let scale = (target_w as f32 / iw).min(target_h as f32 / ih);
    let new_w = (iw * scale).round() as u32;
    let new_h = (ih * scale).round() as u32;
    img.resize_exact(new_w, new_h, FilterType::Lanczos3).to_rgba8()
}

/// Truncate a string to `max_len` characters, adding "..." if needed.
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    } else {
        s.to_string()
    }
}

/// Download images for a list of card entries concurrently.
async fn download_images(
    client: &reqwest::Client,
    cards: &[CardEntry],
) -> Vec<Option<DynamicImage>> {
    let mut images: Vec<Option<DynamicImage>> = Vec::with_capacity(cards.len());
    let mut futures = Vec::new();
    for (i, card) in cards.iter().enumerate() {
        let client = client.clone();
        let pid = card.product_id;
        let fallback_pid = card.fallback_product_id;
        futures.push(tokio::spawn(async move {
            let img = download_card_image(&client, pid).await;
            if img.is_some() {
                return (i, img);
            }
            if let Some(fb_pid) = fallback_pid {
                return (i, download_card_image(&client, fb_pid).await);
            }
            (i, None)
        }));
    }
    images.resize_with(cards.len(), || None);
    for handle in futures {
        if let Ok((idx, img)) = handle.await {
            images[idx] = img;
        }
    }
    images
}

/// Format a price change as arrow + percentage string.
fn format_price_change(pct: f64) -> (String, Rgba<u8>) {
    if pct > 0.5 {
        (format!("\u{25B2}{:.1}%", pct.abs()), PRICE_UP)
    } else if pct < -0.5 {
        (format!("\u{25BC}{:.1}%", pct.abs()), PRICE_DOWN)
    } else {
        ("-0.0%".to_string(), PRICE_FLAT)
    }
}

/// Estimate text width in pixels for a given font size (approximate).
fn text_width(s: &str, font_size: f32) -> u32 {
    // Approximate character width as 0.55 * font_size for Inter font
    (s.len() as f32 * font_size * 0.55) as u32
}

/// Draw a single row in the bar chart section.
fn draw_row(
    canvas: &mut RgbaImage,
    font_bold: &FontRef,
    font_regular: &FontRef,
    card: &CardEntry,
    image: &Option<DynamicImage>,
    rank: usize,
    row_y: u32,
    row_h: u32,
    thumb_x: u32,
    bar_x: u32,
    bar_max_w: u32,
    max_qty: u32,
    padding: u32,
    right_edge: u32,
) {
    let center_y = row_y + row_h / 2;

    // Alternating row background
    if rank % 2 == 0 {
        draw_filled_rect_mut(
            canvas,
            Rect::at(0, row_y as i32).of_size(right_edge, row_h),
            Rgba([22, 22, 36, 255]),
        );
    }

    // Rank number
    let rank_label = format!("#{}", rank + 1);
    let rank_color = if rank == 0 { ACCENT } else { TEXT_SECONDARY };
    draw_text_mut(
        canvas,
        rank_color,
        (padding + 4) as i32,
        (center_y - 10) as i32,
        PxScale::from(20.0),
        font_bold,
        &rank_label,
    );

    // Full card image (scaled to fit row height)
    let card_h = row_h.saturating_sub(8);
    let card_w = (card_h as f32 * 0.715) as u32;
    let card_y = row_y + 4;
    if let Some(ref img) = image {
        let fitted = resize_fit(img, card_w, card_h);
        let img_x = thumb_x + (card_w.saturating_sub(fitted.width())) / 2;
        let img_y = card_y + (card_h.saturating_sub(fitted.height())) / 2;
        image::imageops::overlay(canvas, &fitted, img_x as i64, img_y as i64);
    }

    // Card name (left-aligned)
    let name = truncate(&card.display_name, 28);
    draw_text_mut(
        canvas,
        TEXT_PRIMARY,
        bar_x as i32,
        (center_y - 26) as i32,
        PxScale::from(20.0),
        font_bold,
        &name,
    );

    // Price + change (right-justified)
    let price_font_size = 16.0f32;
    let change_str = card.price_change_pct.map(|pct| format_price_change(pct));
    let price_label = format!("${:.2}", card.avg_price);
    let change_w = change_str.as_ref().map(|(s, _)| text_width(s, price_font_size) + 4).unwrap_or(0);
    let price_w = text_width(&price_label, price_font_size);
    let total_price_w = price_w + change_w + 4;
    let price_x = right_edge - total_price_w - padding;

    draw_text_mut(
        canvas,
        TEXT_SECONDARY,
        price_x as i32,
        (center_y - 24) as i32,
        PxScale::from(price_font_size),
        font_regular,
        &price_label,
    );

    if let Some((label, color)) = &change_str {
        draw_text_mut(
            canvas,
            *color,
            (price_x + price_w + 4) as i32,
            (center_y - 24) as i32,
            PxScale::from(price_font_size),
            font_bold,
            label,
        );
    }

    // Horizontal bar (cap width so it doesn't overlap price)
    let available_bar_w = (price_x.saturating_sub(bar_x).saturating_sub(8)).min(bar_max_w);
    let bar_w = if max_qty > 0 {
        (card.total_qty as f64 / max_qty as f64 * available_bar_w as f64) as u32
    } else {
        0
    };
    let bar_h = 22u32;
    let bar_y = center_y + 2;

    let bar_color = BAR_COLORS[rank % BAR_COLORS.len()];
    draw_filled_rect_mut(
        canvas,
        Rect::at(bar_x as i32, bar_y as i32).of_size(bar_w.max(4), bar_h),
        bar_color,
    );

    // Count label
    let qty_label = format!("{}", card.total_qty);
    draw_text_mut(
        canvas,
        TEXT_PRIMARY,
        (bar_x + bar_w + 8) as i32,
        (bar_y + 1) as i32,
        PxScale::from(18.0),
        font_bold,
        &qty_label,
    );

    // "copies" suffix
    draw_text_mut(
        canvas,
        TEXT_SECONDARY,
        (bar_x + bar_w + 8 + qty_label.len() as u32 * 11 + 4) as i32,
        (bar_y + 3) as i32,
        PxScale::from(14.0),
        font_regular,
        "copies",
    );
}

/// Generate a dark-themed graphic with two sections:
/// - Left: Top 5 overall by volume (bar chart with thumbnails)
/// - Right: Top card per rarity (bar chart with rarity-colored bars)
///
/// Layout (1200x675):
/// ```text
/// +--------------------------------------------------+
/// |  Title                                            |
/// |==================================================|
/// |  TOP SELLERS         |  TOP BY RARITY             |
/// |  #1 [img] Card  ███  |  [img] Card (Common)  ██  |
/// |  #2 [img] Card  ██   |  [img] Card (Uncomm)  ██  |
/// |  #3 [img] Card  █    |  [img] Card (Rare)    ██  |
/// |  #4 [img] Card  █    |  [img] Card (Epic)    ██  |
/// |  #5 [img] Card  █    |  [img] Card (Showcase) █  |
/// +--------------------------------------------------+
/// ```
pub async fn generate_graphic(
    cards: &GraphicCards,
    title: &str,
    output_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if cards.top_overall.is_empty() {
        return Err("No cards to render".into());
    }

    let font_bold = FontRef::try_from_slice(FONT_BOLD)?;
    let font_regular = FontRef::try_from_slice(FONT_REGULAR)?;

    let mut canvas = RgbaImage::from_pixel(WIDTH, HEIGHT, BG);

    // Collect all cards for image downloads
    let all_cards: Vec<&CardEntry> = cards
        .top_overall
        .iter()
        .chain(cards.top_by_rarity.iter())
        .collect();

    let client = reqwest::Client::new();

    // Download all images concurrently
    let all_entries: Vec<CardEntry> = all_cards
        .iter()
        .map(|c| CardEntry {
            product_id: c.product_id,
            product_name: c.product_name.clone(),
            display_name: c.display_name.clone(),
            total_qty: c.total_qty,
            rarity: c.rarity.clone(),
            fallback_product_id: c.fallback_product_id,
            avg_price: c.avg_price,
            price_change_pct: c.price_change_pct,
        })
        .collect();
    let all_images = download_images(&client, &all_entries).await;

    let overall_images = &all_images[..cards.top_overall.len()];
    let rarity_images = &all_images[cards.top_overall.len()..];

    let has_rarity = !cards.top_by_rarity.is_empty();

    // === Title bar ===
    draw_filled_rect_mut(
        &mut canvas,
        Rect::at(0, 0).of_size(WIDTH, 52),
        Rgba([25, 25, 42, 255]),
    );
    draw_filled_rect_mut(
        &mut canvas,
        Rect::at(0, 52).of_size(WIDTH, 3),
        ACCENT,
    );
    draw_text_mut(
        &mut canvas,
        TEXT_PRIMARY,
        24,
        12,
        PxScale::from(28.0),
        &font_bold,
        &truncate(title, 55),
    );

    // === Layout constants ===
    let content_top = 70u32;
    let content_bottom = HEIGHT - 16;
    let content_h = content_bottom - content_top;
    let padding = 16u32;

    // Split into two columns if we have rarity cards
    let left_w = if has_rarity { WIDTH / 2 - 8 } else { WIDTH };
    let right_x = if has_rarity { WIDTH / 2 + 8 } else { WIDTH };

    // === Left column: Top Overall ===
    // Section header
    draw_text_mut(
        &mut canvas,
        ACCENT,
        (padding + 2) as i32,
        (content_top + 2) as i32,
        PxScale::from(16.0),
        &font_bold,
        "TOP SELLERS",
    );

    let section_top = content_top + 26;
    let overall_count = cards.top_overall.len().min(5) as u32;
    let row_h = (content_h - 26) / overall_count.max(1);
    let thumb_x = padding + 38;
    let card_w = ((row_h.saturating_sub(8)) as f32 * 0.715) as u32;
    let bar_x = thumb_x + card_w + 12;
    let bar_max_w = ((left_w - bar_x - 90) as f32 * 0.75) as u32;
    let max_qty_overall = cards.top_overall.iter().map(|c| c.total_qty).max().unwrap_or(1);

    for (i, card) in cards.top_overall.iter().take(5).enumerate() {
        let row_y = section_top + (i as u32) * row_h;
        draw_row(
            &mut canvas,
            &font_bold,
            &font_regular,
            card,
            &overall_images[i],
            i,
            row_y,
            row_h,
            thumb_x,
            bar_x,
            bar_max_w,
            max_qty_overall,
            padding,
            left_w,
        );
    }

    // === Right column: Top by Rarity ===
    if has_rarity {
        // Vertical divider
        draw_filled_rect_mut(
            &mut canvas,
            Rect::at((WIDTH / 2) as i32, (content_top) as i32).of_size(2, content_h),
            Rgba([40, 40, 60, 255]),
        );

        // Section header
        draw_text_mut(
            &mut canvas,
            ACCENT,
            (right_x + padding - 8) as i32,
            (content_top + 2) as i32,
            PxScale::from(16.0),
            &font_bold,
            "TOP BY RARITY",
        );

        let rarity_count = cards.top_by_rarity.len() as u32;
        let rarity_row_h = (content_h - 26) / rarity_count.max(1);
        let r_thumb_x = right_x + padding + 2;
        let r_card_w = ((rarity_row_h.saturating_sub(8)) as f32 * 0.715) as u32;
        let r_bar_x = r_thumb_x + r_card_w + 12;
        let r_bar_max_w = ((WIDTH - r_bar_x - 90) as f32 * 0.75) as u32;
        let max_qty_rarity = cards.top_by_rarity.iter().map(|c| c.total_qty).max().unwrap_or(1);

        for (i, card) in cards.top_by_rarity.iter().enumerate() {
            let row_y = section_top + (i as u32) * rarity_row_h;

            let center_y = row_y + rarity_row_h / 2;

            // Alternating row background (right side only)
            if i % 2 == 0 {
                draw_filled_rect_mut(
                    &mut canvas,
                    Rect::at(right_x as i32, row_y as i32).of_size(WIDTH - right_x, rarity_row_h),
                    Rgba([22, 22, 36, 255]),
                );
            }

            let rc = rarity_color(&card.rarity);

            // Full card image
            let r_card_h = rarity_row_h.saturating_sub(8);
            let card_y = row_y + 4;
            if let Some(ref img) = rarity_images[i] {
                let fitted = resize_fit(img, r_card_w, r_card_h);
                let img_x = r_thumb_x + (r_card_w.saturating_sub(fitted.width())) / 2;
                let img_y = card_y + (r_card_h.saturating_sub(fitted.height())) / 2;
                image::imageops::overlay(&mut canvas, &fitted, img_x as i64, img_y as i64);
            }

            // Rarity label (before card name)
            let rarity_label = format!("[{}]", card.rarity);
            draw_text_mut(
                &mut canvas,
                rc,
                r_bar_x as i32,
                (center_y - 24) as i32,
                PxScale::from(14.0),
                &font_bold,
                &rarity_label,
            );

            // Card name (after rarity)
            let name_x = r_bar_x + text_width(&rarity_label, 14.0) + 4;
            let name = truncate(&card.display_name, 20);
            draw_text_mut(
                &mut canvas,
                TEXT_PRIMARY,
                name_x as i32,
                (center_y - 26) as i32,
                PxScale::from(18.0),
                &font_bold,
                &name,
            );

            // Price + change (right-justified)
            let r_price_font = 15.0f32;
            let r_change_str = card.price_change_pct.map(|pct| format_price_change(pct));
            let r_price_label = format!("${:.2}", card.avg_price);
            let r_change_w = r_change_str.as_ref().map(|(s, _)| text_width(s, r_price_font) + 4).unwrap_or(0);
            let r_price_w = text_width(&r_price_label, r_price_font);
            let r_total_w = r_price_w + r_change_w + 4;
            let r_price_x = WIDTH - r_total_w - 16;

            draw_text_mut(
                &mut canvas,
                TEXT_SECONDARY,
                r_price_x as i32,
                (center_y - 24) as i32,
                PxScale::from(r_price_font),
                &font_regular,
                &r_price_label,
            );

            if let Some((label, color)) = &r_change_str {
                draw_text_mut(
                    &mut canvas,
                    *color,
                    (r_price_x + r_price_w + 4) as i32,
                    (center_y - 24) as i32,
                    PxScale::from(r_price_font),
                    &font_bold,
                    label,
                );
            }

            // Bar
            let bar_w = if max_qty_rarity > 0 {
                (card.total_qty as f64 / max_qty_rarity as f64 * r_bar_max_w as f64) as u32
            } else {
                0
            };
            let bar_h = 20u32;
            let bar_y = center_y + 2;
            draw_filled_rect_mut(
                &mut canvas,
                Rect::at(r_bar_x as i32, bar_y as i32).of_size(bar_w.max(4), bar_h),
                rc,
            );

            // Count label
            let qty_label = format!("{}", card.total_qty);
            draw_text_mut(
                &mut canvas,
                TEXT_PRIMARY,
                (r_bar_x + bar_w + 8) as i32,
                (bar_y + 1) as i32,
                PxScale::from(16.0),
                &font_bold,
                &qty_label,
            );
        }
    }

    // === Bottom accent bar ===
    draw_filled_rect_mut(
        &mut canvas,
        Rect::at(0, (HEIGHT - 3) as i32).of_size(WIDTH, 3),
        ACCENT,
    );

    canvas.save(output_path)?;
    Ok(())
}

/// Helper to draw title bar and bottom accent on a canvas.
fn draw_chrome(canvas: &mut RgbaImage, font_bold: &FontRef, title: &str, w: u32, h: u32) {
    draw_filled_rect_mut(canvas, Rect::at(0, 0).of_size(w, 52), Rgba([25, 25, 42, 255]));
    draw_filled_rect_mut(canvas, Rect::at(0, 52).of_size(w, 3), ACCENT);
    draw_text_mut(canvas, TEXT_PRIMARY, 24, 12, PxScale::from(28.0), font_bold, &truncate(title, 55));
    draw_filled_rect_mut(canvas, Rect::at(0, (h - 3) as i32).of_size(w, 3), ACCENT);
}

/// Generate two separate images: one for top sellers, one for top by rarity.
/// Each image is full-width (1200x675) for maximum clarity.
pub async fn generate_split_graphics(
    cards: &GraphicCards,
    title: &str,
    output_base: &Path,
) -> Result<Vec<std::path::PathBuf>, Box<dyn std::error::Error>> {
    if cards.top_overall.is_empty() {
        return Err("No cards to render".into());
    }

    let font_bold = FontRef::try_from_slice(FONT_BOLD)?;
    let font_regular = FontRef::try_from_slice(FONT_REGULAR)?;
    let client = reqwest::Client::new();
    let padding = 16u32;
    let content_top = 70u32;
    let content_bottom = HEIGHT - 16;
    let content_h = content_bottom - content_top;
    let section_top = content_top + 26;

    let mut paths = Vec::new();

    // Build output paths from base path
    let stem = output_base.file_stem().unwrap_or_default().to_string_lossy();
    let ext = output_base.extension().unwrap_or_default().to_string_lossy();
    let parent = output_base.parent().unwrap_or(Path::new("."));

    // === Image 1: Top Sellers ===
    {
        let mut canvas = RgbaImage::from_pixel(WIDTH, HEIGHT, BG);
        let sellers_title = format!("{} — Top Sellers", title);
        draw_chrome(&mut canvas, &font_bold, &sellers_title, WIDTH, HEIGHT);

        draw_text_mut(&mut canvas, ACCENT, (padding + 2) as i32, (content_top + 2) as i32,
            PxScale::from(16.0), &font_bold, "TOP SELLERS BY VOLUME");

        let overall_count = cards.top_overall.len().min(5) as u32;
        let row_h = (content_h - 26) / overall_count.max(1);
        let thumb_x = padding + 38;
        let card_w = ((row_h.saturating_sub(8)) as f32 * 0.715) as u32;
        let bar_x = thumb_x + card_w + 12;
        let bar_max_w = ((WIDTH - bar_x - 90) as f32 * 0.75) as u32;
        let max_qty = cards.top_overall.iter().map(|c| c.total_qty).max().unwrap_or(1);

        // Download images
        let overall_images = download_images(&client, &cards.top_overall).await;

        for (i, card) in cards.top_overall.iter().take(5).enumerate() {
            let row_y = section_top + (i as u32) * row_h;
            draw_row(&mut canvas, &font_bold, &font_regular, card, &overall_images[i],
                i, row_y, row_h, thumb_x, bar_x, bar_max_w, max_qty, padding, WIDTH);
        }

        let path = parent.join(format!("{}_sellers.{}", stem, ext));
        canvas.save(&path)?;
        paths.push(path);
    }

    // === Image 2: Top by Rarity ===
    if !cards.top_by_rarity.is_empty() {
        let mut canvas = RgbaImage::from_pixel(WIDTH, HEIGHT, BG);
        let rarity_title = format!("{} — Top by Rarity", title);
        draw_chrome(&mut canvas, &font_bold, &rarity_title, WIDTH, HEIGHT);

        draw_text_mut(&mut canvas, ACCENT, (padding + 2) as i32, (content_top + 2) as i32,
            PxScale::from(16.0), &font_bold, "TOP SELLER PER RARITY");

        let rarity_count = cards.top_by_rarity.len() as u32;
        let row_h = (content_h - 26) / rarity_count.max(1);
        let thumb_x = padding + 2;
        let card_w = ((row_h.saturating_sub(8)) as f32 * 0.715) as u32;
        let bar_x = thumb_x + card_w + 12;
        let bar_max_w = ((WIDTH - bar_x - 90) as f32 * 0.75) as u32;
        let max_qty = cards.top_by_rarity.iter().map(|c| c.total_qty).max().unwrap_or(1);

        let rarity_images = download_images(&client, &cards.top_by_rarity).await;

        for (i, card) in cards.top_by_rarity.iter().enumerate() {
            let row_y = section_top + (i as u32) * row_h;
            let rc = rarity_color(&card.rarity);
            let center_y = row_y + row_h / 2;

            // Alternating row background
            if i % 2 == 0 {
                draw_filled_rect_mut(&mut canvas,
                    Rect::at(0, row_y as i32).of_size(WIDTH, row_h), Rgba([22, 22, 36, 255]));
            }

            // Full card image
            let card_h = row_h.saturating_sub(8);
            let card_y = row_y + 4;
            if let Some(ref img) = rarity_images[i] {
                let fitted = resize_fit(img, card_w, card_h);
                let img_x = thumb_x + (card_w.saturating_sub(fitted.width())) / 2;
                let img_y = card_y + (card_h.saturating_sub(fitted.height())) / 2;
                image::imageops::overlay(&mut canvas, &fitted, img_x as i64, img_y as i64);
            }

            // Rarity label before card name
            let rarity_label = format!("[{}]", card.rarity);
            draw_text_mut(&mut canvas, rc, bar_x as i32, (center_y - 24) as i32,
                PxScale::from(16.0), &font_bold, &rarity_label);

            let name_x = bar_x + text_width(&rarity_label, 16.0) + 4;
            let name = truncate(&card.display_name, 24);
            draw_text_mut(&mut canvas, TEXT_PRIMARY, name_x as i32, (center_y - 26) as i32,
                PxScale::from(20.0), &font_bold, &name);

            // Price + change (right-justified)
            let r_price_font = 16.0f32;
            let r_change_str = card.price_change_pct.map(|pct| format_price_change(pct));
            let r_price_label = format!("${:.2}", card.avg_price);
            let r_change_w = r_change_str.as_ref().map(|(s, _)| text_width(s, r_price_font) + 4).unwrap_or(0);
            let r_price_w = text_width(&r_price_label, r_price_font);
            let r_total_w = r_price_w + r_change_w + 4;
            let r_price_x = WIDTH - r_total_w - 16;

            draw_text_mut(&mut canvas, TEXT_SECONDARY, r_price_x as i32, (center_y - 24) as i32,
                PxScale::from(r_price_font), &font_regular, &r_price_label);

            if let Some((label, color)) = &r_change_str {
                draw_text_mut(&mut canvas, *color, (r_price_x + r_price_w + 4) as i32,
                    (center_y - 24) as i32, PxScale::from(r_price_font), &font_bold, label);
            }

            // Bar (rarity-colored)
            let bar_w = if max_qty > 0 {
                (card.total_qty as f64 / max_qty as f64 * bar_max_w as f64) as u32
            } else { 0 };
            let bar_h = 22u32;
            let bar_y = center_y + 2;
            draw_filled_rect_mut(&mut canvas,
                Rect::at(bar_x as i32, bar_y as i32).of_size(bar_w.max(4), bar_h), rc);

            let qty_label = format!("{} copies", card.total_qty);
            draw_text_mut(&mut canvas, TEXT_PRIMARY, (bar_x + bar_w + 8) as i32, (bar_y + 1) as i32,
                PxScale::from(18.0), &font_bold, &qty_label);
        }

        let path = parent.join(format!("{}_rarity.{}", stem, ext));
        canvas.save(&path)?;
        paths.push(path);
    }

    Ok(paths)
}

/// Draw a single mover row with card image, name, price change bar, and labels.
fn draw_mover_row(
    canvas: &mut RgbaImage,
    font_bold: &FontRef,
    font_regular: &FontRef,
    mover: &PriceMover,
    image: &Option<DynamicImage>,
    rank: usize,
    row_y: u32,
    row_h: u32,
    is_gainer: bool,
    max_pct: f64,
    section_x: u32,
    section_w: u32,
) {
    let center_y = row_y + row_h / 2;
    let padding = 12u32;
    let bar_color = if is_gainer { PRICE_UP } else { PRICE_DOWN };

    // Alternating row background
    if rank % 2 == 0 {
        draw_filled_rect_mut(canvas,
            Rect::at(section_x as i32, row_y as i32).of_size(section_w, row_h),
            Rgba([22, 22, 36, 255]));
    }

    // Card image
    let card_h = row_h.saturating_sub(8);
    let card_w = (card_h as f32 * 0.715) as u32;
    let thumb_x = section_x + padding;
    let card_y = row_y + 4;
    if let Some(ref img) = image {
        let fitted = resize_fit(img, card_w, card_h);
        let img_x = thumb_x + (card_w.saturating_sub(fitted.width())) / 2;
        let img_y = card_y + (card_h.saturating_sub(fitted.height())) / 2;
        image::imageops::overlay(canvas, &fitted, img_x as i64, img_y as i64);
    }

    let text_x = thumb_x + card_w + 10;
    let right_edge = section_x + section_w;

    // Card name
    let name = truncate(&mover.product_name, 24);
    draw_text_mut(canvas, TEXT_PRIMARY, text_x as i32, (center_y - 28) as i32,
        PxScale::from(20.0), font_bold, &name);

    // Price: was → now (right-justified)
    let change_label = if is_gainer {
        format!("+{:.1}%", mover.change_pct)
    } else {
        format!("{:.1}%", mover.change_pct)
    };
    let price_str = format!("${:.2} → ${:.2}", mover.previous_price, mover.current_price);
    let change_w = text_width(&change_label, 16.0);
    let price_w = text_width(&price_str, 14.0);
    let total_w = price_w + change_w + 8;
    let price_x = right_edge - total_w - padding;

    draw_text_mut(canvas, TEXT_SECONDARY, price_x as i32, (center_y - 26) as i32,
        PxScale::from(14.0), font_regular, &price_str);

    draw_text_mut(canvas, bar_color, (price_x + price_w + 8) as i32, (center_y - 28) as i32,
        PxScale::from(16.0), font_bold, &change_label);

    // Percentage bar
    let bar_max_w = ((right_edge - text_x - padding) as f32 * 0.65) as u32;
    let bar_w = if max_pct > 0.0 {
        (mover.change_pct.abs() / max_pct * bar_max_w as f64) as u32
    } else { 0 };
    let bar_h = 20u32;
    let bar_y = center_y + 2;

    draw_filled_rect_mut(canvas,
        Rect::at(text_x as i32, bar_y as i32).of_size(bar_w.max(4), bar_h), bar_color);

    // Volume label after bar
    let vol_label = format!("{} sold", mover.volume);
    draw_text_mut(canvas, TEXT_SECONDARY, (text_x + bar_w + 8) as i32, (bar_y + 2) as i32,
        PxScale::from(14.0), font_regular, &vol_label);
}

/// Generate a graphic showing top gainers and losers side by side.
pub async fn generate_movers_graphic(
    gainers: &[PriceMover],
    losers: &[PriceMover],
    title: &str,
    output_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if gainers.is_empty() && losers.is_empty() {
        return Err("No price movers to render".into());
    }

    let font_bold = FontRef::try_from_slice(FONT_BOLD)?;
    let font_regular = FontRef::try_from_slice(FONT_REGULAR)?;
    let client = reqwest::Client::new();

    let mut canvas = RgbaImage::from_pixel(WIDTH, HEIGHT, BG);
    draw_chrome(&mut canvas, &font_bold, title, WIDTH, HEIGHT);

    let content_top = 70u32;
    let content_bottom = HEIGHT - 16;
    let content_h = content_bottom - content_top;
    let section_top = content_top + 26;

    let half_w = WIDTH / 2 - 4;
    let right_x = WIDTH / 2 + 4;

    // Download all images concurrently
    let top_gainers = &gainers[..gainers.len().min(5)];
    let top_losers = &losers[..losers.len().min(5)];

    let gainer_entries: Vec<CardEntry> = top_gainers.iter().map(|m| CardEntry {
        product_id: m.product_id, product_name: m.product_name.clone(),
        display_name: m.product_name.clone(), total_qty: m.volume,
        rarity: m.rarity.clone(), fallback_product_id: None,
        avg_price: m.current_price, price_change_pct: Some(m.change_pct),
    }).collect();
    let loser_entries: Vec<CardEntry> = top_losers.iter().map(|m| CardEntry {
        product_id: m.product_id, product_name: m.product_name.clone(),
        display_name: m.product_name.clone(), total_qty: m.volume,
        rarity: m.rarity.clone(), fallback_product_id: None,
        avg_price: m.current_price, price_change_pct: Some(m.change_pct),
    }).collect();

    let gainer_images = download_images(&client, &gainer_entries).await;
    let loser_images = download_images(&client, &loser_entries).await;

    // === Left column: Gainers ===
    // Section header
    draw_text_mut(&mut canvas, PRICE_UP, 18, (content_top + 2) as i32,
        PxScale::from(16.0), &font_bold, "\u{25B2} BIGGEST GAINERS");

    let row_count = top_gainers.len().max(1) as u32;
    let row_h = (content_h - 26) / row_count;
    let max_gain = top_gainers.iter().map(|m| m.change_pct).fold(0.0f64, f64::max);

    for (i, mover) in top_gainers.iter().enumerate() {
        let row_y = section_top + (i as u32) * row_h;
        draw_mover_row(&mut canvas, &font_bold, &font_regular, mover,
            &gainer_images[i], i, row_y, row_h, true, max_gain, 0, half_w);
    }

    // Vertical divider
    draw_filled_rect_mut(&mut canvas,
        Rect::at((WIDTH / 2) as i32, content_top as i32).of_size(2, content_h),
        Rgba([40, 40, 60, 255]));

    // === Right column: Losers ===
    draw_text_mut(&mut canvas, PRICE_DOWN, (right_x + 6) as i32, (content_top + 2) as i32,
        PxScale::from(16.0), &font_bold, "\u{25BC} BIGGEST DECLINERS");

    let loser_count = top_losers.len().max(1) as u32;
    let loser_row_h = (content_h - 26) / loser_count;
    let max_loss = top_losers.iter().map(|m| m.change_pct.abs()).fold(0.0f64, f64::max);

    for (i, mover) in top_losers.iter().enumerate() {
        let row_y = section_top + (i as u32) * loser_row_h;
        draw_mover_row(&mut canvas, &font_bold, &font_regular, mover,
            &loser_images[i], i, row_y, loser_row_h, false, max_loss, right_x, half_w);
    }

    canvas.save(output_path)?;
    Ok(())
}
