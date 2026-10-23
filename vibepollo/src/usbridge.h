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
 * - **Gamepads**: each controller becomes a virtual pad on a USB/IP port -- an Xbox 360 pad, or
 *   for a PlayStation one a DualShock 4, DualSense or DualSense Edge (the broker picks it from the
 *   arrival), with its touchpad, motion sensors, battery, light, adaptive triggers and, when the
 *   client plays it, its sound device (speaker/headset and the DualSense's haptics). The default
 *   on Windows, where this is the only way the fork builds pads (no libvirtualhid driver, no
 *   ViGEmBus, see src/platform/windows/input.cpp); `USBRIDGE_PAD_BRIDGE=1|0` overrides it.
 *   What a game sends back to a pad comes from the broker as feedback (`feedback_t`); the pad's
 *   sound comes as ready control packets to encrypt and send unreliably on their ENet channel.
 *
 * Without the variable nothing here runs and nothing is advertised.
 */
#pragma once

// standard includes
#include <array>
#include <cstddef>
#include <cstdint>
#include <functional>
#include <memory>
#include <vector>

namespace usbridge {
  /// `UB_RAW_HID_MAGIC`: one chunk of a raw HID device (`LiSendRawHidEvent`).
  constexpr std::uint32_t RAW_HID_MAGIC = 0x55420001;
  /// `LI_FF_USBRIDGE_RAW_HID`: this host takes `LiSendRawHidEvent`. Stock clients ignore it.
  constexpr std::uint32_t FEATURE_FLAG_RAW_HID = 0x10000;
  /// What `vibepollo --usbridge-bridge` prints: the agent asks a binary whether it has this module.
  constexpr const char *PROBE_LINE = "usbridge-bridge 1";
  /// The broker builds this many pads (a Moonlight client's whole `activeGamepadMask`).
  constexpr int MAX_PADS = 16;

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
   * @brief Whether this host runs under the USBridge agent (it named the USB broker).
   * The agent then owns pairing (a client it pairs gets every permission, as on Sunshine)
   * and the displays (it attaches the shared MttVDD monitor; no display helper here).
   */
  bool under_agent();

  /**
   * @brief Whether a pad's sound came from the broker in the last second: the control loop then
   * runs every few milliseconds, so the packets go out as they come.
   */
  bool sound_active();

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
    /// What a game sends back to the broker's pads. Called on a thread of its own; any may be empty.
    struct feedback_t {
      /// (controller, low, high), motors as 0..0xFFFF.
      std::function<void(std::uint8_t, std::uint16_t, std::uint16_t)> rumble;
      /// (controller, r, g, b): the pad's light.
      std::function<void(std::uint8_t, std::uint8_t, std::uint8_t, std::uint8_t)> light;
      /// (controller, rate in Hz, 0 to stop): the pad wants the client's motion sensors.
      std::function<void(std::uint8_t, std::uint16_t)> motion;
      /// (controller, event flags, type left, type right, left, right): `IDX_SET_ADAPTIVE_TRIGGERS`.
      std::function<void(std::uint8_t, std::uint8_t, std::uint8_t, std::uint8_t, const std::array<std::uint8_t, 10> &, const std::array<std::uint8_t, 10> &)> triggers;
      /// (controller, ENet channel, packet type, payload): a control packet of the pad's sound, to
      /// encrypt and send unreliably on that channel.
      std::function<void(std::uint8_t, std::uint8_t, std::uint16_t, std::vector<std::uint8_t>)> packet;
    };

    session_t();
    ~session_t();
    session_t(const session_t &) = delete;
    session_t &operator=(const session_t &) = delete;

    /// Where what a game writes to a pad goes. Replaces the previous target.
    void set_feedback(feedback_t fb);

    /// Forward one raw HID frame (`raw_hid_frame_size` bytes of the packet body).
    void raw_hid(const std::uint8_t *frame, std::size_t size);

    /// A controller's arrival (`SS_CONTROLLER_ARRIVAL`): which pad it becomes. Before its first state.
    void pad_arrival(std::uint8_t slot, std::uint8_t type, std::uint16_t capabilities, std::uint32_t supported_buttons);

    /// One controller snapshot in Moonlight's layout (positive Y is up), `buttons` with the extended
    /// half (`buttonFlags2 << 16`). The pad appears with the first one.
    void pad_state(std::uint8_t slot, std::uint32_t buttons, std::uint8_t lt, std::uint8_t rt, std::int16_t lx, std::int16_t ly, std::int16_t rx, std::int16_t ry);

    /// `SS_CONTROLLER_TOUCH`: x, y across the touchpad, 0..1.
    void pad_touch(std::uint8_t slot, std::uint8_t event_type, std::uint32_t pointer_id, float x, float y);

    /// `SS_CONTROLLER_MOTION`: m/s^2 or degrees per second.
    void pad_motion(std::uint8_t slot, std::uint8_t motion_type, float x, float y, float z);

    /// `SS_CONTROLLER_BATTERY`.
    void pad_battery(std::uint8_t slot, std::uint8_t state, std::uint8_t percent);

    /// Unplug a controller.
    void pad_gone(std::uint8_t slot);

    /// Drop the connection: the broker unplugs everything it carried. The next frame starts over.
    void close();

  private:
    struct impl_t;
    std::unique_ptr<impl_t> impl;
  };
}  // namespace usbridge
