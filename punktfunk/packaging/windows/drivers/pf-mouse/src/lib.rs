// punktfunk virtual HID mouse — UMDF2 HID minidriver (absolute pointer).
//
// Why it exists: with NO pointing device present (a headless streaming host — no dongle), win32k
// reports the cursor as absent (`SM_MOUSEPRESENT` = 0) and DWM never composites a cursor into the
// pf-vdisplay frame, so the streamed desktop has an invisible pointer even though `SendInput`
// moves it. This driver keeps a resident HID mouse devnode alive for the host service's lifetime,
// which makes Windows always consider a pointer present and draw the cursor — the industry-standard
// fix (what Sunshine/Parsec-class virtual-input drivers achieve). Injection stays `SendInput`;
// the report path below is exercised by `punktfunk-host vmouse-spike` (validation) and is the
// future higher-fidelity injection route.
//
// Structure is pf-gamepad minus the identity zoo: one fixed HID identity (PF:MO, an obviously
// virtual VID/PID no software matches on), one 8-byte input report (5 buttons + absolute 15-bit
// X/Y + wheel + AC-pan), no feature/output reports. The host channel is the **sealed pad channel**
// (design/gamepad-channel-sealing.md) verbatim — mailbox `Global\pfmouse-boot-<i>`, unnamed
// `pf_driver_proto::mouse::MouseShm` DATA section — so the whole handshake + shared-memory surface
// lives in `pf_umdf_util` (the audited unsafe layer) and this crate's logic is 100% SAFE Rust; the
// only `unsafe` here is the unavoidable WDF setup FFI, each with a `// SAFETY:` proof.
//
// Report delivery is EVENT-DRIVEN like a real mouse: the timer completes a pended READ_REPORT only
// when the host bumped `in_seq` — an idle section generates no HID traffic (a constant report
// stream would read as user activity to the OS: idle timers, display sleep).

#![allow(non_snake_case, non_upper_case_globals, clippy::missing_safety_doc)]
// Every remaining `unsafe {}` (all WDF setup FFI) must carry a `// SAFETY:` proof.

use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

use pf_driver_proto::gamepad::ChannelProof;
use pf_driver_proto::mouse::{
    MOUSE_PID, MOUSE_REPORT_ID, MOUSE_REPORT_LEN, MOUSE_VER, MOUSE_VID, MouseShm,
};
use pf_umdf_util::channel::{ChannelClient, ChannelConfig};
use pf_umdf_util::hid::{
    IOCTL_HID_GET_DEVICE_ATTRIBUTES, IOCTL_HID_GET_DEVICE_DESCRIPTOR,
    IOCTL_HID_GET_REPORT_DESCRIPTOR, IOCTL_HID_GET_STRING, IOCTL_HID_READ_REPORT,
    IOCTL_HID_WRITE_REPORT, IOCTL_UMDF_HID_GET_INPUT_REPORT, IOCTL_UMDF_HID_SET_OUTPUT_REPORT,
    declared_len, string_request_id,
};
use pf_umdf_util::skeleton::{self, STATUS_NOT_IMPLEMENTED, STATUS_SUCCESS};
use pf_umdf_util::wdf::{self, Request};
use pf_umdf_util::{dbglog, nt_success};
use wdk_sys::{
    NTSTATUS, PCUNICODE_STRING, PDRIVER_OBJECT, PWDFDEVICE_INIT, ULONG, WDF_NO_OBJECT_ATTRIBUTES,
    WDFDEVICE, WDFDRIVER, WDFQUEUE, WDFQUEUE__, WDFREQUEST, WDFTIMER,
    call_unsafe_wdf_function_binding,
};

