use std::path::Path;

use ab_glyph::{FontRef, PxScale};
use image::imageops::FilterType;
use image::{DynamicImage, Rgba, RgbaImage};
use imageproc::drawing::{draw_filled_rect_mut, draw_text_mut};
use imageproc::rect::Rect;

use super::alerts::{CardEntry, GraphicCards};

// Canvas dimensions (Twitter-optimized 16:9)
const WIDTH: u32 = 1200;
const HEIGHT: u32 = 675;

// Colors
const BG: Rgba<u8> = Rgba([18, 18, 30, 255]);
const CARD_BG: Rgba<u8> = Rgba([30, 30, 48, 255]);
const ACCENT: Rgba<u8> = Rgba([99, 102, 241, 255]);
const TEXT_PRIMARY: Rgba<u8> = Rgba([240, 240, 245, 255]);
const TEXT_SECONDARY: Rgba<u8> = Rgba([160, 160, 180, 255]);
const BAR_COLORS: [Rgba<u8>; 5] = [
    Rgba([99, 102, 241, 255]),  // indigo
    Rgba([79, 140, 255, 255]),  // blue
    Rgba([56, 178, 226, 255]),  // cyan
    Rgba([45, 190, 180, 255]),  // teal
    Rgba([60, 200, 150, 255]),  // green
];

// Rarity colors for the "Top by Rarity" section
const RARITY_COLORS: [(&str, Rgba<u8>); 5] = [
    ("Common",   Rgba([140, 140, 155, 255])),   // gray
    ("Uncommon",  Rgba([80, 180, 100, 255])),    // green
    ("Rare",      Rgba([70, 130, 230, 255])),    // blue
    ("Epic",      Rgba([180, 80, 220, 255])),    // purple
    ("Showcase",  Rgba([230, 170, 50, 255])),    // gold
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
    let url = format!("{}/200x200/{}.jpg", CDN_BASE, product_id);
    let resp = client.get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let bytes = resp.bytes().await.ok()?;
    image::load_from_memory(&bytes).ok()
}

