//! USBridge: DXGI Desktop Duplication of an existing monitor -- the MttVDD virtual monitor
//! (`pf_vdisplay::mttvdd`) that every USBridge streamer on Windows shares -- instead of the
//! pf-vdisplay driver's IDD push. Frames are BGRA textures on the monitor's own adapter,
//! handed to the host encoders (NVENC / AMF / QSV / MF take `PixelFormat::Bgra`), the way
//! rust-shine's `capture-dxgi` feeds NVENC.
//!
//! Each delivered frame is a slot of a small ring of owned textures, so the encoder can
//! still be reading one while the next is captured. The duplication is reopened on
//! `DXGI_ERROR_ACCESS_LOST` (a mode change, the secure desktop); the ring follows the size.
//!
//! The pointer is not in duplicated frames and the Windows encoders do not read
//! `frame.cursor`, so it is blended in here with `pf_encode_win::convert::CursorBlendPass`
//! (the pass pf-vdisplay's capture model uses): the clean desktop image is kept, every
//! delivered frame is that image plus the pointer, and a pointer-only move on a still
//! desktop delivers a `CursorRegen` frame so the pointer never freezes.

use crate::dxgi::{make_device, D3d11Frame};
use crate::{CapturedFrame, Capturer, FramePayload, PixelFormat};
use anyhow::{anyhow, bail, Context, Result};
use pf_frame::{FrameOrigin, Provenance};
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_BIND_RENDER_TARGET,
    D3D11_BIND_SHADER_RESOURCE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIFactory1, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource, DXGI_ERROR_ACCESS_LOST,
    DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO, DXGI_OUTDUPL_POINTER_SHAPE_INFO,
};

const RING: usize = 4;

/// The pointer as duplication reports it.
#[derive(Default)]
struct Pointer {
    x: i32,
    y: i32,
    visible: bool,
    /// rgba, w, h, hot_x, hot_y
    shape: Option<(Arc<Vec<u8>>, u32, u32, u32, u32)>,
    serial: u64,
}

impl Pointer {
    fn overlay(&self) -> Option<pf_frame::CursorOverlay> {
        let (rgba, w, h, hot_x, hot_y) = self.shape.clone()?;
        self.visible.then_some(pf_frame::CursorOverlay {
            x: self.x,
            y: self.y,
            w,
            h,
            rgba,
            serial: self.serial,
            hot_x,
            hot_y,
            visible: true,
        })
    }
}

pub struct DxgiDupCapturer {
    target: pf_frame::dxgi::WinCaptureTarget,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    dup: IDXGIOutputDuplication,
    ring: Vec<ID3D11Texture2D>,
    /// The newest desktop image without the pointer.
    clean: ID3D11Texture2D,
    blend: Option<pf_encode_win::convert::CursorBlendPass>,
    pointer: Pointer,
    width: u32,
    height: u32,
    format: DXGI_FORMAT,
    slot: usize,
    /// The ring slot of the newest delivered frame, for a repeat.
    last: Option<usize>,
    source_seq: u64,
    start: Instant,
    keepalive: Option<Box<dyn Send>>,
    /// A frame `wait_arrival` acquired, for the next `try_latest` (a duplication cannot peek).
    pending: Option<CapturedFrame>,
}

// SAFETY: `make_device` omits `SINGLETHREADED`; D3D11/DXGI COM refcounts are interlocked.
// The capturer moves onto its owner thread and is `Send`, not `Sync`, so the immediate
// context and the duplication are never used concurrently.
unsafe impl Send for DxgiDupCapturer {}

fn wide_eq(raw: &[u16], name: &str) -> bool {
    let len = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
    String::from_utf16_lossy(&raw[..len]).eq_ignore_ascii_case(name)
}

