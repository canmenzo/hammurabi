//! Silero VAD v5: how likely each 32 ms of 16 kHz audio is speech. Unlike a loudness gate it
//! hears talk under music and engines, and ignores loud sounds that are not speech.

use ort::session::Session;
use ort::value::Tensor;

const MODEL: &[u8] = include_bytes!("../../assets/silero_vad.onnx");
const WINDOW: usize = 512;
/// Samples of the previous window the model is given again.
const CONTEXT: usize = 64;
const STATE: usize = 2 * 128;

pub struct Vad {
    session: Session,
    state: Vec<f32>,
    /// `CONTEXT` samples of the last window, then the window being filled.
    input: Vec<f32>,
    filled: usize,
}

impl Vad {
    pub fn new() -> anyhow::Result<Self> {
        let env = ort::init().with_name("hammurabi").build()?;
        let session = Session::builder(&env)?
            .with_intra_threads(1)
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .commit_from_memory(MODEL)?;
        Ok(Self {
            session,
            state: vec![0.0; STATE],
            input: vec![0.0; CONTEXT + WINDOW],
            filled: 0,
        })
    }

    pub fn reset(&mut self) {
        self.state.fill(0.0);
        self.input.fill(0.0);
        self.filled = 0;
    }

    /// Highest speech probability among the windows `frame` completes (0 when it completes none).
    pub fn push(&mut self, mut frame: &[f32]) -> anyhow::Result<f32> {
        let mut max = 0f32;
        while !frame.is_empty() {
            let n = (WINDOW - self.filled).min(frame.len());
            self.input[CONTEXT + self.filled..][..n].copy_from_slice(&frame[..n]);
            self.filled += n;
            frame = &frame[n..];
            if self.filled == WINDOW {
                max = max.max(self.run()?);
                self.input.copy_within(WINDOW.., 0);
                self.filled = 0;
            }
        }
        Ok(max)
    }

    fn run(&mut self) -> anyhow::Result<f32> {
        let out = self.session.run(ort::inputs! {
            "input" => Tensor::from_array(([1, CONTEXT + WINDOW], self.input.clone()))?,
            "state" => Tensor::from_array(([2, 1, 128], std::mem::take(&mut self.state)))?,
            "sr" => Tensor::from_array(((), vec![16_000i64]))?,
        })?;
        self.state = out["stateN"].try_extract_tensor::<f32>()?.1.to_vec();
        Ok(out["output"].try_extract_tensor::<f32>()?.1[0])
    }
}
