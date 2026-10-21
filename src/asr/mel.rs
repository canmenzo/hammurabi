use candle_core::{Device, Tensor};
use candle_transformers::models::whisper::{self as m, Config, audio};

// Little-endian f32 mel filter banks from candle-examples (see THIRD_PARTY_NOTICES.md).
pub const MEL_FILTERS_80: &[u8] = include_bytes!("../../assets/melfilters.bytes");
pub const MEL_FILTERS_128: &[u8] = include_bytes!("../../assets/melfilters128.bytes");

pub fn filters(num_mel_bins: usize) -> anyhow::Result<Vec<f32>> {
    let bytes = match num_mel_bins {
        80 => MEL_FILTERS_80,
        128 => MEL_FILTERS_128,
        n => anyhow::bail!("unsupported num_mel_bins {n}"),
    };
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect())
}

/// Mel frames kept after the audio when trimming (1 frame = 10 ms).
pub const TRIM_MARGIN: usize = 100;
/// Shorter windows make Whisper loop ("Today we are testing. Today we are testing. ...").
pub const TRIM_MIN: usize = 1000;

/// Pads (or truncates) to 30 s and returns a `(1, n_mels, 3000)` tensor. With `trim`, only the
/// frames covering the audio plus `TRIM_MARGIN`, at least `TRIM_MIN`, are kept (like whisper.cpp's audio_ctx): the
/// encoder cost scales with the frame count, and candle narrows the positional embedding to it.
pub fn mel_tensor(
    cfg: &Config,
    pcm: &[f32],
    filters: &[f32],
    device: &Device,
    trim: bool,
) -> anyhow::Result<Tensor> {
    let mut samples = pcm[..pcm.len().min(m::N_SAMPLES)].to_vec();
    samples.resize(m::N_SAMPLES, 0.0);
    let mel = audio::pcm_to_mel(cfg, &samples, filters);
    let n_mels = cfg.num_mel_bins;
    // pcm_to_mel appends an extra padding chunk; keep the first 3000 frames.
    let frames = mel.len() / n_mels;
    let mel = Tensor::from_vec(mel, (1, n_mels, frames), device)?;
    let keep = if trim {
        // Even, since the second conv has stride 2.
        (pcm.len().div_ceil(m::HOP_LENGTH) + TRIM_MARGIN)
            .max(TRIM_MIN)
            .next_multiple_of(2)
    } else {
        m::N_FRAMES
    };
    Ok(mel.narrow(2, 0, keep.min(m::N_FRAMES))?)
}
