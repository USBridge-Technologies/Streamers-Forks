/**
 * @file src/usbridge.h
 * @brief USBridge agent integration: the client's USB devices are built by the USBridge USB broker.
 *
 * When Sunshine runs under the USBridge agent, `USBRIDGE_USB_BROKER_CONTROL` names the broker's
 * control port on this machine. Two things then go to it instead of an injector here, over a
 * `hid_stream` connection (rust-shine `bin/usb-broker/src/hid_stream.rs`):
 *
 * - **Raw HID** (`RAW_HID_MAGIC`, the USBridge moonlight-common-c fork): a client's HID device
 *   (a Wacom tablet) sent as its model plus every input report. The body is forwarded verbatim;
 *   the broker rebuilds the device on a virtual USB port so the native driver binds. Offered to
 *   the client with `FEATURE_FLAG_RAW_HID`.
 * - **Gamepads**: each controller becomes a virtual Xbox 360 pad on a USB/IP port. The default
 *   on Windows, where this is the only way the fork builds pads (no libvirtualhid driver, no
 *   ViGEmBus, see src/platform/windows/input.cpp); `USBRIDGE_PAD_BRIDGE=1|0` overrides it.
 *
 * Without the variable nothing here runs and nothing is advertised.
 */
#pragma once

// standard includes
#include <cstddef>
#include <cstdint>
#include <functional>
#include <memory>

namespace usbridge {
  /// `UB_RAW_HID_MAGIC`: one chunk of a raw HID device (`LiSendRawHidEvent`).
  constexpr std::uint32_t RAW_HID_MAGIC = 0x55420001;
  /// `LI_FF_USBRIDGE_RAW_HID`: this host takes `LiSendRawHidEvent`. Stock clients ignore it.
  constexpr std::uint32_t FEATURE_FLAG_RAW_HID = 0x10000;
  /// What `sunshine --usbridge-bridge` prints: the agent asks a binary whether it has this module.
  constexpr const char *PROBE_LINE = "usbridge-bridge 1";
  /// The broker builds this many pads.
  constexpr int MAX_PADS = 4;

  /**
   * @brief Whether to advertise `FEATURE_FLAG_RAW_HID`: a broker is there and builds raw HID devices.
   * Asked per DESCRIBE, so a broker that comes up later needs no restart.
   */
  bool raw_hid_offered();

  /**
   * @brief Whether this host's gamepads are the broker's.
   */
  bool pads_via_broker();

  /**
   * @brief Length of the frame the broker takes from a raw HID packet body.
   * @param body The packet after its header: `kind slot endpoint 0 total:LE16 offset:LE16 length:LE16 data`.
   * @param size Bytes available in `body`.
   * @return The frame length, or 0 when the declared length runs past the packet.
   */
  std::size_t raw_hid_frame_size(const std::uint8_t *body, std::size_t size);

  /**
   * @brief One streaming client's devices on the broker. Closing it unplugs them.
   */
  class session_t {
  public:
    /// Called on a thread of its own with (controller, low, high), motors as 0..0xFFFF.
    using rumble_fn = std::function<void(std::uint8_t, std::uint16_t, std::uint16_t)>;

    session_t();
    ~session_t();
    session_t(const session_t &) = delete;
    session_t &operator=(const session_t &) = delete;

    /// Where the rumble a game writes to a pad goes. Replaces the previous target.
    void set_rumble(rumble_fn fn);

    /// Forward one raw HID frame (`raw_hid_frame_size` bytes of the packet body).
    void raw_hid(const std::uint8_t *frame, std::size_t size);

    /// One controller snapshot in Moonlight's layout (positive Y is up). The pad appears with the first one.
    void pad_state(std::uint8_t slot, std::uint16_t buttons, std::uint8_t lt, std::uint8_t rt, std::int16_t lx, std::int16_t ly, std::int16_t rx, std::int16_t ry);

    /// Unplug a controller.
    void pad_gone(std::uint8_t slot);

    /// Drop the connection: the broker unplugs everything it carried. The next frame starts over.
    void close();

  private:
    struct impl_t;
    std::unique_ptr<impl_t> impl;
  };
}  // namespace usbridge
