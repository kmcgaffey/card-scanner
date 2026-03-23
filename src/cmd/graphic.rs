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
const QTY_BG: Rgba<u8> = Rgba([99, 102, 241, 230]);

// Fonts embedded at compile time
const FONT_BOLD: &[u8] = include_bytes!("../../assets/Inter-Bold.ttf");
#[allow(dead_code)]
const FONT_REGULAR: &[u8] = include_bytes!("../../assets/Inter-Regular.ttf");

// TCGPlayer CDN image URL pattern
const CDN_BASE: &str =
    "https://6d4be195623157e28848-7697ece4918e0a73861de0eb37d08968.ssl.cf1.rackcdn.com";

async fn download_card_image(
    client: &reqwest::Client,
    product_id: u64,
    high_res: bool,
) -> Option<DynamicImage> {
    let suffix = if high_res { "_in_1000x1000" } else { "_200w" };
    let url = format!("{}/{}{}.jpg", CDN_BASE, product_id, suffix);

    let resp = client.get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let bytes = resp.bytes().await.ok()?;
    image::load_from_memory(&bytes).ok()
}

/// Draw a gradient overlay on the bottom portion of a region for text readability.
fn draw_bottom_gradient(canvas: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32) {
    let gradient_h = h / 3;
    let start_y = y + h - gradient_h;
    for row in 0..gradient_h {
        let alpha = (row as f32 / gradient_h as f32 * 220.0) as u8;
        let color = Rgba([18, 18, 30, alpha]);
        draw_filled_rect_mut(
            canvas,
            Rect::at(x as i32, (start_y + row) as i32).of_size(w, 1),
            color,
        );
    }
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

/// Generate a polished dark-themed graphic featuring card art for the top sellers.
///
/// Layout (1200x675):
/// - Title bar at top
/// - Hero card (rank #1) on the left, large
/// - 4 smaller cards in a 2x2 grid on the right
/// - Each card shows: art image, name overlay, copy count badge
pub async fn generate_graphic(
    cards: &[CardEntry],
    title: &str,
    output_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if cards.is_empty() {
        return Err("No cards to render".into());
    }

    let font_bold = FontRef::try_from_slice(FONT_BOLD)?;

    let mut canvas = RgbaImage::from_pixel(WIDTH, HEIGHT, BG);

    // Download card images concurrently
    let client = reqwest::Client::new();
    let top = &cards[..cards.len().min(5)];

    let mut images: Vec<Option<DynamicImage>> = Vec::with_capacity(top.len());
    let mut futures = Vec::new();
    for (i, card) in top.iter().enumerate() {
        let client = client.clone();
        let pid = card.product_id;
        futures.push(tokio::spawn(async move {
            let high_res = i == 0; // Hero card gets high-res
            (i, download_card_image(&client, pid, high_res).await)
        }));
    }
    images.resize_with(top.len(), || None);
    for handle in futures {
        if let Ok((idx, img)) = handle.await {
            images[idx] = img;
        }
    }

    // === Title bar ===
    let title_y = 12u32;
    draw_filled_rect_mut(
        &mut canvas,
        Rect::at(0, 0).of_size(WIDTH, 52),
        Rgba([25, 25, 42, 255]),
    );
    // Accent line under title
    draw_filled_rect_mut(
        &mut canvas,
        Rect::at(0, 52).of_size(WIDTH, 3),
        ACCENT,
    );
    draw_text_mut(
        &mut canvas,
        TEXT_PRIMARY,
        20,
        title_y as i32,
        PxScale::from(28.0),
        &font_bold,
        &truncate(title, 60),
    );

    // === Layout constants ===
    let content_top = 68u32;
    let content_h = HEIGHT - content_top - 12;
    let padding = 12u32;

    // Hero card area (left side)
    let hero_w = 460u32;
    let hero_h = content_h;
    let hero_x = padding;
    let hero_y = content_top;

    // Small cards area (right side, 2x2 grid)
    let grid_x = hero_x + hero_w + padding;
    let grid_w = WIDTH - grid_x - padding;
    let card_w = (grid_w - padding) / 2;
    let card_h = (hero_h - padding) / 2;

    // === Draw hero card (rank #1) ===
    draw_filled_rect_mut(
        &mut canvas,
        Rect::at(hero_x as i32, hero_y as i32).of_size(hero_w, hero_h),
        CARD_BG,
    );
    if let Some(ref img) = images[0] {
        let fitted = resize_cover(img, hero_w, hero_h);
        image::imageops::overlay(&mut canvas, &fitted, hero_x as i64, hero_y as i64);
    }
    draw_bottom_gradient(&mut canvas, hero_x, hero_y, hero_w, hero_h);

    // Hero card name
    let hero_name = truncate(&top[0].product_name, 30);
    draw_text_mut(
        &mut canvas,
        TEXT_PRIMARY,
        (hero_x + 12) as i32,
        (hero_y + hero_h - 62) as i32,
        PxScale::from(24.0),
        &font_bold,
        &hero_name,
    );

    // Hero card qty badge
    let qty_text = format!("{} copies sold", top[0].total_qty);
    let badge_w = qty_text.len() as u32 * 11 + 20;
    draw_filled_rect_mut(
        &mut canvas,
        Rect::at((hero_x + 12) as i32, (hero_y + hero_h - 34) as i32).of_size(badge_w, 26),
        QTY_BG,
    );
    draw_text_mut(
        &mut canvas,
        TEXT_PRIMARY,
        (hero_x + 22) as i32,
        (hero_y + hero_h - 31) as i32,
        PxScale::from(18.0),
        &font_bold,
        &qty_text,
    );

    // === Draw secondary cards (2x2 grid) ===
    let positions = [
        (grid_x, content_top),
        (grid_x + card_w + padding, content_top),
        (grid_x, content_top + card_h + padding),
        (grid_x + card_w + padding, content_top + card_h + padding),
    ];

    for (i, &(cx, cy)) in positions.iter().enumerate() {
        let card_idx = i + 1;
        if card_idx >= top.len() {
            break;
        }

        // Card background
        draw_filled_rect_mut(
            &mut canvas,
            Rect::at(cx as i32, cy as i32).of_size(card_w, card_h),
            CARD_BG,
        );

        // Card image
        if let Some(ref img) = images[card_idx] {
            let fitted = resize_cover(img, card_w, card_h);
            image::imageops::overlay(&mut canvas, &fitted, cx as i64, cy as i64);
        }

        draw_bottom_gradient(&mut canvas, cx, cy, card_w, card_h);

        // Card name
        let name = truncate(&top[card_idx].product_name, 24);
        draw_text_mut(
            &mut canvas,
            TEXT_PRIMARY,
            (cx + 8) as i32,
            (cy + card_h - 50) as i32,
            PxScale::from(16.0),
            &font_bold,
            &name,
        );

        // Qty badge
        let qty = format!("{} sold", top[card_idx].total_qty);
        let badge_w = qty.len() as u32 * 9 + 14;
        draw_filled_rect_mut(
            &mut canvas,
            Rect::at((cx + 8) as i32, (cy + card_h - 28) as i32).of_size(badge_w, 22),
            QTY_BG,
        );
        draw_text_mut(
            &mut canvas,
            TEXT_PRIMARY,
            (cx + 15) as i32,
            (cy + card_h - 26) as i32,
            PxScale::from(14.0),
            &font_bold,
            &qty,
        );

        // Rank indicator
        let rank = format!("#{}", card_idx + 1);
        draw_filled_rect_mut(
            &mut canvas,
            Rect::at(cx as i32, cy as i32).of_size(32, 26),
            ACCENT,
        );
        draw_text_mut(
            &mut canvas,
            TEXT_PRIMARY,
            (cx + 6) as i32,
            (cy + 4) as i32,
            PxScale::from(16.0),
            &font_bold,
            &rank,
        );
    }

    // Rank indicator for hero card
    draw_filled_rect_mut(
        &mut canvas,
        Rect::at(hero_x as i32, hero_y as i32).of_size(36, 30),
        ACCENT,
    );
    draw_text_mut(
        &mut canvas,
        TEXT_PRIMARY,
        (hero_x + 8) as i32,
        (hero_y + 5) as i32,
        PxScale::from(18.0),
        &font_bold,
        "#1",
    );

    // === Bottom bar with branding ===
    let bottom_y = HEIGHT - 3;
    draw_filled_rect_mut(
        &mut canvas,
        Rect::at(0, bottom_y as i32).of_size(WIDTH, 3),
        ACCENT,
    );

    canvas.save(output_path)?;
    Ok(())
}
