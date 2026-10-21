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

/// Pads (or truncates) to 30 s and returns a `(1, n_mels, 3000)` tensor.
pub fn mel_tensor(
    cfg: &Config,
    pcm: &[f32],
    filters: &[f32],
    device: &Device,
) -> anyhow::Result<Tensor> {
    let mut samples = pcm[..pcm.len().min(m::N_SAMPLES)].to_vec();
    samples.resize(m::N_SAMPLES, 0.0);
    let mel = audio::pcm_to_mel(cfg, &samples, filters);
    let n_mels = cfg.num_mel_bins;
    // pcm_to_mel appends an extra padding chunk; keep the first 3000 frames.
    let frames = mel.len() / n_mels;
    let mel = Tensor::from_vec(mel, (1, n_mels, frames), device)?;
    Ok(mel.narrow(2, 0, m::N_FRAMES)?)
}