/// Device on the adapter DXGI lists the output under, plus a duplication of that output.
///
/// Found by GDI name across every adapter, not by `target.adapter_luid`: that is the CCD
/// path's adapter, which for an IddCx monitor is the virtual driver's own adapter, while
/// DXGI lists the output under the GPU that renders it (the RTX 3090 on the test laptop).
fn open_dup(
    target: &pf_frame::dxgi::WinCaptureTarget,
) -> Result<(ID3D11Device, ID3D11DeviceContext, IDXGIOutputDuplication)> {
    // SAFETY: COM calls on owned interfaces; every out value is checked.
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().context("CreateDXGIFactory1")?;
        let mut a = 0;
        while let Ok(adapter) = factory.EnumAdapters1(a) {
            a += 1;
            let mut o = 0;
            while let Ok(output) = adapter.EnumOutputs(o) {
                o += 1;
                if !wide_eq(&output.GetDesc()?.DeviceName, &target.gdi_name) {
                    continue;
                }
                let (device, context) = make_device(&adapter).context("D3D11 device for duplication")?;
                let output1: IDXGIOutput1 = output.cast()?;
                let dup = output1
                    .DuplicateOutput(&device)
                    .with_context(|| format!("DuplicateOutput({})", target.gdi_name))?;
                return Ok((device, context, dup));
            }
        }
    }
    bail!("no DXGI adapter lists {} as an output", target.gdi_name)
}

fn make_texture(device: &ID3D11Device, w: u32, h: u32, format: DXGI_FORMAT) -> Result<ID3D11Texture2D> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: w,
        Height: h,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut tex = None;
    // SAFETY: `desc` is complete; the out-param is checked.
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut tex)) }?;
    tex.ok_or_else(|| anyhow!("CreateTexture2D returned nothing"))
}

/// A DXGI pointer shape as straight-alpha RGBA, cropped to `CURSOR_OVERLAY_MAX`.
fn shape_to_rgba(info: &DXGI_OUTDUPL_POINTER_SHAPE_INFO, buf: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    const MONOCHROME: u32 = 1;
    const COLOR: u32 = 2;
    const MASKED_COLOR: u32 = 4;
    let pitch = info.Pitch as usize;
    let (w, h) = if info.Type == MONOCHROME {
        (info.Width, info.Height / 2)
    } else {
        (info.Width, info.Height)
    };
    if w == 0 || h == 0 {
        return None;
    }
    let mut out = vec![0u8; w as usize * h as usize * 4];
    for y in 0..h as usize {
        for x in 0..w as usize {
            let px: [u8; 4] = match info.Type {
                COLOR | MASKED_COLOR => {
                    let i = y * pitch + x * 4;
                    let (b, g, r, a) = (*buf.get(i)?, *buf.get(i + 1)?, *buf.get(i + 2)?, *buf.get(i + 3)?);
                    if info.Type == COLOR {
                        [r, g, b, a]
                    } else if a == 0 {
                        [r, g, b, 255] // mask clear: the colour replaces the screen
                    } else if (r, g, b) == (0, 0, 0) {
                        [0, 0, 0, 0] // XOR with black changes nothing
                    } else {
                        [r, g, b, 255] // XOR, approximated by the colour itself
                    }
                }
                MONOCHROME => {
                    let bit = 0x80u8 >> (x % 8);
                    let and = buf.get(y * pitch + x / 8)? & bit != 0;
                    let xor = buf.get((y + h as usize) * pitch + x / 8)? & bit != 0;
                    match (and, xor) {
                        (true, false) => [0, 0, 0, 0],
                        (false, false) => [0, 0, 0, 255],
                        (false, true) => [255, 255, 255, 255],
                        (true, true) => [0, 0, 0, 255], // invert: drawn black so it stays visible
                    }
                }
                _ => return None,
            };
            let o = (y * w as usize + x) * 4;
            out[o..o + 4].copy_from_slice(&px);
        }
    }
    Some(pf_frame::crop_cursor_rgba(out, w, h))
}

impl DxgiDupCapturer {
    pub fn open(target: pf_frame::dxgi::WinCaptureTarget, keepalive: Box<dyn Send>) -> Result<Self> {
        let (device, context, dup) = open_dup(&target)?;
        // SAFETY: plain getter on a live duplication.
        let mode = unsafe { dup.GetDesc() }.ModeDesc;
        if mode.Format != DXGI_FORMAT_B8G8R8A8_UNORM {
            // An HDR (FP16) desktop: SDR only for now, like rust-shine's default.
            bail!(
                "{} composes in DXGI format {} -- only 8-bit BGRA is captured",
                target.gdi_name,
                mode.Format.0
            );
        }
        let ring = (0..RING)
            .map(|_| make_texture(&device, mode.Width, mode.Height, mode.Format))
            .collect::<Result<Vec<_>>>()?;
        let clean = make_texture(&device, mode.Width, mode.Height, mode.Format)?;
        let blend = match pf_encode_win::convert::CursorBlendPass::new(&device) {
            Ok(b) => Some(b),
            Err(e) => {
                tracing::warn!(error = %e, "cursor blend pass unavailable -- the stream has no pointer");
                None
            }
        };
        tracing::info!(
            display = %target.gdi_name,
            width = mode.Width,
            height = mode.Height,
            refresh = mode.RefreshRate.Numerator / mode.RefreshRate.Denominator.max(1),
            "DXGI duplication capture opened (MttVDD)"
        );
        Ok(DxgiDupCapturer {
            target,
            device,
            context,
            dup,
            ring,
            clean,
            blend,
            pointer: Pointer::default(),
            width: mode.Width,
            height: mode.Height,
            format: mode.Format,
            slot: 0,
            last: None,
            source_seq: 0,
            start: Instant::now(),
            keepalive: Some(keepalive),
            pending: None,
        })
    }

