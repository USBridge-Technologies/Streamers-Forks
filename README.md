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
- on Windows, gamepads, as Xbox 360 controllers on usbip-win2
  (`USBRIDGE_PAD_BRIDGE=1|0` forces either way).

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

Build (Linux):

    cd punktfunk
    cargo build --release --locked -p punktfunk-host -p punktfunk-encode-worker \
        --features punktfunk-host/nvenc,punktfunk-host/vulkan-encode

## sunshine

The fork that used to live in `itsme228/Sunshine` (`web_bind_address`,
`usbridgeDisplayCursor`, no Vulkan encoder), plus the USB broker bridge in
`sunshine/src/usbridge.cpp`.

On Windows the fork has no libvirtualhid, so it needs no Virtual HID Driver,
no libvirtualhid broker service and no license:

- keyboard and mouse use `SendInput`;
- touch and pen use synthetic pointer devices (Windows 10 1809+);
- gamepads under the USBridge agent are the USB broker's Xbox 360 pads on
  usbip-win2;
- ViGEmBus is only a fallback for Sunshine running without the agent.

See `sunshine/src/platform/windows/input.cpp`. Only libvirtualhid's
platform-neutral core is compiled there, with its "no backend" and "no
license" stubs (`sunshine/cmake/compile_definitions/common.cmake`), so the
shared config and web UI code builds unchanged. Linux and macOS keep upstream
libvirtualhid.

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
