//! Tray icon: Phosphor "subtitles" (fill, MIT), pre-rasterized to straight RGBA.
//! Files live in assets/icons/ as {idle,active}-{16,32,64}.rgba.
//! app-64.rgba is the window icon: the same glyph on the teal tile of assets/hammurabi.ico.

use std::sync::Arc;

use eframe::egui::IconData;

const IDLE: [(u32, &[u8]); 3] = [
    (16, include_bytes!("../../assets/icons/idle-16.rgba")),
    (32, include_bytes!("../../assets/icons/idle-32.rgba")),
    (64, include_bytes!("../../assets/icons/idle-64.rgba")),
];

const ACTIVE: [(u32, &[u8]); 3] = [
    (16, include_bytes!("../../assets/icons/active-16.rgba")),
    (32, include_bytes!("../../assets/icons/active-32.rgba")),
    (64, include_bytes!("../../assets/icons/active-64.rgba")),
];

/// Embedded icon for `size`: the smallest size >= `size`, else the largest. Returns (rgba, width, height).
pub fn icon_rgba(active: bool, size: u32) -> (Vec<u8>, u32, u32) {
    let set = if active { &ACTIVE } else { &IDLE };
    let (dim, px) = set
        .iter()
        .copied()
        .find(|&(d, _)| d >= size)
        .unwrap_or(set[set.len() - 1]);
    (px.to_vec(), dim, dim)
}

pub fn window_icon() -> Arc<IconData> {
    Arc::new(IconData {
        rgba: include_bytes!("../../assets/icons/app-64.rgba").to_vec(),
        width: 64,
        height: 64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_len_and_shape() {
        for size in [16u32, 32, 64] {
            for active in [false, true] {
                let (buf, w, h) = icon_rgba(active, size);
                assert_eq!((w, h), (size, size));
                assert_eq!(buf.len(), (size * size * 4) as usize);
            }
        }
    }

    #[test]
    fn window_icon_shape() {
        let i = window_icon();
        assert_eq!(i.rgba.len(), (i.width * i.height * 4) as usize);
    }

    #[test]
    fn picks_smallest_size_at_least_requested() {
        assert_eq!(icon_rgba(false, 24).1, 32);
        assert_eq!(icon_rgba(true, 1).1, 16);
        assert_eq!(icon_rgba(true, 512).1, 64);
    }

    #[test]
    fn corners_transparent_middle_opaque() {
        for size in [16u32, 32, 64] {
            for active in [false, true] {
                let (buf, w, _) = icon_rgba(active, size);
                let alpha = |x: u32, y: u32| buf[((y * w + x) * 4 + 3) as usize];
                assert_eq!(alpha(0, 0), 0);
                assert_eq!(alpha(w - 1, 0), 0);
                assert_eq!(alpha(0, w - 1), 0);
                assert_eq!(alpha(w - 1, w - 1), 0);
                let middle_opaque =
                    (w / 4..w * 3 / 4).any(|y| (w / 4..w * 3 / 4).any(|x| alpha(x, y) == 255));
                assert!(middle_opaque, "size {size} active {active}");
            }
        }
    }

    #[test]
    fn colors_match_state() {
        let cases: [(bool, [u8; 3]); 2] = [(false, [0xFF, 0xFF, 0xFF]), (true, [0xF5, 0xB1, 0x3D])];
        for (active, rgb) in cases {
            let (buf, _, _) = icon_rgba(active, 32);
            for px in buf.as_chunks::<4>().0.iter().filter(|p| p[3] > 0) {
                assert_eq!(&px[..3], &rgb);
            }
        }
    }
}
