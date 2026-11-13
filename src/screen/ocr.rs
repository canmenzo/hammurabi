//! Windows' own text recognition (Windows.Media.Ocr): a few milliseconds for a band of a
//! picture, on the CPU, nothing to download where the language is installed.

use anyhow::anyhow;
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;
use windows::core::HSTRING;

use super::reader::Line;

pub struct Ocr {
    engine: OcrEngine,
}

impl Ocr {
    /// `lang`: a tag like "en" or "en-US". Fails when Windows has no recognizer for it.
    pub fn new(lang: &str) -> anyhow::Result<Self> {
        let tag = if lang.contains('-') || lang != "en" {
            lang.to_string()
        } else {
            "en-US".to_string()
        };
        let language = Language::CreateLanguage(&HSTRING::from(tag.as_str()))?;
        let engine = OcrEngine::TryCreateFromLanguage(&language)
            .ok()
            .or_else(|| OcrEngine::TryCreateFromUserProfileLanguages().ok())
            .ok_or_else(|| anyhow!("Windows has no text recognition for {tag}"))?;
        Ok(Self { engine })
    }

    /// The lines in a picture of one byte a pixel, `width` by `height`.
    pub fn read(&self, grey: &[u8], width: usize, height: usize) -> anyhow::Result<Vec<Line>> {
        let writer = DataWriter::new()?;
        writer.WriteBytes(grey)?;
        let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
            &writer.DetachBuffer()?,
            BitmapPixelFormat::Gray8,
            width as i32,
            height as i32,
        )?;
        let result = self.engine.RecognizeAsync(&bitmap)?.join()?;
        let mut lines = Vec::new();
        for line in result.Lines()? {
            let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, 0f32, 0f32);
            let mut words = Vec::new();
            for word in line.Words()? {
                let rect = word.BoundingRect()?;
                x0 = x0.min(rect.X);
                y0 = y0.min(rect.Y);
                x1 = x1.max(rect.X + rect.Width);
                y1 = y1.max(rect.Y + rect.Height);
                words.push((word.Text()?.to_string(), rect.X, rect.X + rect.Width));
            }
            if !words.is_empty() {
                lines.push(Line {
                    text: line.Text()?.to_string(),
                    x0,
                    y0,
                    x1,
                    y1,
                    words,
                });
            }
        }
        Ok(lines)
    }
}