// HID report descriptor (80 bytes): one application collection (Generic Desktop / Mouse), report
// id 0x01 — 5 buttons, ABSOLUTE 15-bit X/Y (logical 0..=32767), relative wheel + AC-pan. Absolute
// axes so a future report-driven injection maps 1:1 onto the desktop, and so the OS treats the
// device as a pointer that never "drifts"; presence (not fidelity) is this driver's job today.
#[rustfmt::skip]
static MOUSE_RDESC: [u8; 80] = [
    0x05, 0x01,        // Usage Page (Generic Desktop)
    0x09, 0x02,        // Usage (Mouse)
    0xA1, 0x01,        // Collection (Application)
    0x85, 0x01,        //   Report ID (1)
    0x09, 0x01,        //   Usage (Pointer)
    0xA1, 0x00,        //   Collection (Physical)
    0x05, 0x09,        //     Usage Page (Button)
    0x19, 0x01,        //     Usage Minimum (1)
    0x29, 0x05,        //     Usage Maximum (5)
    0x15, 0x00,        //     Logical Minimum (0)
    0x25, 0x01,        //     Logical Maximum (1)
    0x75, 0x01,        //     Report Size (1)
    0x95, 0x05,        //     Report Count (5)
    0x81, 0x02,        //     Input (Data,Var,Abs) — buttons 1..5
    0x75, 0x03,        //     Report Size (3)
    0x95, 0x01,        //     Report Count (1)
    0x81, 0x03,        //     Input (Const) — pad
    0x05, 0x01,        //     Usage Page (Generic Desktop)
    0x09, 0x30,        //     Usage (X)
    0x09, 0x31,        //     Usage (Y)
    0x15, 0x00,        //     Logical Minimum (0)
    0x26, 0xFF, 0x7F,  //     Logical Maximum (32767)
    0x75, 0x10,        //     Report Size (16)
    0x95, 0x02,        //     Report Count (2)
    0x81, 0x02,        //     Input (Data,Var,Abs) — absolute X/Y
    0x09, 0x38,        //     Usage (Wheel)
    0x15, 0x81,        //     Logical Minimum (-127)
    0x25, 0x7F,        //     Logical Maximum (127)
    0x75, 0x08,        //     Report Size (8)
    0x95, 0x01,        //     Report Count (1)
    0x81, 0x06,        //     Input (Data,Var,Rel) — wheel
    0x05, 0x0C,        //     Usage Page (Consumer)
    0x0A, 0x38, 0x02,  //     Usage (AC Pan)
    0x15, 0x81,        //     Logical Minimum (-127)
    0x25, 0x7F,        //     Logical Maximum (127)
    0x75, 0x08,        //     Report Size (8)
    0x95, 0x01,        //     Report Count (1)
    0x81, 0x06,        //     Input (Data,Var,Rel) — horizontal wheel
    0xC0,              //   End Collection
    0xC0,              // End Collection
];

// HID descriptor (9 bytes, packed): len, type=0x21, bcdHID=0x0100, country=0, numDesc=1, then
// {reportType=0x22, wReportLength = 80 (0x0050)}.
static HID_DESC: [u8; 9] = [0x09, 0x21, 0x00, 0x01, 0x00, 0x01, 0x22, 0x50, 0x00];

// `wReportLength` above and `MOUSE_RDESC`'s array size are the same number written twice, in
// different places. Out of step they fail quietly: hidclass asks for `wReportLength` bytes and
// parses whatever it gets, so the mouse enumerates truncated or not at all with nothing naming
// the cause. Same compile-time pairing pf-gamepad uses.
const _: () = assert!(declared_len(&HID_DESC) == MOUSE_RDESC.len());

// HID_DEVICE_ATTRIBUTES (32 bytes): Size(u32)=32, VendorID, ProductID, VersionNumber, Reserved[11].
fn hid_attrs() -> [u8; 32] {
    let mut a = [0u8; 32];
    a[0..4].copy_from_slice(&32u32.to_le_bytes());
    a[4..6].copy_from_slice(&MOUSE_VID.to_le_bytes());
    a[6..8].copy_from_slice(&MOUSE_PID.to_le_bytes());
    a[8..10].copy_from_slice(&MOUSE_VER.to_le_bytes());
    a
}

/// A report that answers a client's GET_INPUT_REPORT query before the host published anything:
/// id + all-zero state. Never fed into the input stream (READ_REPORT completes only on a fresh
/// host publish), so it cannot warp the cursor to (0,0).
const NEUTRAL_REPORT: [u8; MOUSE_REPORT_LEN] = {
    let mut r = [0u8; MOUSE_REPORT_LEN];
    r[0] = MOUSE_REPORT_ID;
    r
};

