#![allow(dead_code)]

// Little-endian f32 mel filter banks from candle-examples (see THIRD_PARTY_NOTICES.md).
pub const MEL_FILTERS_80: &[u8] = include_bytes!("../../assets/melfilters.bytes");
pub const MEL_FILTERS_128: &[u8] = include_bytes!("../../assets/melfilters128.bytes");