/// Resize and crop an image to fill a target area (cover mode).
fn resize_cover(img: &DynamicImage, target_w: u32, target_h: u32) -> RgbaImage {
    let (iw, ih) = (img.width() as f32, img.height() as f32);
    let scale = (target_w as f32 / iw).max(target_h as f32 / ih);
    let new_w = (iw * scale).ceil() as u32;
    let new_h = (ih * scale).ceil() as u32;
    let resized = img.resize_exact(new_w, new_h, FilterType::Lanczos3);
    let crop_x = (new_w.saturating_sub(target_w)) / 2;
    let crop_y = (new_h.saturating_sub(target_h)) / 2;
    resized
        .crop_imm(crop_x, crop_y, target_w, target_h)
        .to_rgba8()
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
    show_rarity: bool,
) {
    let center_y = row_y + row_h / 2;
    let thumb_size = (row_h - 12).min(80);

    // Alternating row background
    if rank % 2 == 0 {
        draw_filled_rect_mut(
            canvas,
            Rect::at(0, row_y as i32).of_size(WIDTH, row_h),
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

    // Card art thumbnail
    let thumb_y = center_y - thumb_size / 2;
    draw_filled_rect_mut(
        canvas,
        Rect::at(thumb_x as i32, thumb_y as i32).of_size(thumb_size, thumb_size),
        CARD_BG,
    );
    if let Some(ref img) = image {
        let fitted = resize_cover(img, thumb_size, thumb_size);
        image::imageops::overlay(canvas, &fitted, thumb_x as i64, thumb_y as i64);
    }
    // Thumbnail border
    let border_color = BAR_COLORS[rank % BAR_COLORS.len()];
    draw_filled_rect_mut(canvas, Rect::at(thumb_x as i32, thumb_y as i32).of_size(thumb_size, 2), border_color);
    draw_filled_rect_mut(canvas, Rect::at(thumb_x as i32, (thumb_y + thumb_size - 2) as i32).of_size(thumb_size, 2), border_color);
    draw_filled_rect_mut(canvas, Rect::at(thumb_x as i32, thumb_y as i32).of_size(2, thumb_size), border_color);
    draw_filled_rect_mut(canvas, Rect::at((thumb_x + thumb_size - 2) as i32, thumb_y as i32).of_size(2, thumb_size), border_color);

    // Card name
    let name = truncate(&card.product_name, 38);
    draw_text_mut(
        canvas,
        TEXT_PRIMARY,
        bar_x as i32,
        (center_y - 24) as i32,
        PxScale::from(18.0),
        font_bold,
        &name,
    );

    // Rarity label (if showing by-rarity section)
    if show_rarity && !card.rarity.is_empty() {
        let name_width = name.len() as u32 * 10 + 8;
        let rc = rarity_color(&card.rarity);
        draw_text_mut(
            canvas,
            rc,
            (bar_x + name_width) as i32,
            (center_y - 22) as i32,
            PxScale::from(14.0),
            font_regular,
            &card.rarity,
        );
    }

    // Horizontal bar
    let bar_w = if max_qty > 0 {
        (card.total_qty as f64 / max_qty as f64 * bar_max_w as f64) as u32
    } else {
        0
    };
    let bar_h = 22u32;
    let bar_y = center_y + 2;

    let bar_color = if show_rarity {
        rarity_color(&card.rarity)
    } else {
        BAR_COLORS[rank % BAR_COLORS.len()]
    };
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
            total_qty: c.total_qty,
            rarity: c.rarity.clone(),
            fallback_product_id: c.fallback_product_id,
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
    let thumb_size = (row_h - 12).min(80);
    let bar_x = thumb_x + thumb_size + 12;
    let bar_max_w = left_w - bar_x - 90;
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
            false,
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
        let r_thumb_size = (rarity_row_h - 12).min(80);
        let r_bar_x = r_thumb_x + r_thumb_size + 12;
        let r_bar_max_w = WIDTH - r_bar_x - 90;
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

            // Rarity badge
            let rc = rarity_color(&card.rarity);
            let badge_w = card.rarity.len() as u32 * 8 + 12;
            draw_filled_rect_mut(
                &mut canvas,
                Rect::at((r_thumb_x - 2) as i32, (center_y - 10) as i32).of_size(badge_w, 20),
                Rgba([rc.0[0], rc.0[1], rc.0[2], 50]),
            );

            // Card art thumbnail
            let t_size = r_thumb_size;
            let thumb_y = center_y - t_size / 2;
            draw_filled_rect_mut(
                &mut canvas,
                Rect::at(r_thumb_x as i32, thumb_y as i32).of_size(t_size, t_size),
                CARD_BG,
            );
            if let Some(ref img) = rarity_images[i] {
                let fitted = resize_cover(img, t_size, t_size);
                image::imageops::overlay(&mut canvas, &fitted, r_thumb_x as i64, thumb_y as i64);
            }
            // Rarity-colored border
            draw_filled_rect_mut(&mut canvas, Rect::at(r_thumb_x as i32, thumb_y as i32).of_size(t_size, 2), rc);
            draw_filled_rect_mut(&mut canvas, Rect::at(r_thumb_x as i32, (thumb_y + t_size - 2) as i32).of_size(t_size, 2), rc);
            draw_filled_rect_mut(&mut canvas, Rect::at(r_thumb_x as i32, thumb_y as i32).of_size(2, t_size), rc);
            draw_filled_rect_mut(&mut canvas, Rect::at((r_thumb_x + t_size - 2) as i32, thumb_y as i32).of_size(2, t_size), rc);

            // Card name
            let name = truncate(&card.product_name, 28);
            draw_text_mut(
                &mut canvas,
                TEXT_PRIMARY,
                r_bar_x as i32,
                (center_y - 24) as i32,
                PxScale::from(16.0),
                &font_bold,
                &name,
            );

            // Rarity label
            draw_text_mut(
                &mut canvas,
                rc,
                (r_bar_x + name.len() as u32 * 9 + 6) as i32,
                (center_y - 22) as i32,
                PxScale::from(13.0),
                &font_regular,
                &card.rarity,
            );

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
