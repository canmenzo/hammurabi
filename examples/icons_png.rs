use hammurabi::ui::icons::icon_rgba;
use std::io::Write;

fn write_bmp(path: &str, rgba: &[u8], w: u32, h: u32) -> std::io::Result<()> {
    let bg = [0x1Eu8, 0x1E, 0x1E];
    let row = (w * 3) as usize;
    let data_len = row * h as usize;
    let mut f = std::fs::File::create(path)?;
    f.write_all(b"BM")?;
    f.write_all(&((54 + data_len) as u32).to_le_bytes())?;
    f.write_all(&[0; 4])?;
    f.write_all(&54u32.to_le_bytes())?;
    f.write_all(&40u32.to_le_bytes())?;
    f.write_all(&(w as i32).to_le_bytes())?;
    f.write_all(&(h as i32).to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&24u16.to_le_bytes())?;
    f.write_all(&[0; 24])?;
    for y in (0..h).rev() {
        let mut line = Vec::with_capacity(row);
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let a = rgba[i + 3] as u32;
            for c in [2usize, 1, 0] {
                let v = (rgba[i + c] as u32 * a + bg[c] as u32 * (255 - a) + 127) / 255;
                line.push(v as u8);
            }
        }
        f.write_all(&line)?;
    }
    Ok(())
}

fn main() -> std::io::Result<()> {
    let dir = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    for (name, active) in [("idle", false), ("active", true)] {
        let (rgba, w, h) = icon_rgba(active, 64);
        write_bmp(&format!("{dir}/icon_{name}_64.bmp"), &rgba, w, h)?;
    }
    Ok(())
}
