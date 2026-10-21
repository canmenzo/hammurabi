use candle_core::{Device, Tensor};
use candle_transformers::models::whisper as m;

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

const BINS: usize = m::N_FFT / 2 + 1;

/// Log-mel spectrogram as tensor ops (a DFT matmul), so it runs on the model's device and only
/// for the frames the encoder sees. Matches candle's `audio::pcm_to_mel` (whisper.cpp's
/// algorithm), which FFTs all 4500 frames of 30 s plus padding on the CPU.
pub struct Mel {
    /// `(N_FFT, 2 * BINS)`: Hann-windowed cos | sin basis.
    basis: Tensor,
    /// `(BINS, n_mels)`, with whisper.cpp's folding of the mirrored half (bins 1..N/2 count twice).
    filters: Tensor,
    device: Device,
}

impl Mel {
    pub fn new(num_mel_bins: usize, device: &Device) -> anyhow::Result<Self> {
        let f = filters(num_mel_bins)?;
        let n = m::N_FFT;
        let mut ft = vec![0f32; BINS * num_mel_bins];
        for k in 0..BINS {
            let w = if k == 0 || k == n / 2 { 1.0 } else { 2.0 };
            for j in 0..num_mel_bins {
                ft[k * num_mel_bins + j] = f[j * BINS + k] * w;
            }
        }
        let mut basis = vec![0f32; n * 2 * BINS];
        let tau = std::f64::consts::TAU;
        for i in 0..n {
            let hann = 0.5 * (1.0 - (tau * i as f64 / n as f64).cos());
            for k in 0..BINS {
                let a = tau * ((i * k) % n) as f64 / n as f64;
                basis[i * 2 * BINS + k] = (hann * a.cos()) as f32;
                basis[i * 2 * BINS + BINS + k] = (hann * a.sin()) as f32;
            }
        }
        Ok(Self {
            basis: Tensor::from_vec(basis, (n, 2 * BINS), device)?,
            filters: Tensor::from_vec(ft, (BINS, num_mel_bins), device)?,
            device: device.clone(),
        })
    }

    /// `(1, n_mels, 3000)` for the first 30 s. With `trim`, only the frames covering the audio
    /// plus `TRIM_MARGIN`, at least `TRIM_MIN`, are kept (like whisper.cpp's audio_ctx): the
    /// encoder cost scales with the frame count, and candle narrows the positional embedding to it.
    pub fn tensor(&self, pcm: &[f32], trim: bool) -> anyhow::Result<Tensor> {
        let pcm = &pcm[..pcm.len().min(m::N_SAMPLES)];
        let keep = if trim {
            // Even, since the second conv has stride 2.
            (pcm.len().div_ceil(m::HOP_LENGTH) + TRIM_MARGIN)
                .max(TRIM_MIN)
                .next_multiple_of(2)
                .min(m::N_FRAMES)
        } else {
            m::N_FRAMES
        };
        let mut frames = vec![0f32; keep * m::N_FFT];
        for (i, f) in frames
            .as_chunks_mut::<{ m::N_FFT }>()
            .0
            .iter_mut()
            .enumerate()
        {
            let s = (i * m::HOP_LENGTH).min(pcm.len());
            let e = (s + m::N_FFT).min(pcm.len());
            f[..e - s].copy_from_slice(&pcm[s..e]);
        }
        let frames = Tensor::from_vec(frames, (keep, m::N_FFT), &self.device)?;
        let spec = frames.matmul(&self.basis)?.sqr()?;
        let power = (spec.narrow(1, 0, BINS)? + spec.narrow(1, BINS, BINS)?)?;
        let mel = power
            .matmul(&self.filters)?
            .maximum(1e-10f32)?
            .log()?
            .affine(std::f64::consts::LOG10_E, 0.0)?;
        let floor = (mel.max_keepdim(0)?.max_keepdim(1)? - 8.0)?;
        let mel = mel.broadcast_maximum(&floor)?.affine(0.25, 1.0)?;
        Ok(mel.t()?.contiguous()?.unsqueeze(0)?)
    }
}

#[cfg(test)]
mod tests {
    use candle_core::{Device, IndexOp};
    use candle_transformers::models::whisper::{self as m, audio};

    #[test]
    fn matches_candle_pcm_to_mel() {
        let cfg: m::Config = serde_json::from_str(
            r#"{"num_mel_bins":80,"max_source_positions":1500,"d_model":384,"encoder_attention_heads":6,
            "encoder_layers":4,"vocab_size":51865,"max_target_positions":448,"decoder_attention_heads":6,
            "decoder_layers":4,"suppress_tokens":[]}"#,
        )
        .unwrap();
        let filters = super::filters(80).unwrap();
        // 2.5 s of a chirp plus a little deterministic noise.
        let pcm: Vec<f32> = (0..40_000)
            .map(|i| {
                let t = i as f32 / 16_000.0;
                0.5 * (std::f32::consts::TAU * (200.0 + 900.0 * t) * t).sin()
                    + 0.01 * ((i * 7919 % 101) as f32 / 50.0 - 1.0)
            })
            .collect();
        let mut padded = pcm.clone();
        padded.resize(m::N_SAMPLES, 0.0);
        let want = audio::pcm_to_mel(&cfg, &padded, &filters);
        let frames = want.len() / 80;

        let got = super::Mel::new(80, &Device::Cpu)
            .unwrap()
            .tensor(&pcm, true)
            .unwrap();
        let (_, n_mels, keep) = got.dims3().unwrap();
        assert_eq!((n_mels, keep), (80, super::TRIM_MIN));
        let got = got.i(0).unwrap().to_vec2::<f32>().unwrap();
        let (mut max, mut sum) = (0f32, 0f32);
        for (j, row) in got.iter().enumerate() {
            for (i, v) in row.iter().enumerate() {
                let d = (v - want[j * frames + i]).abs();
                max = max.max(d);
                sum += d;
            }
        }
        let mean = sum / (80 * keep) as f32;
        assert!(mean < 1e-3 && max < 0.05, "mean {mean} max {max}");
    }
}
