//! USBridge: the virtual monitor from VirtualDrivers' "Virtual Display Driver" (MttVDD,
//! <https://github.com/VirtualDrivers/Virtual-Display-Driver>, MIT), the one driver every
//! USBridge streamer on Windows shares, instead of the pf-vdisplay IddCx driver.
//!
//! Ported from rust-shine's `virtual-display` crate (`windows/mttvdd.rs`,
//! `display_config.rs`, `primary.rs`), where it runs the RustShine streamer. MttVDD has no
//! "create a monitor" call: it plugs in one monitor with a fixed mode list, read when the
//! device starts. "Create" attaches that monitor to the desktop in the mode closest to the
//! client's and makes it the primary display; dropping the lease puts the desktop back and
//! detaches it. None of it needs elevation.
//!
//! Primary matters for the frame rate, not only for where windows open: on a hybrid laptop
//! DWM composes at the primary display's refresh, so a 120 Hz virtual monitor next to a
//! 60 Hz primary panel gets 60 frames a second (measured: 60.3/s, and 120.3/s once it is
//! primary). Capture is DXGI Desktop Duplication on the host side
//! (`pf_capture::open_dxgi_dup`); a target from here carries `wudf_pid == 0`.
//!
//! Verified against MttVDD release 25.7.23 (in rust-shine):
//! - The legacy `ChangeDisplaySettingsExW` attach and move fail (`DISP_CHANGE_FAILED`) on a
//!   hybrid-GPU laptop, where Windows also puts a newly seen virtual monitor into
//!   "duplicate" with the panel. Attach and moves go through `SetDisplayConfig`; only the
//!   mode is set with the legacy call, which works.
//! - The GDI source name changes every time the device restarts, so it is always looked up
//!   by the adapter's hardware id.

use std::mem::size_of;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use windows::core::PCWSTR;
use windows::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig,
    SetDisplayConfig, DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
    DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME, DISPLAYCONFIG_DEVICE_INFO_HEADER,
    DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE, DISPLAYCONFIG_PATH_INFO,
    DISPLAYCONFIG_SOURCE_DEVICE_NAME, DISPLAYCONFIG_TARGET_DEVICE_NAME, QDC_ALL_PATHS,
    QDC_ONLY_ACTIVE_PATHS, QUERY_DISPLAY_CONFIG_FLAGS, SDC_ALLOW_CHANGES,
    SDC_ALLOW_PATH_ORDER_CHANGES, SDC_APPLY, SDC_SAVE_TO_DATABASE, SDC_TOPOLOGY_SUPPLIED,
    SDC_USE_SUPPLIED_DISPLAY_CONFIG, SET_DISPLAY_CONFIG_FLAGS,
};
use windows::Win32::Foundation::{ERROR_SUCCESS, POINTL};
use windows::Win32::Graphics::Gdi::{
    ChangeDisplaySettingsExW, EnumDisplayDevicesW, EnumDisplaySettingsW, CDS_UPDATEREGISTRY,
    DEVMODEW, DISPLAYCONFIG_PATH_ACTIVE, DISPLAYCONFIG_PATH_MODE_IDX_INVALID, DISPLAY_DEVICEW,
    DISPLAY_DEVICE_ATTACHED_TO_DESKTOP, DISP_CHANGE_SUCCESSFUL, DM_DISPLAYFREQUENCY,
    DM_PELSHEIGHT, DM_PELSWIDTH, ENUM_DISPLAY_SETTINGS_MODE,
};

use crate::backend::{DisplayOwnership, VirtualDisplay, VirtualOutput};
use crate::Mode;

/// The device node's hardware id (`install-mttvdd.ps1`), which `EnumDisplayDevicesW` also
/// reports as each of its sources' `DeviceID`.
const HARDWARE_ID: &str = "Root\\MttVDD";
/// MttVDD's monitor in its target device path (`DISPLAY#MTT1337#...`).
const TARGET_ID: &str = "MTT1337";

