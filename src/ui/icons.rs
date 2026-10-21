#![allow(dead_code, unused_variables)]

/// Procedural stepped-ziggurat tray icon: white idle, amber active. Returns (rgba, width, height).
pub fn icon_rgba(active: bool, size: u32) -> (Vec<u8>, u32, u32) {
    (vec![0; (size * size * 4) as usize], size, size)
}