static MANUAL_QUEUE: AtomicPtr<WDFQUEUE__> = AtomicPtr::new(core::ptr::null_mut());
/// The latest host-published report (kept for GET_INPUT_REPORT queries).
static INPUT_REPORT: std::sync::Mutex<[u8; MOUSE_REPORT_LEN]> =
    std::sync::Mutex::new(NEUTRAL_REPORT);
/// The last `in_seq` a READ_REPORT was completed for — the event-driven gate. NOT advanced when no
/// read is pended (the next tick retries), so a publish is never dropped while a reader exists.
static DELIVERED_SEQ: AtomicU32 = AtomicU32::new(0);

// ---- the sealed host channel: layouts + offsets from pf_driver_proto (drift = compile error) ----
const SHM_MAGIC: u32 = pf_driver_proto::mouse::MOUSE_MAGIC; // "PFMO"
const SHM_SIZE: usize = core::mem::size_of::<MouseShm>();
const GAMEPAD_PROTO_VERSION: u32 = pf_driver_proto::gamepad::GAMEPAD_PROTO_VERSION;

// MouseShm field offsets (the driver reads report + in_seq, writes the health marks).
const OFF_IN_SEQ: usize = core::mem::offset_of!(MouseShm, in_seq);
const OFF_REPORT: usize = core::mem::offset_of!(MouseShm, report);
const OFF_DRIVER_PROTO: usize = core::mem::offset_of!(MouseShm, driver_proto);
const OFF_DRIVER_HEARTBEAT: usize = core::mem::offset_of!(MouseShm, driver_heartbeat);
const OFF_PAD_INDEX: usize = core::mem::offset_of!(MouseShm, pad_index);

/// The sealed-channel client (`ProcessSharingDisabled` gives the mouse its own WUDFHost, so this
/// static is per-device). The handshake/adoption/validation state machine lives in `pf_umdf_util`.
static CHANNEL: ChannelClient = ChannelClient::new();

/// This device's channel config (magic/size/index offset + our logger).
fn channel_cfg() -> ChannelConfig {
    ChannelConfig {
        tag: "pf-mouse",
        boot_name_prefix: "Global\\pfmouse-boot-",
        data_magic: SHM_MAGIC,
        data_size: SHM_SIZE,
        min_data_size: SHM_SIZE, // layout never grew — no fallback size
        pad_index_off: OFF_PAD_INDEX,
        log,
    }
}

// The bring-up file log. OPT-IN — debug builds, or the `PFMOUSE_DEBUG_LOG` env var — so a RELEASE
// driver never writes the file and never traps into the debugger. Path, sink and the gate live
// in `pf_umdf_util::log`, one copy for all four drivers.
pf_umdf_util::file_log!("pfmouse-driver.log", "PFMOUSE_DEBUG_LOG");

#[unsafe(export_name = "DriverEntry")]
pub unsafe extern "system" fn driver_entry(
    driver: PDRIVER_OBJECT,
    registry_path: PCUNICODE_STRING,
) -> NTSTATUS {
    log("[pf-mouse] DriverEntry");
    // SAFETY: `driver`/`registry_path` are the loader's DriverEntry arguments.
    unsafe { skeleton::driver_create(driver, registry_path, Some(evt_device_add)) }
}

