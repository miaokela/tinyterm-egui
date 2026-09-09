//! Minimal TrueType / OpenType vertical-metrics reader.
//!
//! epaint places a row's baseline at `ascent` measured from the row top, while
//! the row height is `ascent - descent + line_gap`. When a font declares a large
//! `lineGap` (Hiragino Sans GB declares a whopping 0.5 em) that entire gap ends
//! up *below* the baseline, so the visible glyphs sit well above the centre of
//! their own line box — which is why labels look vertically off-centre.
//!
//! Reading `hhea` (falling back to `OS/2`) lets us compensate exactly, for
//! whichever CJK font the machine happens to have.

/// Vertical metrics, normalised to the font's em square.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    pub units_per_em: f32,
    pub ascent: f32,
    pub descent: f32,
    pub line_gap: f32,
}

impl Metrics {
    /// `line_gap / units_per_em` — how much extra space the font adds per line.
    pub fn line_gap_em(&self) -> f32 {
        if self.units_per_em <= 0.0 {
            0.0
        } else {
            (self.line_gap / self.units_per_em).max(0.0)
        }
    }

    /// Downward shift (as a fraction of the font size) that re-centres glyphs in
    /// their line box.
    pub fn centring_offset_factor(&self) -> f32 {
        self.line_gap_em() / 2.0
    }
}

fn u16_at(d: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_be_bytes(d.get(o..o + 2)?.try_into().ok()?))
}

fn i16_at(d: &[u8], o: usize) -> Option<i16> {
    Some(i16::from_be_bytes(d.get(o..o + 2)?.try_into().ok()?))
}

fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(o..o + 4)?.try_into().ok()?))
}

/// Offset of the offset table for `face_index` (handles `.ttc` collections).
fn face_offset(data: &[u8], face_index: u32) -> Option<usize> {
    if data.len() < 12 {
        return None;
    }
    if &data[0..4] == b"ttcf" {
        let num_fonts = u32_at(data, 8)?;
        if face_index >= num_fonts {
            return None;
        }
        u32_at(data, 12 + face_index as usize * 4).map(|o| o as usize)
    } else {
        Some(0)
    }
}

fn table_range(data: &[u8], face: usize, tag: &[u8; 4]) -> Option<(usize, usize)> {
    let num_tables = u16_at(data, face + 4)? as usize;
    for i in 0..num_tables {
        let rec = face + 12 + i * 16;
        if data.get(rec..rec + 4)? == tag {
            let offset = u32_at(data, rec + 8)? as usize;
            let length = u32_at(data, rec + 12)? as usize;
            if offset + length <= data.len() {
                return Some((offset, length));
            }
        }
    }
    None
}

/// Read the vertical metrics of one face, or `None` if the file is not a
/// recognisable sfnt font.
pub fn read(data: &[u8], face_index: u32) -> Option<Metrics> {
    let face = face_offset(data, face_index)?;
    let (head, _) = table_range(data, face, b"head")?;
    let units_per_em = u16_at(data, head + 18)? as f32;
    if units_per_em <= 0.0 {
        return None;
    }

    if let Some((hhea, _)) = table_range(data, face, b"hhea") {
        let ascent = i16_at(data, hhea + 4)? as f32;
        let descent = i16_at(data, hhea + 6)? as f32;
        let line_gap = i16_at(data, hhea + 8)? as f32;
        if ascent > 0.0 {
            return Some(Metrics {
                units_per_em,
                ascent,
                descent,
                line_gap,
            });
        }
    }

    // OS/2 fallback (version >= 0 has the typo metrics).
    let (os2, _) = table_range(data, face, b"OS/2")?;
    let ascent = i16_at(data, os2 + 68)? as f32;
    let descent = i16_at(data, os2 + 70)? as f32;
    let line_gap = i16_at(data, os2 + 72)? as f32;
    Some(Metrics {
        units_per_em,
        ascent,
        descent,
        line_gap,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_system_fonts() {
        // macOS-only font paths; on Linux/Windows this test is a no-op so the CI
        // build stays green on all three platforms.
        let candidates = [
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/System/Library/Fonts/STHeiti Medium.ttc",
            "/System/Library/Fonts/Menlo.ttc",
            "/System/Library/Fonts/HelveticaNeue.ttc",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "C:\\Windows\\Fonts\\segoeui.ttf",
        ];
        let mut parsed = 0;
        for path in candidates {
            let Ok(bytes) = std::fs::read(path) else {
                continue;
            };
            let m = read(&bytes, 0).unwrap_or_else(|| panic!("failed to parse {path}"));
            assert!(m.units_per_em > 0.0, "{path}");
            assert!(m.ascent > 0.0, "{path}");
            assert!(m.descent < 0.0, "{path}: descent {}", m.descent);
            parsed += 1;
        }
        if parsed == 0 {
            eprintln!("skipping: no known system font available on this platform");
        }
    }

    #[test]
    fn hiragino_gap_is_compensated() {
        let Ok(bytes) = std::fs::read("/System/Library/Fonts/Hiragino Sans GB.ttc") else {
            return;
        };
        let m = read(&bytes, 0).unwrap();
        assert!((m.line_gap_em() - 0.5).abs() < 0.01, "{:?}", m);
        assert!((m.centring_offset_factor() - 0.25).abs() < 0.01, "{:?}", m);
    }

    #[test]
    fn rejects_garbage() {
        assert!(read(b"not a font at all", 0).is_none());
        assert!(read(&[], 0).is_none());
    }
}