    pub(crate) fn set_keepalive(&mut self, keepalive: Box<dyn Send>) {
        self.keepalive = Some(keepalive);
    }

    /// Reopen after ACCESS_LOST; the monitor may have changed size or device.
    fn reopen(&mut self) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(3);
        let (device, context, dup) = loop {
            match open_dup(&self.target) {
                Ok(v) => break v,
                Err(e) if Instant::now() < deadline => {
                    tracing::debug!(error = %e, "DXGI duplication reopen retry");
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => return Err(e),
            }
        };
        // SAFETY: plain getter on a live duplication.
        let mode = unsafe { dup.GetDesc() }.ModeDesc;
        self.ring = (0..RING)
            .map(|_| make_texture(&device, mode.Width, mode.Height, mode.Format))
            .collect::<Result<Vec<_>>>()?;
        self.clean = make_texture(&device, mode.Width, mode.Height, mode.Format)?;
        self.blend = pf_encode_win::convert::CursorBlendPass::new(&device).ok();
        self.width = mode.Width;
        self.height = mode.Height;
        self.format = mode.Format;
        self.last = None;
        self.pending = None;
        self.source_seq = 0;
        self.device = device;
        self.context = context;
        self.dup = dup;
        tracing::info!(display = %self.target.gdi_name, width = mode.Width, height = mode.Height, "DXGI duplication reopened");
        Ok(())
    }

    fn frame(&self, slot: usize, origin: FrameOrigin, qpc: u64) -> CapturedFrame {
        CapturedFrame {
            provenance: Provenance {
                origin,
                source_seq: self.source_seq,
                source_qpc: qpc,
            },
            width: self.width,
            height: self.height,
            pts_ns: self.start.elapsed().as_nanos() as u64,
            format: PixelFormat::Bgra,
            payload: FramePayload::D3d11(D3d11Frame {
                texture: self.ring[slot].clone(),
                device: self.device.clone(),
                pyro: None,
            }),
            cursor: None,
        }
    }

    /// Pointer position/visibility and shape from one acquired frame; `true` if it changed.
    fn update_pointer(&mut self, info: &DXGI_OUTDUPL_FRAME_INFO) -> bool {
        let mut changed = false;
        if info.LastMouseUpdateTime != 0 {
            let p = info.PointerPosition;
            let now = (p.Position.x, p.Position.y, p.Visible.as_bool());
            changed |= now != (self.pointer.x, self.pointer.y, self.pointer.visible);
            (self.pointer.x, self.pointer.y, self.pointer.visible) = now;
        }
        if info.PointerShapeBufferSize > 0 {
            let mut buf = vec![0u8; info.PointerShapeBufferSize as usize];
            let mut used = 0u32;
            let mut sinfo = DXGI_OUTDUPL_POINTER_SHAPE_INFO::default();
            // SAFETY: `buf` holds the size the frame info reported; out-params are locals.
            let got = unsafe {
                self.dup.GetFramePointerShape(buf.len() as u32, buf.as_mut_ptr().cast(), &mut used, &mut sinfo)
            };
            if got.is_ok() {
                if let Some((rgba, w, h)) = shape_to_rgba(&sinfo, &buf[..(used as usize).min(buf.len())]) {
                    self.pointer.shape = Some((Arc::new(rgba), w, h, sinfo.HotSpot.x as u32, sinfo.HotSpot.y as u32));
                    self.pointer.serial += 1;
                    changed = true;
                }
            }
        }
        changed
    }

