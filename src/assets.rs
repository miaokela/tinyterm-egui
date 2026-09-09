//! Image assets: the TinyTerm app icon (window/dock icon) and the logo used by
//! the empty states. Both are the original TinyTerm artwork, vendored from the
//! Tauri project (`src-tauri/icons/icon.png` and `public/assets/logo.png`).

/// Decode a PNG into `(width, height, RGBA8)`.
pub fn decode_png(bytes: &[u8]) -> anyhow::Result<(usize, usize, Vec<u8>)> {
    // `Decoder` needs `BufRead + Seek`; `&[u8]` only implements `BufRead`.
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info()?;
    // png 0.18 returns `Option<usize>` here (None = would not fit in memory).
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| anyhow::anyhow!("PNG is too large to decode"))?;
    let mut buffer = vec![0; size];
    let info = reader.next_frame(&mut buffer)?;
    buffer.truncate(info.buffer_size());

    // The vendored assets are RGBA8; convert the rare other layouts so the
    // caller can always treat the result as RGBA.
    let rgba = match info.color_type {
        png::ColorType::Rgba => buffer,
        png::ColorType::Rgb => buffer
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::Grayscale => buffer
            .iter()
            .flat_map(|&v| [v, v, v, 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buffer
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Indexed => {
            anyhow::bail!("indexed PNGs are not supported for the app icon")
        }
    };

    Ok((info.width as usize, info.height as usize, rgba))
}

/// Window / dock icon (512×512 RGBA).
pub fn window_icon() -> egui::IconData {
    match decode_png(include_bytes!("../assets/icon.png")) {
        Ok((width, height, rgba)) => egui::IconData {
            rgba,
            width: width as u32,
            height: height as u32,
        },
        Err(e) => {
            log::warn!("failed to decode the app icon: {e}");
            // A 1×1 transparent icon is better than failing to start.
            egui::IconData {
                rgba: vec![0, 0, 0, 0],
                width: 1,
                height: 1,
            }
        }
    }
}

/// Logo texture used by the empty states.
pub fn logo_texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    match decode_png(include_bytes!("../assets/logo.png")) {
        Ok((width, height, rgba)) => {
            let image = egui::ColorImage::from_rgba_unmultiplied([width, height], &rgba);
            Some(ctx.load_texture(
                "tinyterm-logo",
                image,
                egui::TextureOptions::LINEAR,
            ))
        }
        Err(e) => {
            log::warn!("failed to decode the logo: {e}");
            None
        }
    }
}
