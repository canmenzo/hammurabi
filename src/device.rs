use candle_core::Device;
use candle_core::utils::{cuda_is_available, metal_is_available};

/// CUDA, then Metal, then CPU. `prefer_gpu = false` forces the CPU.
/// Returns the device and a short label for the UI ("CUDA 0", "Metal", "CPU").
pub fn best_device(prefer_gpu: bool) -> (Device, String) {
    if prefer_gpu && cuda_is_available() {
        match Device::new_cuda(0) {
            Ok(d) => {
                candle_core::cuda::set_gemm_reduced_precision_f32(true);
                return (d, "CUDA 0".into());
            }
            Err(e) => log::warn!("CUDA unavailable, falling back: {e}"),
        }
    }
    if prefer_gpu && metal_is_available() {
        match Device::new_metal(0) {
            Ok(d) => return (d, "Metal".into()),
            Err(e) => log::warn!("Metal unavailable, falling back: {e}"),
        }
    }
    (Device::Cpu, "CPU".into())
}

/// The label `best_device(true)` will most likely return, without creating a device.
pub fn label() -> String {
    if cuda_is_available() {
        "CUDA 0".into()
    } else if metal_is_available() {
        "Metal".into()
    } else {
        "CPU".into()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn cpu_when_gpu_not_preferred() {
        let (d, label) = super::best_device(false);
        assert!(d.is_cpu());
        assert_eq!(label, "CPU");
    }
}
