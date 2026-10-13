//! Windows Graphics Capture with bounded event delivery and RAII COM/GPU lifetime.
use crate::{Result, WindowIdentity};
use std::{
    marker::PhantomData,
    rc::Rc,
    sync::mpsc::{self, Receiver, SyncSender},
    time::Duration,
};
use windows::{
    Foundation::TypedEventHandler,
    Graphics::{
        Capture::*,
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
    },
    Win32::{
        Foundation::{HMODULE, HWND},
        Graphics::{Direct3D::D3D_DRIVER_TYPE_HARDWARE, Direct3D11::*, Dxgi::IDXGIDevice},
        System::WinRT::{
            Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
            Graphics::Capture::IGraphicsCaptureItemInterop,
            RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize,
        },
    },
    core::{Interface, factory},
};
#[derive(Debug)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
    pub relative_time: i64,
}
const MAX_FRAME_BYTES: u64 = 64 << 20;
fn extent(width: i32, height: i32) -> Result<()> {
    if width <= 0
        || height <= 0
        || width > 16384
        || height > 16384
        || width as u64 * height as u64 * 4 > MAX_FRAME_BYTES
    {
        return Err("capture.extent_budget".into());
    }
    Ok(())
}
#[derive(Default)]
struct StartingCapture {
    pool: Option<Direct3D11CaptureFramePool>,
    session: Option<GraphicsCaptureSession>,
}
impl Drop for StartingCapture {
    fn drop(&mut self) {
        if let Some(s) = &self.session {
            let _ = s.Close();
        }
        if let Some(p) = &self.pool {
            let _ = p.Close();
        }
    }
}
struct Apartment {
    _thread: PhantomData<Rc<()>>,
}
impl Apartment {
    fn open() -> Result<Self> {
        unsafe {
            RoInitialize(RO_INIT_MULTITHREADED).map_err(|e| e.to_string())?;
        }
        Ok(Self {
            _thread: PhantomData,
        })
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe {
            RoUninitialize();
        }
    }
}
pub struct CaptureStream {
    target: WindowIdentity,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    events: Receiver<()>,
    token: i64,
    closed: bool,
    signal_only: bool,
    _apartment: Apartment,
}
impl CaptureStream {
    pub fn open(target: WindowIdentity) -> Result<Self> {
        Self::open_mode(target, false)
    }
    pub fn open_signal(target: WindowIdentity) -> Result<Self> {
        Self::open_mode(target, true)
    }
    fn open_mode(target: WindowIdentity, signal_only: bool) -> Result<Self> {
        target.verify()?;
        let apartment = Apartment::open()?;
        // Retain the process MTA against WGC returning worker threads during final teardown.
        retain_mta()?;
        let interop: IGraphicsCaptureItemInterop =
            factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
                .map_err(|e| e.to_string())?;
        let item: GraphicsCaptureItem =
            unsafe { interop.CreateForWindow(HWND(target.hwnd as *mut _)) }
                .map_err(|e| e.to_string())?;
        let size = item.Size().map_err(|e| e.to_string())?;
        extent(size.Width, size.Height)?;
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
            .map_err(|e| e.to_string())?;
        }
        let device = device.ok_or("capture.device")?;
        let context = context.ok_or("capture.context")?;
        let dxgi: IDXGIDevice = device.cast().map_err(|e| e.to_string())?;
        let runtime: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }
            .map_err(|e| e.to_string())?
            .cast()
            .map_err(|e| e.to_string())?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &runtime,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            size,
        )
        .map_err(|e| e.to_string())?;
        let mut starting = StartingCapture {
            pool: Some(pool.clone()),
            session: None,
        };
        let (tx, events) = mpsc::sync_channel(1);
        let token = pool
            .FrameArrived(&TypedEventHandler::new(move |_, _| {
                signal(&tx);
                Ok(())
            }))
            .map_err(|e| e.to_string())?;
        let session = pool
            .CreateCaptureSession(&item)
            .map_err(|e| e.to_string())?;
        starting.session = Some(session.clone());
        session.StartCapture().map_err(|e| e.to_string())?;
        starting.session.take();
        starting.pool.take();
        Ok(Self {
            target,
            pool,
            session,
            device,
            context,
            events,
            token,
            closed: false,
            signal_only,
            _apartment: apartment,
        })
    }
    pub fn next(&self, timeout: Duration) -> Result<Frame> {
        if self.closed {
            return Err("capture.closed".into());
        }
        self.target.verify()?;
        self.events
            .recv_timeout(timeout)
            .map_err(|e| format!("capture.frame_wait:{e}"))?;
        let frame = self.pool.TryGetNextFrame().map_err(|e| e.to_string())?;
        let result = (|| -> Result<Frame> {
            let size = frame.ContentSize().map_err(|e| e.to_string())?;
            extent(size.Width, size.Height)?;
            let surface = frame.Surface().map_err(|e| e.to_string())?;
            let access: IDirect3DDxgiInterfaceAccess = surface.cast().map_err(|e| e.to_string())?;
            let texture: ID3D11Texture2D =
                unsafe { access.GetInterface() }.map_err(|e| e.to_string())?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            unsafe {
                texture.GetDesc(&mut desc);
            }
            if desc.Width < size.Width as u32 || desc.Height < size.Height as u32 {
                return Err("capture.texture_extent".into());
            }
            if desc.Width as u64 * desc.Height as u64 * 4 > MAX_FRAME_BYTES {
                return Err("capture.texture_budget".into());
            }
            let source_box = if self.signal_only {
                let (x, y) = crate::geometry::signal_offset(
                    &self.target,
                    size.Width as u32,
                    size.Height as u32,
                )?;
                desc.Width = 6;
                desc.Height = 2;
                Some(D3D11_BOX {
                    left: x,
                    top: y,
                    front: 0,
                    right: x + 6,
                    bottom: y + 2,
                    back: 1,
                })
            } else {
                None
            };
            desc.Usage = D3D11_USAGE_STAGING;
            desc.BindFlags = 0;
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            desc.MiscFlags = 0;
            let mut staging = None;
            unsafe {
                self.device
                    .CreateTexture2D(&desc, None, Some(&mut staging))
                    .map_err(|e| e.to_string())?;
            }
            let staging = staging.ok_or("capture.staging")?;
            unsafe {
                if let Some(region) = &source_box {
                    self.context.CopySubresourceRegion(
                        &staging,
                        0,
                        0,
                        0,
                        0,
                        &texture,
                        0,
                        Some(region),
                    );
                } else {
                    self.context.CopyResource(&staging, &texture);
                }
            }
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            unsafe {
                self.context
                    .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                    .map_err(|e| e.to_string())?;
            }
            let mapped_guard = Mapped {
                context: &self.context,
                texture: &staging,
            };
            let row = desc.Width as usize * 4;
            let mut bgra = vec![0; row * desc.Height as usize];
            if mapped.pData.is_null() || (mapped.RowPitch as usize) < row {
                return Err("capture.mapped_pitch".into());
            }
            for y in 0..desc.Height as usize {
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        (mapped.pData as *const u8).add(y * mapped.RowPitch as usize),
                        bgra.as_mut_ptr().add(y * row),
                        row,
                    );
                }
            }
            drop(mapped_guard);
            self.target.verify()?;
            Ok(Frame {
                width: desc.Width,
                height: desc.Height,
                bgra,
                relative_time: frame
                    .SystemRelativeTime()
                    .map_err(|e| e.to_string())?
                    .Duration,
            })
        })();
        let close = frame.Close().map_err(|e| e.to_string());
        result.and_then(|v| close.map(|_| v))
    }
    /// Explicit close reports native cleanup errors; Drop still performs a bounded retry.
    pub fn close(mut self) -> Result<()> {
        let mut issues = Vec::new();
        for result in [
            self.pool.RemoveFrameArrived(self.token),
            self.session.Close(),
            self.pool.Close(),
        ] {
            if let Err(e) = result {
                issues.push(e.to_string());
            }
        }
        self.closed = issues.is_empty();
        if issues.is_empty() {
            Ok(())
        } else {
            Err(format!("capture.cleanup_incomplete:{}", issues.join(";")))
        }
    }
}
fn signal(tx: &SyncSender<()>) {
    let _ = tx.try_send(());
}
struct Mapped<'a> {
    context: &'a ID3D11DeviceContext,
    texture: &'a ID3D11Texture2D,
}
impl Drop for Mapped<'_> {
    fn drop(&mut self) {
        unsafe {
            self.context.Unmap(self.texture, 0);
        }
    }
}
impl Drop for CaptureStream {
    fn drop(&mut self) {
        if self.closed {
            return;
        }
        let _ = self.pool.RemoveFrameArrived(self.token);
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}
fn retain_mta() -> Result<()> {
    static ONCE: std::sync::OnceLock<std::result::Result<(), String>> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| {
        #[link(name = "ole32")]
        unsafe extern "system" {
            fn CoIncrementMTAUsage(cookie: *mut usize) -> i32;
        }
        let mut cookie = 0;
        let hr = unsafe { CoIncrementMTAUsage(&mut cookie) };
        if hr < 0 {
            Err(format!("capture.mta:{hr:x}"))
        } else {
            Ok(())
        }
    })
    .clone()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_frame_allocation_budget() {
        assert!(extent(3840, 2160).is_ok());
        assert!(extent(16384, 16384).is_err());
        assert!(extent(0, 200).is_err());
        assert!(extent(-1, 200).is_err());
    }
}