    /// One acquire of up to `timeout_ms`: `Some` for a new desktop image or a pointer move
    /// over the last one, `None` when nothing new arrived.
    fn acquire(&mut self, timeout_ms: u32) -> Result<Option<CapturedFrame>> {
        let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut res: Option<IDXGIResource> = None;
        // SAFETY: out-params are locals; the frame is released below on every path.
        match unsafe { self.dup.AcquireNextFrame(timeout_ms, &mut info, &mut res) } {
            Ok(()) => {}
            Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(None),
            Err(e) if e.code() == DXGI_ERROR_ACCESS_LOST => {
                self.reopen()?;
                return Ok(None);
            }
            Err(e) => return Err(anyhow!("AcquireNextFrame: {e}")),
        }
        let result = (|| -> Result<Option<CapturedFrame>> {
            let pointer_changed = self.update_pointer(&info);
            let new_image = info.LastPresentTime != 0;
            if new_image {
                let tex: ID3D11Texture2D = res
                    .take()
                    .ok_or_else(|| anyhow!("AcquireNextFrame returned no resource"))?
                    .cast()?;
                // SAFETY: same device, size and format (both follow the mode on reopen).
                unsafe { self.context.CopyResource(&self.clean, &tex) };
                self.source_seq += 1;
            }
            if !(new_image || (pointer_changed && self.source_seq > 0)) {
                return Ok(None);
            }
            let slot = self.slot;
            self.slot = (self.slot + 1) % RING;
            // SAFETY: as above.
            unsafe { self.context.CopyResource(&self.ring[slot], &self.clean) };
            if let (Some(ov), Some(blend)) = (self.pointer.overlay(), self.blend.as_mut()) {
                if let Err(e) = blend.blend(&self.device, &self.context, &self.ring[slot], &ov, 0.0) {
                    tracing::debug!(error = %e, "cursor blend failed");
                }
            }
            self.last = Some(slot);
            let origin = if new_image { FrameOrigin::Source } else { FrameOrigin::CursorRegen };
            Ok(Some(self.frame(slot, origin, info.LastPresentTime as u64)))
        })();
        // SAFETY: matches the successful AcquireNextFrame above.
        unsafe {
            let _ = self.dup.ReleaseFrame();
        }
        result
    }
}

impl Capturer for DxgiDupCapturer {
    fn next_frame(&mut self) -> Result<CapturedFrame> {
        self.next_frame_within(Duration::from_secs(2))
    }

    /// The next new frame, waiting up to `budget`; past that a repeat of the newest one
    /// when there is one (a static desktop presents nothing).
    fn next_frame_within(&mut self, budget: Duration) -> Result<CapturedFrame> {
        if let Some(f) = self.pending.take() {
            return Ok(f);
        }
        let deadline = Instant::now() + budget;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if let Some(f) = self.acquire(left.as_millis().clamp(1, 100) as u32)? {
                return Ok(f);
            }
            if Instant::now() >= deadline {
                return match self.last {
                    Some(slot) => Ok(self.frame(slot, FrameOrigin::Hold, 0)),
                    None => Err(anyhow!("no frame from {} within {budget:?}", self.target.gdi_name)),
                };
            }
        }
    }

    /// The encode loop then follows the desktop's presents instead of its own tick, whose
    /// phase drifts against vsync: on a fixed 120 Hz tick about a sixth of the 120 new images
    /// a second arrived two per tick and only the newest was sent (measured: 99 unique/s).
    fn supports_arrival_wait(&self) -> bool {
        true
    }

    fn wait_arrival(&mut self, deadline: Instant) {
        while self.pending.is_none() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return;
            }
            match self.acquire(left.as_millis().clamp(1, 100) as u32) {
                Ok(Some(f)) => self.pending = Some(f),
                Ok(None) => {}
                Err(e) => {
                    tracing::debug!(error = %e, "DXGI duplication wait failed");
                    return;
                }
            }
        }
    }

    fn try_latest(&mut self) -> Result<Option<CapturedFrame>> {
        // The parked frame, or anything newer, without waiting.
        let mut newest = self.pending.take();
        while let Some(f) = self.acquire(0)? {
            newest = Some(f);
        }
        Ok(newest)
    }

    fn take_keepalive(&mut self) -> Option<Box<dyn Send>> {
        self.keepalive.take()
    }
}
