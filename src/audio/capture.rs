use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{
    Device, FromSample, I24, Sample, SampleFormat, SizedSample, Stream, StreamConfig,
    SupportedStreamConfig, U24,
};
use crossbeam_channel::Sender;

use crate::types::{AudioChunk, SourceKind};

/// Builds and starts an input stream that sends mono f32 chunks at the device rate.
pub fn start_stream(
    device: &Device,
    config: SupportedStreamConfig,
    source: SourceKind,
    tx: Sender<AudioChunk>,
) -> anyhow::Result<Stream> {
    let cfg = config.config();
    let stream = match config.sample_format() {
        SampleFormat::I8 => build::<i8>(device, cfg, source, tx),
        SampleFormat::I16 => build::<i16>(device, cfg, source, tx),
        SampleFormat::I24 => build::<I24>(device, cfg, source, tx),
        SampleFormat::I32 => build::<i32>(device, cfg, source, tx),
        SampleFormat::I64 => build::<i64>(device, cfg, source, tx),
        SampleFormat::U8 => build::<u8>(device, cfg, source, tx),
        SampleFormat::U16 => build::<u16>(device, cfg, source, tx),
        SampleFormat::U24 => build::<U24>(device, cfg, source, tx),
        SampleFormat::U32 => build::<u32>(device, cfg, source, tx),
        SampleFormat::U64 => build::<u64>(device, cfg, source, tx),
        SampleFormat::F32 => build::<f32>(device, cfg, source, tx),
        SampleFormat::F64 => build::<f64>(device, cfg, source, tx),
        f => anyhow::bail!("Unsupported sample format {f} on {device}"),
    }?;
    stream.play()?;
    log::info!(
        "{source:?} capture: {device}, {} Hz, {} ch, {}",
        cfg.sample_rate,
        cfg.channels,
        config.sample_format()
    );
    Ok(stream)
}

fn build<T>(
    device: &Device,
    cfg: StreamConfig,
    source: SourceKind,
    tx: Sender<AudioChunk>,
) -> anyhow::Result<Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = cfg.channels.max(1) as usize;
    let sample_rate = cfg.sample_rate;
    let stream = device.build_input_stream(
        cfg,
        move |data: &[T], _: &_| {
            let samples = downmix(data, channels);
            // Full channel means the engine is behind; dropping is the intended backpressure.
            let _ = tx.try_send(AudioChunk {
                source,
                samples,
                sample_rate,
            });
        },
        // WASAPI flags a discontinuity on the first packet of every stream, so xruns are noise.
        move |e: cpal::Error| match e.kind() {
            cpal::ErrorKind::Xrun => log::debug!("{source:?} capture: {e}"),
            _ => log::warn!("{source:?} capture error: {e}"),
        },
        None,
    )?;
    Ok(stream)
}

fn downmix<T>(data: &[T], channels: usize) -> Vec<f32>
where
    T: Sample,
    f32: FromSample<T>,
{
    if channels == 1 {
        return data.iter().map(|&s| f32::from_sample(s)).collect();
    }
    let scale = 1.0 / channels as f32;
    data.chunks_exact(channels)
        .map(|f| f.iter().map(|&s| f32::from_sample(s)).sum::<f32>() * scale)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_formats() {
        assert_eq!(
            downmix(&[i16::MAX, i16::MAX, 0, 0], 2),
            vec![1.0f32 - 1.0 / 32768.0, 0.0]
        );
        assert_eq!(downmix(&[128u8, 0], 1), vec![0.0, -1.0]);
        assert_eq!(downmix(&[0.5f32, -0.5, 1.0, 1.0], 2), vec![0.0, 1.0]);
    }
}