/// Whether the MttVDD adapter is installed (has a GDI source).
pub fn is_installed() -> bool {
    find_source().is_some()
}

/// `USBRIDGE_VDD_PRIMARY=0` keeps the physical primary (and 60 Hz on a hybrid laptop).
fn want_primary() -> bool {
    !matches!(
        std::env::var("USBRIDGE_VDD_PRIMARY").as_deref(),
        Ok("0") | Ok("false") | Ok("off")
    )
}

/// One MttVDD monitor at a time: the driver plugs in exactly one.
static IN_USE: Mutex<bool> = Mutex::new(false);

pub struct MttvddDisplay;

impl MttvddDisplay {
    pub fn new() -> Self {
        MttvddDisplay
    }
}

impl Default for MttvddDisplay {
    fn default() -> Self {
        Self::new()
    }
}

impl VirtualDisplay for MttvddDisplay {
    fn name(&self) -> &'static str {
        "mttvdd"
    }

    fn create(&mut self, mode: Mode) -> Result<VirtualOutput> {
        {
            let mut busy = IN_USE.lock().unwrap();
            if *busy {
                bail!("the MttVDD virtual monitor is already streamed by another session");
            }
            *busy = true;
        }
        match attach(mode) {
            Ok((got, target)) => Ok(VirtualOutput {
                node_id: 0,
                preferred_mode: Some((got.width, got.height, got.refresh_hz)),
                win_capture: Some(target),
                keepalive: Box::new(Lease),
                ownership: DisplayOwnership::Owned,
            }),
            Err(e) => {
                *IN_USE.lock().unwrap() = false;
                Err(e)
            }
        }
    }
}

/// Dropping it puts the desktop back and detaches the monitor.
struct Lease;

impl Drop for Lease {
    fn drop(&mut self) {
        if let Err(e) = restore_primary() {
            tracing::warn!(error = %e, "MttVDD: putting the previous primary display back failed");
        }
        if let Err(e) = detach_target(TARGET_ID) {
            tracing::warn!(error = %e, "MttVDD: detaching the virtual monitor failed");
        } else {
            tracing::info!("MttVDD: virtual monitor detached");
        }
        *IN_USE.lock().unwrap() = false;
    }
}

