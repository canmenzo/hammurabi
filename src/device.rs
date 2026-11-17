use candle_core::Device;
use candle_core::utils::{cuda_is_available, metal_is_available};

/// CUDA, then Metal, then CPU. `prefer_gpu = false` forces the CPU.
/// Returns the device and a short label for the UI ("CUDA 0", "Metal", "CPU").
pub fn best_device(prefer_gpu: bool) -> (Device, String) {
    if prefer_gpu && cuda_is_available() {
        // The CUDA build's kernels are compiled for one GPU generation; on another, the first
        // kernel fails to load, so try one here instead of failing later.
        let works = |d: &Device| {
            candle_core::Tensor::ones(4, candle_core::DType::F32, d)
                .and_then(|t| t.affine(2.0, 1.0))
                .and_then(|t| t.to_vec1::<f32>())
        };
        match Device::new_cuda(0) {
            Ok(d) => match works(&d) {
                Ok(_) => {
                    candle_core::cuda::set_gemm_reduced_precision_f32(true);
                    return (d, "CUDA 0".into());
                }
                Err(e) => log::warn!("this GPU cannot run the CUDA build, using the CPU: {e}"),
            },
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

/// The graphics card with the most memory of its own, read from the hardware whatever this build
/// computes on: what the models are recommended by.
#[derive(Clone, Debug, PartialEq)]
pub struct Gpu {
    pub name: String,
    /// Dedicated memory in bytes.
    pub vram: u64,
}

impl Gpu {
    /// An NVIDIA RTX 50 series card, the generation the CUDA build is compiled for. "RTX 500" is
    /// an older workstation card.
    pub fn rtx50(&self) -> bool {
        let name = self.name.to_ascii_uppercase();
        let model = |after: &str| {
            name.split(after)
                .nth(1)
                .and_then(|rest| rest.split(' ').next())
                .is_some_and(|n| n.len() == 4 && n.starts_with("50"))
        };
        name.contains("NVIDIA") && (model("RTX ") || name.contains("BLACKWELL"))
    }
}

#[cfg(windows)]
pub fn gpu() -> Option<Gpu> {
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1,
    };
    // Microsoft's own renderer, listed when a driver is missing.
    const MICROSOFT: u32 = 0x1414;
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.ok()?;
    (0..)
        .map_while(|i| unsafe { factory.EnumAdapters1(i) }.ok())
        .filter_map(|a| unsafe { a.GetDesc1() }.ok())
        .filter(|d| d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 == 0 && d.VendorId != MICROSOFT)
        .map(|d| {
            let end = d.Description.iter().position(|&c| c == 0).unwrap_or(128);
            Gpu {
                name: String::from_utf16_lossy(&d.Description[..end])
                    .trim()
                    .into(),
                vram: d.DedicatedVideoMemory as u64,
            }
        })
        .max_by_key(|g| g.vram)
}

#[cfg(not(windows))]
pub fn gpu() -> Option<Gpu> {
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn cpu_when_gpu_not_preferred() {
        let (d, label) = super::best_device(false);
        assert!(d.is_cpu());
        assert_eq!(label, "CPU");
    }

    #[test]
    #[ignore = "needs a GPU build on a machine with that GPU"]
    fn gpu_when_built_for_it() {
        let (_, label) = super::best_device(true);
        assert_ne!(label, "CPU");
    }

    #[test]
    fn knows_rtx50() {
        let gpu = |name: &str| super::Gpu {
            name: name.into(),
            vram: 0,
        };
        assert!(gpu("NVIDIA GeForce RTX 5080").rtx50());
        assert!(gpu("NVIDIA GeForce RTX 5070 Ti Laptop GPU").rtx50());
        assert!(gpu("NVIDIA RTX PRO 6000 Blackwell Workstation Edition").rtx50());
        assert!(!gpu("NVIDIA RTX 500 Ada Generation Laptop GPU").rtx50());
        assert!(!gpu("NVIDIA GeForce RTX 4090").rtx50());
        assert!(!gpu("AMD Radeon RX 7900 XTX").rtx50());
    }

    #[test]
    #[cfg(windows)]
    #[ignore = "prints this machine's graphics card"]
    fn finds_gpu() {
        println!("{:?}", super::gpu());
    }
}