extern "C" fn evt_device_add(_driver: WDFDRIVER, mut device_init: PWDFDEVICE_INIT) -> NTSTATUS {
    log("[pf-mouse] EvtDeviceAdd");

    // Mark as a filter (HID minidriver sits below mshidumdf.sys).
    // SAFETY: device_init is provided by the framework and non-null.
    unsafe { call_unsafe_wdf_function_binding!(WdfFdoInitSetFilter, device_init) };

    let mut device: WDFDEVICE = core::ptr::null_mut();
    // SAFETY: device_init valid; attributes allowed null; device receives the handle.
    let st = unsafe {
        call_unsafe_wdf_function_binding!(
            WdfDeviceCreate,
            &mut device_init,
            WDF_NO_OBJECT_ATTRIBUTES,
            &mut device
        )
    };
    if !nt_success(st) {
        dbglog!("[pf-mouse] WdfDeviceCreate failed 0x{:08x}", st as u32);
        return st;
    }

    // SAFETY: `device` is the live device just created — the exact contract this fn requires.
    let shm_idx = unsafe { wdf::query_location_index(device) };
    CHANNEL.set_index(shm_idx);
    dbglog!("[pf-mouse] shm index = {shm_idx}");

    // Default parallel queue handling all IOCTLs.
    // SAFETY: `device` is the live device just created.
    if let Err(st) = unsafe { skeleton::create_default_queue(device, Some(evt_io_device_control)) }
    {
        dbglog!(
            "[pf-mouse] default WdfIoQueueCreate failed 0x{:08x}",
            st as u32
        );
        return st;
    }

    // Manual queue: pended READ_REPORT requests are completed by the timer on fresh host input.
    // SAFETY: `device` is the live device just created.
    let manual_queue = match unsafe { skeleton::create_manual_queue(device) } {
        Ok(q) => q,
        Err(st) => {
            dbglog!(
                "[pf-mouse] manual WdfIoQueueCreate failed 0x{:08x}",
                st as u32
            );
            return st;
        }
    };
    MANUAL_QUEUE.store(manual_queue, Ordering::SeqCst);

    // Periodic timer (parent = manual queue): sealed-channel pump + health marks + event-driven
    // READ_REPORT completion. 8 ms — the proven pf-gamepad cadence; the mouse is presence-first
    // (SendInput injects), so a 125 Hz ceiling on the validation/report path is fine.
    // SAFETY: `manual_queue` is the live queue just created.
    let timer = unsafe { skeleton::create_periodic_timer(manual_queue.cast(), Some(evt_timer), 8) };
    if let Err(st) = timer {
        dbglog!("[pf-mouse] WdfTimerCreate failed 0x{:08x}", st as u32);
        return st;
    }

    log("[pf-mouse] device ready (HID mouse 5046:4D4F)");
    STATUS_SUCCESS
}

extern "C" fn evt_io_device_control(
    _queue: WDFQUEUE,
    request: WDFREQUEST,
    _output_len: usize,
    _input_len: usize,
    ioctl: ULONG,
) {
    // SAFETY: `request` is the live request for THIS EvtIoDeviceControl invocation — exactly the
    // contract `Request::new` requires. Everything after is safe (the token owns completion).
    let request = unsafe { Request::new(request) };

    // Skip the READ_REPORT cadence so the log stays readable; the descriptor handshake still logs.
    if ioctl != IOCTL_HID_READ_REPORT {
        dbglog!("[pf-mouse] ioctl 0x{ioctl:08x} out={_output_len} in={_input_len}");
    }

    // READ_REPORT forwards to the manual queue (the timer completes it on fresh input) — this
    // CONSUMES the request token, so it's handled apart from the status-and-complete paths below.
    if ioctl == IOCTL_HID_READ_REPORT {
        let mq: WDFQUEUE = MANUAL_QUEUE.load(Ordering::SeqCst);
        // SAFETY: `mq` is the manual queue created in EvtDeviceAdd (a live WDFQUEUE of this device).
        match unsafe { request.forward_to_queue(mq) } {
            Ok(()) => {}                        // framework owns it now (completed by the timer)
            Err((req, st)) => req.complete(st), // forward failed → complete with the error
        }
        return;
    }

    let status: NTSTATUS = match ioctl {
        IOCTL_HID_GET_DEVICE_DESCRIPTOR => request.copy_to_output(&HID_DESC),
        IOCTL_HID_GET_DEVICE_ATTRIBUTES => request.copy_to_output(&hid_attrs()),
        IOCTL_HID_GET_REPORT_DESCRIPTOR => request.copy_to_output(&MOUSE_RDESC),
        IOCTL_UMDF_HID_GET_INPUT_REPORT => {
            let report = INPUT_REPORT.lock().map(|g| *g).unwrap_or(NEUTRAL_REPORT);
            request.copy_to_output(&report)
        }
        // No output reports are declared; ack a stray write instead of failing the sender.
        IOCTL_HID_WRITE_REPORT | IOCTL_UMDF_HID_SET_OUTPUT_REPORT => STATUS_SUCCESS,
        IOCTL_HID_GET_STRING => on_get_string(&request),
        // The channel proof (see `pf_umdf_util::hid`): the host asks THIS devnode which process
        // serves it, and duplicates the DATA section into the answer — so it never has to trust the
        // LocalService-writable bootstrap mailbox to name its target.
        _ => STATUS_NOT_IMPLEMENTED,
    };

    dbglog!("[pf-mouse] ioctl 0x{ioctl:08x} -> 0x{:08x}", status as u32);
    request.complete(status);
}

