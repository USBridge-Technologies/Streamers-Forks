# Streamers-Forks

Forks of third-party streaming hosts that the USBridge agent can run, one
directory each, with the USBridge changes on top of an upstream snapshot.

| Directory | Upstream | Snapshot |
|---|---|---|
| `punktfunk/` | https://git.unom.io/unom/punktfunk | `6988e5c229ffc7d5c45ec472d24a24285959cdfb` (2026-10-02, 0.42.0) |
| `sunshine/` | https://github.com/LizardByte/Sunshine | `4429acd026f7942115c58aeaa7d0ea50a33d0ca9` |

Each directory keeps its upstream license. The first commit that adds a
directory is the untouched upstream tree; later commits are the USBridge
changes, so `git log -- <directory>` shows exactly what differs.

## USB through the USBridge USB broker

Both hosts get the same change. Under the USBridge agent
(`USBRIDGE_USB_BROKER_CONTROL` names the broker's control port) the host does
not build the client's USB devices itself; it forwards them to the broker's
`hid_stream` (rust-shine, `bin/usb-broker/src/hid_stream.rs`), which builds
them on USB/IP ports:

- a raw HID device the USBridge client sends over the control stream
  (`LiSendRawHidEvent`, a tablet), offered to the client with the
  `LI_FF_USBRIDGE_RAW_HID` feature flag;
- on Windows, gamepads, as Xbox 360 controllers on usbip-win2 (Sunshine
  has no other way to build them there, see below; elsewhere
  `USBRIDGE_PAD_BRIDGE=1` turns this on).

Plain USB/IP passthrough of a device needs nothing from the host: the broker
does it alone. Without the variable both hosts behave as upstream. The agent
tells a build that has the change from a stock one by asking it
(`punktfunk-host usbridge-bridge`, `sunshine --usbridge-bridge`).

## punktfunk

`punktfunk-host` hands a USBridge client's USB devices to the USBridge USB
broker instead of its own injectors: a raw HID device sent over the stream (a
tablet) and, on Windows, gamepads. See
`punktfunk/crates/punktfunk-host/src/gamestream/usbridge.rs`. Without
`USBRIDGE_USB_BROKER_CONTROL` in its environment the host behaves as upstream.

On Windows, upstream re-ACLs its config dir for the `%ProgramData%` dir a
SYSTEM service shares with the user: full control for SYSTEM and
Administrators only, read-only for Users. The agent instead runs the host
unelevated, with `PUNKTFUNK_CONFIG_DIR` in the user's own `%APPDATA%`, so that
DACL locked the host out of its own dir. Its first write
(`native-key.pem`) failed with "Access is denied" and it exited. The fork
hardens only dirs under `%ProgramData%`
(`punktfunk/crates/pf-paths/src/lib.rs`, `is_under_program_data`).

### Windows video: the shared MttVDD monitor, no pf-vdisplay driver

On Windows, upstream streams only from its own IddCx driver, `pf-vdisplay`. That
driver creates the virtual monitor and also encodes inside WUDFHost, so the host
must run as a SYSTEM service, and the driver needs an elevated install plus its
own certificate. The fork uses the virtual monitor every USBridge streamer
shares instead: MttVDD (VirtualDrivers' Virtual Display Driver, which RustShine
uses too). That is one driver for all streamers, with the host running as the
user.

The pieces:

- `pf-vdisplay/src/vdisplay/windows/mttvdd.rs` handles the monitor, ported from
  rust-shine's `virtual-display`:
  - attaches MttVDD in the mode closest to the client's (`SetDisplayConfig`);
  - makes it the primary display;
  - on session end, restores the old primary and detaches it.
- `pf-capture/src/windows/dxgi_dup.rs` captures it with DXGI Desktop
  Duplication into BGRA D3D11 textures:
  - the pointer is blended in (`CursorBlendPass`);
  - `wait_arrival` lets the encode loop follow the desktop's presents.
- `pf-encode/src/select/windows.rs` opens the host's own NVENC / AMF / QSV /
  Media Foundation encoder for those frames; upstream refused here.
- `punktfunk-host` has `CaptureBackend::DxgiDup`, picked whenever MttVDD is
  installed. `USBRIDGE_VDISPLAY=pf` goes back to pf-vdisplay.

The monitor has to be primary for 120 Hz: on a hybrid laptop, DWM composes at the
primary display's refresh, so a 120 Hz virtual monitor next to a 60 Hz primary
panel gets 60 frames a second. `USBRIDGE_VDD_PRIMARY=0` keeps the physical
primary anyway.

Measured on a Radeon 780M + RTX 3090 laptop at 2560x1600 HEVC, host side, 60 s
per run, with a D3D11 vsync animation on the virtual monitor:

| | fork on MttVDD | upstream on pf-vdisplay |
|---|---|---|
| 120 fps: frames sent / unique per s | 118.7 / 117.2 | 119.0 / 119.0 |
| 60 fps: frames sent / unique per s | 60.3 / 60.0 | 60.4 / 60.4 |
| capture, slowest per second (median) | 0.27 ms | 0.6 ms |

Build (Linux):

    cd punktfunk
    cargo build --release --locked -p punktfunk-host -p punktfunk-encode-worker \
        --features punktfunk-host/nvenc,punktfunk-host/vulkan-encode

## sunshine

The fork that used to live in `itsme228/Sunshine` (`web_bind_address`,
`usbridgeDisplayCursor`, no Vulkan encoder), plus the USB broker bridge in
`sunshine/src/usbridge.cpp`.

### Windows: signed usbip-win2 only, no libvirtualhid, no ViGEmBus

Upstream Sunshine on Windows needs third-party input drivers: libvirtualhid's
Virtual HID Driver, with its broker service and a paid license, or the
retired ViGEmBus. The fork drops both. On Windows the only driver involved
is usbip-win2, which is signed and already installed by the USBridge agent:

| Input | Upstream | Fork |
|---|---|---|
| Keyboard, mouse | libvirtualhid driver (licensed), `SendInput` fallback | `SendInput` |
| Touch, pen | libvirtualhid | synthetic pointer devices (Windows 10 1809+) |
| Gamepads | libvirtualhid driver (licensed) or ViGEmBus | USBridge USB broker: Xbox 360 pads on usbip-win2 |
| Tablets, other HID devices from a USBridge client | -- | USBridge USB broker on usbip-win2 |

Where the code is:

- `sunshine/src/platform/windows/input.cpp` holds the Windows input.
- `sunshine/src/usbridge.cpp` holds the broker bridge.

What was removed and what stays:

- `third-party/ViGEmClient` is no longer built, and its submodule is gone.
- Of libvirtualhid, only the platform-neutral core is compiled on Windows,
  with its "no backend" and "no license" stubs
  (`sunshine/cmake/compile_definitions/common.cmake`). This keeps the shared
  config and web UI code building unchanged. Nothing opens the driver, its
  broker service or the license server.

Without the USBridge agent (no `USBRIDGE_USB_BROKER_CONTROL`), Sunshine on
Windows has no gamepads. Keyboard, mouse, touch and pen work as before.

### Linux and macOS

These keep upstream libvirtualhid for keyboard, mouse and gamepads (uinput or
uhid on Linux; no driver and no license on either). Tablets and other HID
devices from a USBridge client still go to the USB broker:

- on Linux, through vhci-hcd;
- on macOS, through the USBridge USB/IP dongle.

### Build

Its submodules are registered in this repository's `.gitmodules`:

    git submodule update --init --recursive
    cmake -B sunshine/build -S sunshine -DCMAKE_BUILD_TYPE=Release -DBUILD_DOCS=OFF \
        -DSUNSHINE_ENABLE_VULKAN=OFF -DSUNSHINE_ENABLE_TRAY=OFF
    cmake --build sunshine/build

## Releases

Built in GitHub Actions only, never on a dev box: `sunshine-release.yml`
(Linux, Windows, macOS) and `punktfunk-release.yml` (Linux x86_64 host and
encode worker with NVENC; Windows x64 `punktfunk-host.exe`, no installer).
Both are manual: pick a platform (`all` by default) and give a tag to publish
a release, or leave the tag empty for a build-only test. From Actions -> the
workflow -> Run workflow, or from a terminal:

    scripts/release.sh punktfunk v0.42.0.usbridge.2            # all platforms
    scripts/release.sh punktfunk v0.42.0.usbridge.2 windows
    scripts/release.sh sunshine v2026.1003.1.usbridge linux
    scripts/release.sh punktfunk "" windows                    # build only