/// Attach in the mode closest to `want`, make it primary, and describe it for capture.
fn attach(want: Mode) -> Result<(Mode, pf_frame::dxgi::WinCaptureTarget)> {
    let source = find_source().ok_or_else(|| {
        anyhow!("MttVDD (Virtual Display Driver) is not installed -- the USBridge agent installs it")
    })?;
    let mode = pick_mode(&modes(&source.name), want)
        .ok_or_else(|| anyhow!("MttVDD offers no display modes (its monitor never arrived)"))?;
    if (mode.width, mode.height, mode.refresh_hz) != (want.width, want.height, want.refresh_hz) {
        tracing::warn!(?want, got = ?mode, "MttVDD has no exact mode for the client -- using the closest");
    }
    if !source.attached {
        let primary_before = current_primary();
        attach_target(TARGET_ID).context("MttVDD attach")?;
        if let Some(p) = primary_before {
            if let Err(e) = move_origin_to(&p) {
                tracing::warn!(error = %e, primary = %p, "MttVDD: couldn't keep the primary in place after attach");
            }
        }
    }
    set_mode(&source.name, mode)?;

    // ChangeDisplaySettingsExW returns before GDI and CCD catch up.
    let deadline = Instant::now() + Duration::from_secs(3);
    let name = loop {
        if let Some(s) = find_source().filter(|s| s.attached) {
            break s.name;
        }
        if Instant::now() >= deadline {
            bail!("MttVDD's monitor didn't attach to the desktop within 3 s");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    if want_primary() {
        make_primary(&name).context("make the MttVDD monitor primary")?;
    }
    let path = active_paths()?
        .into_iter()
        .find(|p| source_gdi_name(p).is_some_and(|n| n.eq_ignore_ascii_case(&name)))
        .ok_or_else(|| anyhow!("{name} is not an active display path"))?;
    let target = pf_frame::dxgi::WinCaptureTarget {
        adapter_luid: pf_frame::dxgi::pack_luid(path.sourceInfo.adapterId),
        gdi_name: name.clone(),
        target_id: path.targetInfo.id,
        // No WUDFHost to share a ring with: the host duplicates the output with DXGI.
        wudf_pid: 0,
        cursor_excluded: false,
    };
    tracing::info!(display = %name, ?mode, primary = want_primary(), "MttVDD virtual monitor attached");
    Ok((mode, target))
}

// ---- GDI ------------------------------------------------------------------------------

struct Source {
    name: String,
    attached: bool,
}

fn from_wide(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn display_devices() -> impl Iterator<Item = DISPLAY_DEVICEW> {
    (0u32..).map_while(|i| {
        let mut dd = DISPLAY_DEVICEW {
            cb: size_of::<DISPLAY_DEVICEW>() as u32,
            ..Default::default()
        };
        // SAFETY: `dd` is a properly sized out-param; a null device enumerates adapters.
        unsafe { EnumDisplayDevicesW(PCWSTR::null(), i, &mut dd, 0) }
            .as_bool()
            .then_some(dd)
    })
}

/// The MttVDD adapter's GDI source; an attached one wins if a config has several.
fn find_source() -> Option<Source> {
    display_devices()
        .filter(|dd| from_wide(&dd.DeviceID).eq_ignore_ascii_case(HARDWARE_ID))
        .map(|dd| Source {
            name: from_wide(&dd.DeviceName),
            attached: dd.StateFlags.0 & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP.0 != 0,
        })
        .max_by_key(|s| s.attached)
}

fn devmode() -> DEVMODEW {
    DEVMODEW {
        dmSize: size_of::<DEVMODEW>() as u16,
        ..Default::default()
    }
}

fn modes(name: &str) -> Vec<Mode> {
    let name = wide(name);
    let mut out: Vec<Mode> = Vec::new();
    for i in 0.. {
        let mut dm = devmode();
        // SAFETY: `name` is NUL-terminated and outlives the call; `dm` is sized.
        if !unsafe { EnumDisplaySettingsW(PCWSTR(name.as_ptr()), ENUM_DISPLAY_SETTINGS_MODE(i), &mut dm) }
            .as_bool()
        {
            break;
        }
        let m = Mode {
            width: dm.dmPelsWidth,
            height: dm.dmPelsHeight,
            refresh_hz: dm.dmDisplayFrequency,
        };
        if !out.iter().any(|o| (o.width, o.height, o.refresh_hz) == (m.width, m.height, m.refresh_hz)) {
            out.push(m);
        }
    }
    out
}

/// Exact match if offered, else the closest size, then refresh rate.
fn pick_mode(available: &[Mode], want: Mode) -> Option<Mode> {
    available.iter().copied().min_by_key(|m| {
        (
            m.width.abs_diff(want.width) + m.height.abs_diff(want.height),
            m.refresh_hz.abs_diff(want.refresh_hz),
        )
    })
}

/// Size and refresh rate of an attached source, position untouched.
fn set_mode(name: &str, mode: Mode) -> Result<()> {
    let mut dm = devmode();
    dm.dmFields = DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY;
    dm.dmPelsWidth = mode.width;
    dm.dmPelsHeight = mode.height;
    dm.dmDisplayFrequency = mode.refresh_hz;
    let w = wide(name);
    // SAFETY: `w` is NUL-terminated; `dm` lives across the call.
    let r = unsafe {
        ChangeDisplaySettingsExW(PCWSTR(w.as_ptr()), Some(&dm), None, CDS_UPDATEREGISTRY, None)
    };
    if r != DISP_CHANGE_SUCCESSFUL {
        bail!("ChangeDisplaySettingsExW({name}) failed: DISP_CHANGE {}", r.0);
    }
    Ok(())
}

// ---- CCD (SetDisplayConfig) -------------------------------------------------------------

fn query(
    flags: QUERY_DISPLAY_CONFIG_FLAGS,
) -> Result<(Vec<DISPLAYCONFIG_PATH_INFO>, Vec<DISPLAYCONFIG_MODE_INFO>)> {
    // The path count can grow between the two calls (ERROR_INSUFFICIENT_BUFFER): size again.
    for _ in 0..3 {
        let (mut pc, mut mc) = (0u32, 0u32);
        // SAFETY: plain out-params.
        let err = unsafe { GetDisplayConfigBufferSizes(flags, &mut pc, &mut mc) };
        if err != ERROR_SUCCESS {
            bail!("GetDisplayConfigBufferSizes failed: {}", err.0);
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); pc as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mc as usize];
        // SAFETY: the buffers hold `pc`/`mc` elements, as the call is told.
        let err = unsafe {
            QueryDisplayConfig(flags, &mut pc, paths.as_mut_ptr(), &mut mc, modes.as_mut_ptr(), None)
        };
        if err == ERROR_SUCCESS {
            paths.truncate(pc as usize);
            modes.truncate(mc as usize);
            return Ok((paths, modes));
        }
        if err.0 != 122 {
            bail!("QueryDisplayConfig failed: {}", err.0);
        }
    }
    bail!("QueryDisplayConfig kept growing")
}

fn active_paths() -> Result<Vec<DISPLAYCONFIG_PATH_INFO>> {
    Ok(query(QDC_ONLY_ACTIVE_PATHS)?.0)
}

fn set(
    what: &str,
    paths: &[DISPLAYCONFIG_PATH_INFO],
    modes: Option<&[DISPLAYCONFIG_MODE_INFO]>,
    flags: SET_DISPLAY_CONFIG_FLAGS,
) -> Result<()> {
    // SAFETY: slices outlive the call.
    let r = unsafe { SetDisplayConfig(Some(paths), modes, flags) };
    if r != 0 {
        bail!("SetDisplayConfig({what}) failed: {r}");
    }
    Ok(())
}

fn target_device_path(path: &DISPLAYCONFIG_PATH_INFO) -> Option<String> {
    let mut name = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
    name.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
    name.header.size = size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
    name.header.adapterId = path.targetInfo.adapterId;
    name.header.id = path.targetInfo.id;
    // SAFETY: the header heads a correctly sized, typed request.
    if unsafe { DisplayConfigGetDeviceInfo(&mut name.header as *mut DISPLAYCONFIG_DEVICE_INFO_HEADER) } != 0 {
        return None;
    }
    Some(from_wide(&name.monitorDevicePath))
}

fn source_gdi_name(path: &DISPLAYCONFIG_PATH_INFO) -> Option<String> {
    let mut sn = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
    sn.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
    sn.header.size = size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
    sn.header.adapterId = path.sourceInfo.adapterId;
    sn.header.id = path.sourceInfo.id;
    // SAFETY: the header heads a correctly sized, typed request.
    if unsafe { DisplayConfigGetDeviceInfo(&mut sn.header as *mut DISPLAYCONFIG_DEVICE_INFO_HEADER) } != 0 {
        return None;
    }
    Some(from_wide(&sn.viewGdiDeviceName))
}

fn is_active(p: &DISPLAYCONFIG_PATH_INFO) -> bool {
    p.flags & DISPLAYCONFIG_PATH_ACTIVE != 0
}

fn same_source(a: &DISPLAYCONFIG_PATH_INFO, b: &DISPLAYCONFIG_PATH_INFO) -> bool {
    a.sourceInfo.id == b.sourceInfo.id
        && a.sourceInfo.adapterId.LowPart == b.sourceInfo.adapterId.LowPart
        && a.sourceInfo.adapterId.HighPart == b.sourceInfo.adapterId.HighPart
}

/// Attach the monitor whose target path contains `target_id` as a display of its own
/// (dropping a "duplicate" of another screen). Windows picks mode and position.
fn attach_target(target_id: &str) -> Result<()> {
    let (all, _) = query(QDC_ALL_PATHS)?;
    let is_target = |p: &DISPLAYCONFIG_PATH_INFO| target_device_path(p).is_some_and(|d| d.contains(target_id));
    let mut sel: Vec<DISPLAYCONFIG_PATH_INFO> =
        all.iter().filter(|p| is_active(p) && !is_target(p)).copied().collect();
    let free = all
        .iter()
        .find(|p| is_target(p) && p.targetInfo.targetAvailable.as_bool() && !sel.iter().any(|s| same_source(s, p)))
        .copied()
        .ok_or_else(|| anyhow!("no free display path for the MttVDD monitor"))?;
    let mut vpath = free;
    vpath.flags |= DISPLAYCONFIG_PATH_ACTIVE;
    sel.push(vpath);
    for p in &mut sel {
        p.sourceInfo.Anonymous.modeInfoIdx = DISPLAYCONFIG_PATH_MODE_IDX_INVALID;
        p.targetInfo.Anonymous.modeInfoIdx = DISPLAYCONFIG_PATH_MODE_IDX_INVALID;
    }
    set("attach", &sel, None, SDC_APPLY | SDC_TOPOLOGY_SUPPLIED | SDC_ALLOW_PATH_ORDER_CHANGES)
}

/// Detach every active path to `target_id`, keeping the rest of the desktop as is.
fn detach_target(target_id: &str) -> Result<()> {
    let (paths, modes) = query(QDC_ONLY_ACTIVE_PATHS)?;
    let keep: Vec<DISPLAYCONFIG_PATH_INFO> = paths
        .iter()
        .filter(|p| !target_device_path(p).is_some_and(|d| d.contains(target_id)))
        .copied()
        .collect();
    if keep.len() == paths.len() {
        return Ok(());
    }
    set(
        "detach",
        &keep,
        Some(&modes),
        SDC_APPLY | SDC_USE_SUPPLIED_DISPLAY_CONFIG | SDC_ALLOW_CHANGES | SDC_SAVE_TO_DATABASE,
    )
}

// ---- primary display --------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
struct Placed {
    name: String,
    x: i32,
    y: i32,
}

/// The arrangement from before `make_primary` first moved things.
static SAVED: Mutex<Option<Vec<Placed>>> = Mutex::new(None);

fn layout() -> Result<Vec<Placed>> {
    let (paths, modes) = query(QDC_ONLY_ACTIVE_PATHS)?;
    let mut out: Vec<Placed> = Vec::new();
    for p in &paths {
        // SAFETY: an active path's source carries a mode index (union read of a u32).
        let idx = unsafe { p.sourceInfo.Anonymous.modeInfoIdx } as usize;
        let Some(m) = modes.get(idx).filter(|m| m.infoType == DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE) else {
            continue;
        };
        let Some(name) = source_gdi_name(p) else { continue };
        if !out.iter().any(|o| o.name.eq_ignore_ascii_case(&name)) {
            // SAFETY: `infoType` says this is the source-mode arm.
            let pos = unsafe { m.Anonymous.sourceMode.position };
            out.push(Placed { name, x: pos.x, y: pos.y });
        }
    }
    Ok(out)
}

fn apply(layout: &[Placed]) -> Result<()> {
    let (paths, mut modes) = query(QDC_ONLY_ACTIVE_PATHS)?;
    for p in &paths {
        // SAFETY: as in `layout`.
        let idx = unsafe { p.sourceInfo.Anonymous.modeInfoIdx } as usize;
        let Some(name) = source_gdi_name(p) else { continue };
        let Some(pl) = layout.iter().find(|l| l.name.eq_ignore_ascii_case(&name)) else { continue };
        if let Some(m) = modes.get_mut(idx).filter(|m| m.infoType == DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE) {
            m.Anonymous.sourceMode.position = POINTL { x: pl.x, y: pl.y };
        }
    }
    set(
        "positions",
        &paths,
        Some(&modes),
        SDC_APPLY | SDC_USE_SUPPLIED_DISPLAY_CONFIG | SDC_ALLOW_CHANGES | SDC_SAVE_TO_DATABASE,
    )
}

/// `layout` with `target` at the origin: everything shifted by its position.
fn shifted_to(layout: &[Placed], target: &str) -> Option<Vec<Placed>> {
    let t = layout.iter().find(|p| p.name.eq_ignore_ascii_case(target))?;
    let (dx, dy) = (t.x, t.y);
    Some(layout.iter().map(|p| Placed { name: p.name.clone(), x: p.x - dx, y: p.y - dy }).collect())
}

fn current_primary() -> Option<String> {
    layout().ok()?.into_iter().find(|p| p.x == 0 && p.y == 0).map(|p| p.name)
}

fn move_origin_to(name: &str) -> Result<()> {
    let l = layout()?;
    match shifted_to(&l, name) {
        Some(next) if next != l => apply(&next),
        Some(_) => Ok(()),
        None => bail!("{name} isn't an active display"),
    }
}

fn make_primary(name: &str) -> Result<()> {
    let l = layout()?;
    let next = shifted_to(&l, name).ok_or_else(|| anyhow!("{name} isn't an active display"))?;
    if next == l {
        return Ok(());
    }
    SAVED.lock().unwrap().get_or_insert(l);
    apply(&next)?;
    tracing::info!(display = name, "MttVDD monitor is now the primary display");
    Ok(())
}

fn restore_primary() -> Result<()> {
    let Some(saved) = SAVED.lock().unwrap().take() else { return Ok(()) };
    let now = layout()?;
    let Some(old) = saved.iter().find(|p| p.x == 0 && p.y == 0) else { return Ok(()) };
    if !now.iter().any(|p| p.name.eq_ignore_ascii_case(&old.name)) {
        return Ok(());
    }
    let back: Vec<Placed> = saved
        .iter()
        .filter(|p| now.iter().any(|n| n.name.eq_ignore_ascii_case(&p.name)))
        .cloned()
        .collect();
    apply(&back)?;
    tracing::info!(display = %old.name, "primary display restored");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(width: u32, height: u32, refresh_hz: u32) -> Mode {
        Mode { width, height, refresh_hz }
    }

    #[test]
    fn pick_mode_prefers_exact_then_nearest_size_then_rate() {
        let modes = [m(1920, 1080, 60), m(1920, 1080, 120), m(2560, 1440, 60)];
        let k = |o: Option<Mode>| o.map(|x| (x.width, x.height, x.refresh_hz));
        assert_eq!(k(pick_mode(&modes, m(1920, 1080, 120))), Some((1920, 1080, 120)));
        assert_eq!(k(pick_mode(&modes, m(1920, 1080, 100))), Some((1920, 1080, 120)));
        assert_eq!(k(pick_mode(&modes, m(2556, 1179, 60))), Some((2560, 1440, 60)));
        assert!(pick_mode(&[], m(1920, 1080, 60)).is_none());
    }

    #[test]
    fn shift_moves_target_to_origin_and_back() {
        let p = |n: &str, x, y| Placed { name: n.into(), x, y };
        let l = vec![p("A", 0, 0), p("B", 0, 1600), p("V", 2560, 1600)];
        let moved = shifted_to(&l, "v").unwrap();
        assert_eq!(moved[2], p("V", 0, 0));
        assert_eq!(shifted_to(&moved, "A").unwrap(), l);
    }
}