// IOCTL_HID_GET_STRING: the input is a ULONG whose low word is the string id and whose high word
// is the language id. Windows polls ids 0x0E/0x0F/0x10 (manufacturer/product/serial) as well as
// the 0/1/2 HID_STRING_ID_* constants — serve both (the pf-gamepad finding).
fn on_get_string(request: &Request) -> NTSTATUS {
    let (bytes, _) = match request.input_bytes(4) {
        Ok(v) => v,
        Err(st) => return st,
    };
    let (_, string_id) = string_request_id(&bytes);
    let s: String = match string_id {
        0 | 0x000E => "Punktfunk".into(),
        // (2) The SERIAL carries the channel proof — the one transport measured to reach a UMDF HID
        // minidriver from user mode (`HidD_GetSerialNumberString`, zero-access handle, verified on
        // .173). Safe HERE and only here: nothing reads the virtual mouse's serial, whereas the pads'
        // serials are what SDL and Steam dedup controllers on. The old value was the inert
        // "PFMOUSE00"; the proof text is just as inert and does the security work.
        2 | 0x0010 => ChannelProof::new(CHANNEL.index(), std::process::id()).to_hid_string(),
        _ => "Punktfunk Virtual Mouse".into(),
    };
    request.copy_utf16z_to_output(&s)
}

extern "C" fn evt_timer(timer: WDFTIMER) {
    // One sealed-channel tick: publish our pid / adopt a delivery / detect host-gone (all safe,
    // via pf_umdf_util), then stamp the health marks the host watches.
    let Some(view) = CHANNEL.pump(&channel_cfg()) else {
        return; // host gone or not attached — nothing to deliver, nothing to mark
    };
    view.write_u32(OFF_DRIVER_PROTO, GAMEPAD_PROTO_VERSION);
    let hb = view.read_u32(OFF_DRIVER_HEARTBEAT).wrapping_add(1);
    view.write_u32(OFF_DRIVER_HEARTBEAT, hb);

    // Event-driven delivery: only when the host published a NEW report (in_seq advanced) does a
    // pended READ_REPORT complete. Acquire pairs with the host's Release bump, so the report bytes
    // read below are the ones that seq published. If no read is pended right now, DELIVERED_SEQ is
    // NOT advanced — the next tick retries while hidclass re-pends its reader.
    let seq = view.load_u32(OFF_IN_SEQ, Ordering::Acquire);
    if seq == 0 || seq == DELIVERED_SEQ.load(Ordering::Relaxed) {
        return;
    }
    // SAFETY-free queue access: the timer's parent object is the manual queue (set in
    // EvtDeviceAdd); the framework guarantees a live handle here.
    // SAFETY: see above — WdfTimerGetParentObject on the framework-provided live timer.
    let queue =
        unsafe { call_unsafe_wdf_function_binding!(WdfTimerGetParentObject, timer) } as WDFQUEUE;
    // SAFETY: `queue` is that live manual queue — the exact contract `retrieve_next_request` needs.
    let Some(request) = (unsafe { wdf::retrieve_next_request(queue) }) else {
        return; // no reader pended — retry next tick (seq stays undelivered)
    };
    let mut report = [0u8; MOUSE_REPORT_LEN];
    view.read_bytes(OFF_REPORT, &mut report);
    DELIVERED_SEQ.store(seq, Ordering::Relaxed);
    if report[0] == MOUSE_REPORT_ID {
        if let Ok(mut g) = INPUT_REPORT.lock() {
            *g = report;
        }
        let st = request.copy_to_output(&report);
        request.complete(st);
    } else {
        // A malformed publish (host bug / torn first write): the request is already retrieved,
        // so answer with the last good report, its relative wheel and pan zeroed — replayed,
        // they would scroll twice.
        let mut report = INPUT_REPORT.lock().map(|g| *g).unwrap_or(NEUTRAL_REPORT);
        report[6] = 0;
        report[7] = 0;
        let st = request.copy_to_output(&report);
        request.complete(st);
    }
}
