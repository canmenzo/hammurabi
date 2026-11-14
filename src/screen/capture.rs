//! A window or a display as pictures, through Windows.Graphics.Capture: it also gives a window
//! that others cover, and takes nothing from the program it reads.

use anyhow::{Context, anyhow};
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::UI::WindowsAndMessaging::{IsIconic, IsWindow};
use windows::core::{Interface, factory};

use super::Area;

/// Rows of pixels, four bytes each: blue, green, red, alpha.
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub bgra: Vec<u8>,
}

pub enum Target {
    Window(HWND),
    Display(HMONITOR),
}

pub struct Capture {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    shared: IDirect3DDevice,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    size: SizeInt32,
    /// The window captured, when it is one.
    window: Option<HWND>,
    /// Where the part to read is copied to, and its size.
    staging: Option<(ID3D11Texture2D, u32, u32)>,
}

impl Capture {
    pub fn start(target: &Target) -> anyhow::Result<Self> {
        let (mut device, mut context) = (None, None);
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        }
        .context("no Direct3D device")?;
        let device: ID3D11Device = device.ok_or_else(|| anyhow!("no Direct3D device"))?;
        let context = context.ok_or_else(|| anyhow!("no Direct3D context"))?;
        let dxgi: IDXGIDevice = device.cast()?;
        let shared: IDirect3DDevice =
            unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }?.cast()?;
        let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        let item: GraphicsCaptureItem = match *target {
            Target::Window(w) => unsafe { interop.CreateForWindow(w) },
            Target::Display(m) => unsafe { interop.CreateForMonitor(m) },
        }
        .context("this cannot be captured")?;
        let size = item.Size()?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &shared,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            1,
            size,
        )?;
        let session = pool.CreateCaptureSession(&item)?;
        // Neither exists before Windows 10 2004 and Windows 11: the cursor and a yellow frame
        // are then in the picture, which reading the bottom of it survives.
        let _ = session.SetIsCursorCaptureEnabled(false);
        let _ = session.SetIsBorderRequired(false);
        session.StartCapture()?;
        Ok(Self {
            device,
            context,
            shared,
            pool,
            session,
            size,
            window: match *target {
                Target::Window(w) => Some(w),
                Target::Display(_) => None,
            },
            staging: None,
        })
    }

    /// That part of the newest picture, or None when nothing was drawn since the last call.
    pub fn part(&mut self, area: Area) -> anyhow::Result<Option<Image>> {
        let mut newest = None;
        while let Ok(f) = self.pool.TryGetNextFrame() {
            newest = Some(f);
        }
        let Some(frame) = newest else {
            return Ok(None);
        };
        let size = frame.ContentSize()?;
        if size != self.size && size.Width > 0 && size.Height > 0 {
            // The window was resized: pictures of the new size come after this.
            self.size = size;
            self.pool.Recreate(
                &self.shared,
                DirectXPixelFormat::B8G8R8A8UIntNormalized,
                1,
                size,
            )?;
            return Ok(None);
        }
        let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
        let texture: ID3D11Texture2D = unsafe { access.GetInterface() }?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut desc) };
        let width = desc.Width.min(size.Width as u32);
        let height = desc.Height.min(size.Height as u32);
        // From and to in pixels, of `len`: at least one of them.
        let span = |from: f32, to: f32, len: u32| {
            let a = ((from.clamp(0.0, 1.0) * len as f32) as u32).min(len.saturating_sub(1));
            (
                a,
                ((to.clamp(0.0, 1.0) * len as f32).ceil() as u32).clamp(a + 1, len),
            )
        };
        let (x0, x1) = span(area.left, area.right, width);
        let (y0, y1) = span(area.top, area.bottom, height);
        let (cols, rows) = (x1 - x0, y1 - y0);
        if self
            .staging
            .as_ref()
            .is_none_or(|&(_, w, h)| (w, h) != (cols, rows))
        {
            let desc = D3D11_TEXTURE2D_DESC {
                Width: cols,
                Height: rows,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
            };
            let mut t = None;
            unsafe { self.device.CreateTexture2D(&desc, None, Some(&mut t)) }?;
            self.staging = t.map(|t| (t, cols, rows));
        }
        let (staging, ..) = self
            .staging
            .as_ref()
            .ok_or_else(|| anyhow!("no texture to read from"))?;
        let from = D3D11_BOX {
            left: x0,
            top: y0,
            front: 0,
            right: x1,
            bottom: y1,
            back: 1,
        };
        let (w, h) = (cols as usize, rows as usize);
        let mut bgra = vec![0u8; w * h * 4];
        unsafe {
            self.context
                .CopySubresourceRegion(staging, 0, 0, 0, 0, &texture, 0, Some(&from));
            let mut map = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut map))?;
            for y in 0..h {
                let row = std::slice::from_raw_parts(
                    (map.pData as *const u8).add(y * map.RowPitch as usize),
                    w * 4,
                );
                bgra[y * w * 4..(y + 1) * w * 4].copy_from_slice(row);
            }
            self.context.Unmap(staging, 0);
        }
        Ok(Some(Image {
            width: w,
            height: h,
            bgra,
        }))
    }

    /// False once the window is gone.
    pub fn alive(&self) -> bool {
        self.window
            .is_none_or(|w| unsafe { IsWindow(Some(w)) }.as_bool())
    }

    /// A minimized window is not drawn, so no picture of it comes.
    pub fn minimized(&self) -> bool {
        self.window
            .is_some_and(|w| unsafe { IsIconic(w) }.as_bool())
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}
