//! Render a `vt100` screen to a PNG image, so colors and attributes can be
//! visually inspected. Text reading (`read_screen`) is cheaper and preferred.
//! screenshots are for when color / layout actually matters.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use image::{ImageBuffer, Rgba, RgbaImage};

static FONT_BYTES: &[u8] = include_bytes!("../assets/DejaVuSansMono.ttf");

const CELL_W: u32 = 9;
const CELL_H: u32 = 18;
const CELL_W_F: f32 = 9.0;
const CELL_H_F: f32 = 18.0;
const FONT_PX: f32 = 16.0;

/// The classic xterm 256-color palette entry for an indexed color.
fn idx_to_rgb(idx: u8) -> (u8, u8, u8) {
    match idx {
        0 => (0, 0, 0),
        1 => (205, 0, 0),
        2 => (0, 205, 0),
        3 => (205, 205, 0),
        4 => (0, 0, 238),
        5 => (205, 0, 205),
        6 => (0, 205, 205),
        7 => (229, 229, 229),
        8 => (127, 127, 127),
        9 => (255, 0, 0),
        10 => (0, 255, 0),
        11 => (255, 255, 0),
        12 => (92, 92, 255),
        13 => (255, 0, 255),
        14 => (0, 255, 255),
        15 => (255, 255, 255),
        16..=231 => {
            let c = idx - 16;
            let r = c / 36;
            let g = (c % 36) / 6;
            let b = c % 6;
            let conv = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            (conv(r), conv(g), conv(b))
        }
        _ => {
            let v = 8 + (idx - 232) * 10;
            (v, v, v)
        }
    }
}

fn resolve(color: vt100::Color, default: (u8, u8, u8)) -> (u8, u8, u8) {
    match color {
        vt100::Color::Default => default,
        vt100::Color::Idx(i) => idx_to_rgb(i),
        vt100::Color::Rgb(r, g, b) => (r, g, b),
    }
}

/// Render the visible screen to PNG bytes.
pub fn screen_to_png(screen: &vt100::Screen) -> anyhow::Result<Vec<u8>> {
    let font = FontRef::try_from_slice(FONT_BYTES).expect("embedded font is valid");
    let scaled = font.as_scaled(PxScale::from(FONT_PX));
    let ascent = scaled.ascent();

    let (rows, cols) = screen.size();
    let width = u32::from(cols) * CELL_W;
    let height = u32::from(rows) * CELL_H;
    let mut img: RgbaImage = ImageBuffer::from_pixel(width, height, Rgba([0, 0, 0, 255]));

    let ink = (229, 229, 229);
    let paper = (0, 0, 0);

    for row in 0..rows {
        for col in 0..cols {
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            let mut fg = resolve(cell.fgcolor(), ink);
            let mut bg = resolve(cell.bgcolor(), paper);
            if cell.inverse() {
                std::mem::swap(&mut fg, &mut bg);
            }

            let x0 = u32::from(col) * CELL_W;
            let y0 = u32::from(row) * CELL_H;

            // Background fill.
            if bg != paper {
                for y in y0..(y0 + CELL_H).min(height) {
                    for x in x0..(x0 + CELL_W).min(width) {
                        img.put_pixel(x, y, Rgba([bg.0, bg.1, bg.2, 255]));
                    }
                }
            }

            // Glyph.
            let text = cell.contents();
            let Some(ch) = text.chars().next() else {
                continue;
            };
            if ch == ' ' || ch == '\0' {
                continue;
            }
            let params = GlyphParams {
                x0: f32::from(col) * CELL_W_F,
                y0: f32::from(row) * CELL_H_F,
                ascent,
                fg,
                width,
                height,
            };
            draw_glyph(&mut img, &font, ch, &params);
        }
    }

    let mut out = Vec::new();
    {
        use image::ImageEncoder;
        let encoder = image::codecs::png::PngEncoder::new(&mut out);
        encoder.write_image(&img, width, height, image::ExtendedColorType::Rgba8)?;
    }
    Ok(out)
}

/// Placement and color for rasterizing a single glyph into the image.
/// `x0`/`y0` are the cell's top-left pixel position (already in `f32`).
struct GlyphParams {
    x0: f32,
    y0: f32,
    ascent: f32,
    fg: (u8, u8, u8),
    width: u32,
    height: u32,
}

/// Clamp an integer pixel coordinate into `0..max`, or `None` if out of range.
fn pixel_index(coord: i64, max: u32) -> Option<u32> {
    let i = u32::try_from(coord).ok()?;
    (i < max).then_some(i)
}

/// Round a float pixel value to the nearest whole pixel. This is the single
/// float-to-integer conversion in the rasterizer: [`ab_glyph`] hands us `f32`
/// coverage/positions and std has no lossless `f32`-to-integer conversion, so
/// the rounding is deliberate. Callers range-check via [`pixel_index`] and
/// `u16::try_from`.
#[allow(clippy::cast_possible_truncation)]
fn round_f32(v: f32) -> i64 {
    v.round() as i64
}

/// Alpha-blend one 8-bit color channel using integer math (`alpha` is 0..=256).
fn blend_channel(fg: u8, bg: u8, alpha: u16) -> u8 {
    let mixed = (u16::from(fg) * alpha + u16::from(bg) * (256 - alpha)) / 256;
    u8::try_from(mixed.min(255)).unwrap_or(255)
}

fn draw_glyph(img: &mut RgbaImage, font: &FontRef, ch: char, p: &GlyphParams) {
    let scaled = font.as_scaled(PxScale::from(FONT_PX));
    let glyph = scaled.scaled_glyph(ch);
    let Some(outlined) = font.outline_glyph(glyph) else {
        return;
    };
    let bounds = outlined.px_bounds();
    // Round the glyph origin to whole pixels once, then offset by the integer
    // coverage coordinates, keeping all per-pixel work in integer space.
    let base_x = round_f32(p.x0 + bounds.min.x);
    let base_y = round_f32(p.y0 + p.ascent + bounds.min.y);
    outlined.draw(|gx, gy, coverage| {
        if coverage <= 0.01 {
            return;
        }
        let Some(px) = pixel_index(base_x + i64::from(gx), p.width) else {
            return;
        };
        let Some(py) = pixel_index(base_y + i64::from(gy), p.height) else {
            return;
        };
        // Coverage 0.0..=1.0 -> integer alpha 0..=256 for integer blending.
        let alpha =
            u16::try_from(round_f32(coverage.clamp(0.0, 1.0) * 256.0).clamp(0, 256)).unwrap_or(256);
        let bg = img.get_pixel(px, py).0;
        img.put_pixel(
            px,
            py,
            Rgba([
                blend_channel(p.fg.0, bg[0], alpha),
                blend_channel(p.fg.1, bg[1], alpha),
                blend_channel(p.fg.2, bg[2], alpha),
                255,
            ]),
        );
    });
}
