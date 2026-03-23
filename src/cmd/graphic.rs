use std::path::Path;

use ab_glyph::{FontRef, PxScale};
use image::imageops::FilterType;
use image::{DynamicImage, Rgba, RgbaImage};
use imageproc::drawing::{draw_filled_rect_mut, draw_text_mut};
use imageproc::rect::Rect;

use super::alerts::CardEntry;

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
    Rgba([99, 102, 241, 255]),  // #1 indigo
    Rgba([79, 140, 255, 255]),  // #2 blue
    Rgba([56, 178, 226, 255]), // #3 cyan
    Rgba([45, 190, 180, 255]), // #4 teal
    Rgba([60, 200, 150, 255]), // #5 green
];

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

/// Generate a polished dark-themed graphic with card art thumbnails and bar chart.
///
/// Layout (1200x675):
/// ```text
/// +--------------------------------------------------+
/// |  Title                                            |
/// |==================================================|
/// |  #1  [thumb]  Card Name                           |
/// |               ██████████████████████████  34      |
/// |                                                   |
/// |  #2  [thumb]  Card Name                           |
/// |               ████████████████████████    33      |
/// |  ...                                              |
/// +--------------------------------------------------+
/// ```
pub async fn generate_graphic(
    cards: &[CardEntry],
    title: &str,
    output_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if cards.is_empty() {
        return Err("No cards to render".into());
    }

    let font_bold = FontRef::try_from_slice(FONT_BOLD)?;
    let font_regular = FontRef::try_from_slice(FONT_REGULAR)?;

    let mut canvas = RgbaImage::from_pixel(WIDTH, HEIGHT, BG);

    // Download card images concurrently
    let client = reqwest::Client::new();
    let top = &cards[..cards.len().min(5)];

    let mut images: Vec<Option<DynamicImage>> = Vec::with_capacity(top.len());
    let mut futures = Vec::new();
    for (i, card) in top.iter().enumerate() {
        let client = client.clone();
        let pid = card.product_id;
        let fallback_pid = card.fallback_product_id;
        futures.push(tokio::spawn(async move {
            let img = download_card_image(&client, pid).await;
            if img.is_some() {
                return (i, img);
            }
            // Try fallback (base card) if variant image not found
            if let Some(fb_pid) = fallback_pid {
                return (i, download_card_image(&client, fb_pid).await);
            }
            (i, None)
        }));
    }
    images.resize_with(top.len(), || None);
    for handle in futures {
        if let Ok((idx, img)) = handle.await {
            images[idx] = img;
        }
    }

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

    // === Bar chart layout ===
    let content_top = 70u32;
    let content_bottom = HEIGHT - 16;
    let content_h = content_bottom - content_top;
    let count = top.len() as u32;
    let row_h = content_h / count;
    let padding = 16u32;

    let thumb_size = (row_h - 16).min(100); // thumbnail square, max 100px
    let thumb_x = padding + 44; // after rank label
    let bar_x = thumb_x + thumb_size + 16; // bars start after thumbnail
    let bar_max_w = WIDTH - bar_x - 100; // leave room for count label

    let max_qty = top.iter().map(|c| c.total_qty).max().unwrap_or(1);

    for (i, card) in top.iter().enumerate() {
        let row_y = content_top + (i as u32) * row_h;
        let center_y = row_y + row_h / 2;

        // Row background (subtle alternating)
        if i % 2 == 0 {
            draw_filled_rect_mut(
                &mut canvas,
                Rect::at(0, row_y as i32).of_size(WIDTH, row_h),
                Rgba([22, 22, 36, 255]),
            );
        }

        // Rank number
        let rank = format!("#{}", i + 1);
        let rank_color = if i == 0 { ACCENT } else { TEXT_SECONDARY };
        draw_text_mut(
            &mut canvas,
            rank_color,
            (padding + 4) as i32,
            (center_y - 12) as i32,
            PxScale::from(24.0),
            &font_bold,
            &rank,
        );

        // Card art thumbnail
        let thumb_y = center_y - thumb_size / 2;
        draw_filled_rect_mut(
            &mut canvas,
            Rect::at(thumb_x as i32, thumb_y as i32).of_size(thumb_size, thumb_size),
            CARD_BG,
        );
        if let Some(ref img) = images[i] {
            let fitted = resize_cover(img, thumb_size, thumb_size);
            image::imageops::overlay(&mut canvas, &fitted, thumb_x as i64, thumb_y as i64);
        }
        // Thumbnail border
        let border_color = BAR_COLORS[i % BAR_COLORS.len()];
        // Top edge
        draw_filled_rect_mut(&mut canvas, Rect::at(thumb_x as i32, thumb_y as i32).of_size(thumb_size, 2), border_color);
        // Bottom edge
        draw_filled_rect_mut(&mut canvas, Rect::at(thumb_x as i32, (thumb_y + thumb_size - 2) as i32).of_size(thumb_size, 2), border_color);
        // Left edge
        draw_filled_rect_mut(&mut canvas, Rect::at(thumb_x as i32, thumb_y as i32).of_size(2, thumb_size), border_color);
        // Right edge
        draw_filled_rect_mut(&mut canvas, Rect::at((thumb_x + thumb_size - 2) as i32, thumb_y as i32).of_size(2, thumb_size), border_color);

        // Card name
        let name = truncate(&card.product_name, 40);
        draw_text_mut(
            &mut canvas,
            TEXT_PRIMARY,
            bar_x as i32,
            (center_y - 28) as i32,
            PxScale::from(20.0),
            &font_bold,
            &name,
        );

        // Horizontal bar
        let bar_w = if max_qty > 0 {
            (card.total_qty as f64 / max_qty as f64 * bar_max_w as f64) as u32
        } else {
            0
        };
        let bar_h = 28u32;
        let bar_y = center_y;

        let bar_color = BAR_COLORS[i % BAR_COLORS.len()];
        draw_filled_rect_mut(
            &mut canvas,
            Rect::at(bar_x as i32, bar_y as i32).of_size(bar_w.max(4), bar_h),
            bar_color,
        );

        // Count label at end of bar
        let qty_label = format!("{}", card.total_qty);
        draw_text_mut(
            &mut canvas,
            TEXT_PRIMARY,
            (bar_x + bar_w + 10) as i32,
            (bar_y + 3) as i32,
            PxScale::from(20.0),
            &font_bold,
            &qty_label,
        );

        // "copies" suffix
        draw_text_mut(
            &mut canvas,
            TEXT_SECONDARY,
            (bar_x + bar_w + 10 + qty_label.len() as u32 * 12 + 4) as i32,
            (bar_y + 5) as i32,
            PxScale::from(16.0),
            &font_regular,
            "copies",
        );
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
