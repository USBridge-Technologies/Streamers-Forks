/**
 * @file src/usbridge.cpp
 * @brief Definitions for the USBridge USB broker bridge. Plain sockets and the standard library
 * only, so it builds and tests on its own (`USBRIDGE_STANDALONE`).
 */
// standard includes
#include <atomic>
#include <chrono>
#include <cstdlib>
#include <cstring>
#include <mutex>
#include <string>
#include <string_view>
#include <thread>
#include <vector>

// platform includes
#ifdef _WIN32
  #include <winsock2.h>
  #include <ws2tcpip.h>
#else
  #include <arpa/inet.h>
  #include <fcntl.h>
  #include <netinet/in.h>
  #include <netinet/tcp.h>
  #include <poll.h>
  #include <sys/socket.h>
  #include <unistd.h>
#endif

// local includes
#include "usbridge.h"

#ifdef USBRIDGE_STANDALONE
  #include <iostream>
  #define USBRIDGE_LOG(level) std::cerr << std::endl \
                                        << "[" #level "] "
#else
  #include "logging.h"
  #define USBRIDGE_LOG(level) BOOST_LOG(level)
#endif

using namespace std::literals;

namespace usbridge {
  namespace {
    constexpr auto BROKER_ENV = "USBRIDGE_USB_BROKER_CONTROL";
    constexpr auto PAD_ENV = "USBRIDGE_PAD_BRIDGE";

    /// `kind slot endpoint reserved total:LE16 offset:LE16 length:LE16`, then the data.
    constexpr std::size_t FRAME_HEADER_LEN = 10;
    constexpr std::uint8_t KIND_PAD_STATE = 0x10;
    constexpr std::uint8_t KIND_PAD_GONE = 0x11;
    constexpr std::uint8_t KIND_PAD_RUMBLE = 0x12;
    constexpr std::uint8_t KIND_PAD_ARRIVAL = 0x13;
    constexpr std::uint8_t KIND_PAD_TOUCH = 0x14;
    constexpr std::uint8_t KIND_PAD_MOTION = 0x15;
    constexpr std::uint8_t KIND_PAD_BATTERY = 0x16;
    constexpr std::uint8_t KIND_PAD_LIGHT = 0x17;
    constexpr std::uint8_t KIND_PAD_MOTION_REQUEST = 0x18;
    constexpr std::uint8_t KIND_PAD_TRIGGERS = 0x19;
    constexpr std::uint8_t KIND_PAD_PACKET = 0x1A;

    void put16(std::uint8_t *at, std::uint16_t v) {
      at[0] = static_cast<std::uint8_t>(v & 0xFF);
      at[1] = static_cast<std::uint8_t>(v >> 8);
    }

    void put32(std::uint8_t *at, std::uint32_t v) {
      put16(at, static_cast<std::uint16_t>(v & 0xFFFF));
      put16(at + 2, static_cast<std::uint16_t>(v >> 16));
    }

    void put_float(std::uint8_t *at, float f) {
      std::uint32_t v;
      static_assert(sizeof(v) == sizeof(f));
      std::memcpy(&v, &f, sizeof(v));
      put32(at, v);
    }

    std::uint16_t get16(const std::uint8_t *at) {
      return static_cast<std::uint16_t>(at[0] | (at[1] << 8));
    }

    constexpr auto CONNECT_TIMEOUT = 300ms;
    constexpr auto REPLY_TIMEOUT = 500ms;
    constexpr auto WRITE_TIMEOUT = 200ms;
    constexpr auto RETRY_INTERVAL = 2s;

    /// When a pad's sound last came, in steady-clock ticks.
    std::atomic<std::int64_t> last_sound {0};

#ifdef _WIN32
    using sock_t = SOCKET;
    constexpr sock_t NO_SOCK = INVALID_SOCKET;

    void close_sock(sock_t s) {
      closesocket(s);
    }

    void shutdown_sock(sock_t s) {
      shutdown(s, SD_BOTH);
    }

    void set_blocking(sock_t s, bool blocking) {
      u_long mode = blocking ? 0 : 1;
      ioctlsocket(s, FIONBIO, &mode);
    }

    void net_init() {
      static std::once_flag once;
      std::call_once(once, [] {
        WSADATA data;
        WSAStartup(MAKEWORD(2, 2), &data);
      });
    }
#else
    using sock_t = int;
    constexpr sock_t NO_SOCK = -1;

    void close_sock(sock_t s) {
      ::close(s);
    }

    void shutdown_sock(sock_t s) {
      shutdown(s, SHUT_RDWR);
    }

    void set_blocking(sock_t s, bool blocking) {
      const int flags = fcntl(s, F_GETFL, 0);
      fcntl(s, F_SETFL, blocking ? flags & ~O_NONBLOCK : flags | O_NONBLOCK);
    }

    void net_init() {
    }
#endif

    /// Waits until `s` is readable (or writable). False on timeout.
    bool wait_for(sock_t s, bool writable, std::chrono::milliseconds timeout) {
#ifndef _WIN32
      pollfd fd {s, static_cast<short>(writable ? POLLOUT : POLLIN), 0};
      return poll(&fd, 1, static_cast<int>(timeout.count())) > 0;
#else
      fd_set set;
      FD_ZERO(&set);
      FD_SET(s, &set);
      timeval tv {};
      tv.tv_sec = static_cast<long>(timeout.count() / 1000);
      tv.tv_usec = static_cast<long>((timeout.count() % 1000) * 1000);
      return select(0, writable ? nullptr : &set, writable ? &set : nullptr, nullptr, &tv) > 0;
#endif
    }

    /// Sends everything or fails; never waits longer than `WRITE_TIMEOUT` on a stalled broker.
    bool send_all(sock_t s, const std::uint8_t *data, std::size_t size) {
      while (size > 0) {
        if (!wait_for(s, true, WRITE_TIMEOUT)) {
          return false;
        }
#ifdef MSG_NOSIGNAL
        const auto n = send(s, reinterpret_cast<const char *>(data), static_cast<int>(size), MSG_NOSIGNAL);
#else
        const auto n = send(s, reinterpret_cast<const char *>(data), static_cast<int>(size), 0);
#endif
        if (n <= 0) {
          return false;
        }
        data += n;
        size -= static_cast<std::size_t>(n);
      }
      return true;
    }

    /// The broker's control address, when the agent gave one: `127.0.0.1:18090`.
    bool broker_addr(sockaddr_in &addr) {
      const char *env = std::getenv(BROKER_ENV);
      if (!env) {
        return false;
      }
      const std::string value {env};
      const auto colon = value.rfind(':');
      if (colon == std::string::npos) {
        return false;
      }
      const int port = std::atoi(value.c_str() + colon + 1);
      addr = {};
      addr.sin_family = AF_INET;
      addr.sin_port = htons(static_cast<std::uint16_t>(port));
      return port > 0 && port < 65536 && inet_pton(AF_INET, value.substr(0, colon).c_str(), &addr.sin_addr) == 1;
    }

    /**
     * Opens a `hid_stream`. The reply says whether the broker builds raw HID devices; which of
     * them need a license (a Wacom tablet) is decided on its side, per device.
     */
    sock_t open_stream(bool &builds_raw_hid, std::string &error) {
      builds_raw_hid = false;
      sockaddr_in addr {};
      if (!broker_addr(addr)) {
        error = "no broker address";
        return NO_SOCK;
      }
      net_init();
      const sock_t s = socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
      if (s == NO_SOCK) {
        error = "socket";
        return NO_SOCK;
      }
      const auto fail = [&](const char *what) {
        error = what;
        close_sock(s);
        return NO_SOCK;
      };

      set_blocking(s, false);
      connect(s, reinterpret_cast<const sockaddr *>(&addr), sizeof(addr));
      int so_error = 0;
      socklen_t len = sizeof(so_error);
      if (!wait_for(s, true, CONNECT_TIMEOUT) || getsockopt(s, SOL_SOCKET, SO_ERROR, reinterpret_cast<char *>(&so_error), &len) != 0 || so_error != 0) {
        return fail("the USB broker is not listening");
      }
      const int nodelay = 1;
      setsockopt(s, IPPROTO_TCP, TCP_NODELAY, reinterpret_cast<const char *>(&nodelay), sizeof(nodelay));

      constexpr std::string_view request = "{\"cmd\":\"hid_stream\"}\n";
      if (!send_all(s, reinterpret_cast<const std::uint8_t *>(request.data()), request.size())) {
        return fail("the USB broker did not take the request");
      }

      // Byte at a time: nothing after the line may be consumed, it is the first rumble.
      std::string reply;
      const auto deadline = std::chrono::steady_clock::now() + REPLY_TIMEOUT;
      while (reply.empty() || reply.back() != '\n') {
        const auto left = std::chrono::duration_cast<std::chrono::milliseconds>(deadline - std::chrono::steady_clock::now());
        char c = 0;
        if (left <= 0ms || reply.size() > 512 || !wait_for(s, false, left) || recv(s, &c, 1, 0) != 1) {
          return fail("the USB broker did not answer hid_stream");
        }
        reply.push_back(c);
      }
      if (reply.find("\"ok\":true") == std::string::npos) {
        error = "the USB broker refused hid_stream: " + reply.substr(0, reply.size() - 1);
        close_sock(s);
        return NO_SOCK;
      }
      builds_raw_hid = reply.find("\"raw_hid\":true") != std::string::npos;
      // The rumble reader blocks in recv; a send still waits for room first (send_all).
      set_blocking(s, true);
      return s;
    }
  }  // namespace

  bool raw_hid_offered() {
    bool builds = false;
    std::string error;
    const sock_t s = open_stream(builds, error);
    if (s == NO_SOCK) {
      return false;
    }
    shutdown_sock(s);
    close_sock(s);
    return builds;
  }

  bool pads_via_broker() {
    static const bool enabled = [] {
      sockaddr_in addr {};
      if (!broker_addr(addr)) {
        return false;
      }
      const char *env = std::getenv(PAD_ENV);
      if (env && env[0] == '1') {
        return true;
      }
      if (env && env[0] == '0') {
        return false;
      }
#ifdef _WIN32
      return true;
#else
      return false;
#endif
    }();
    return enabled;
  }

  bool sound_active() {
    const auto last = std::chrono::steady_clock::time_point(std::chrono::steady_clock::duration(last_sound.load()));
    return std::chrono::steady_clock::now() - last < 1s;
  }

  std::size_t raw_hid_frame_size(const std::uint8_t *body, std::size_t size) {
    if (size < FRAME_HEADER_LEN) {
      return 0;
    }
    const std::size_t length = static_cast<std::size_t>(body[8]) | (static_cast<std::size_t>(body[9]) << 8);
    return FRAME_HEADER_LEN + length <= size ? FRAME_HEADER_LEN + length : 0;
  }

  /// One `hid_stream` connection, opened on first use.
  struct session_t::impl_t {
    std::mutex mutex;  ///< Guards the socket and what goes out on it.
    sock_t sock = NO_SOCK;
    std::thread reader;  ///< Feedback the broker sends back, read so that sending never waits on it.
    std::chrono::steady_clock::time_point retry_at {};
    std::uint16_t live = 0;  ///< Controllers the broker holds a pad for.
    /// Each controller's arrival, sent again on a new connection (the broker forgot it).
    std::array<std::vector<std::uint8_t>, MAX_PADS> arrivals;

    std::mutex feedback_mutex;
    feedback_t feedback;

    /// Reads exactly `size` bytes. False when the connection is gone.
    static bool recv_all(sock_t s, std::uint8_t *data, std::size_t size) {
      while (size > 0) {
        const auto n = recv(s, reinterpret_cast<char *>(data), static_cast<int>(size), 0);
        if (n <= 0) {
          return false;
        }
        data += n;
        size -= static_cast<std::size_t>(n);
      }
      return true;
    }

    /// The broker's messages: `kind slot` and a body whose size the kind says (a packet's carries
    /// its own length). An unknown kind ends the reading; the stream can't be followed past it.
    void read_feedback(sock_t s) {
      std::uint8_t head[2];
      std::vector<std::uint8_t> body;
      while (recv_all(s, head, sizeof(head))) {
        const auto kind = head[0];
        const auto slot = head[1];
        std::size_t size = 0;
        switch (kind) {
          case KIND_PAD_RUMBLE:
          case KIND_PAD_MOTION_REQUEST:
            size = 2;
            break;
          case KIND_PAD_LIGHT:
            size = 3;
            break;
          case KIND_PAD_TRIGGERS:
            size = 23;
            break;
          case KIND_PAD_PACKET:
            size = 5;
            break;
          default:
            USBRIDGE_LOG(warning) << "usbridge: unknown message 0x"sv << std::hex << static_cast<int>(kind) << std::dec << " from the USB broker"sv;
            return;
        }
        body.resize(size);
        if (!recv_all(s, body.data(), size)) {
          return;
        }
        std::vector<std::uint8_t> payload;
        if (kind == KIND_PAD_PACKET) {
          payload.resize(get16(body.data() + 3));
          if (!recv_all(s, payload.data(), payload.size())) {
            return;
          }
        }
        const auto wide = [](std::uint8_t v) {
          return static_cast<std::uint16_t>((v << 8) | v);
        };
        std::lock_guard lg {feedback_mutex};
        const auto &fb = feedback;
        switch (kind) {
          case KIND_PAD_RUMBLE:
            if (fb.rumble) {
              fb.rumble(slot, wide(body[0]), wide(body[1]));
            }
            break;
          case KIND_PAD_MOTION_REQUEST:
            if (fb.motion) {
              fb.motion(slot, get16(body.data()));
            }
            break;
          case KIND_PAD_LIGHT:
            if (fb.light) {
              fb.light(slot, body[0], body[1], body[2]);
            }
            break;
          case KIND_PAD_TRIGGERS:
            if (fb.triggers) {
              std::array<std::uint8_t, 10> left {};
              std::array<std::uint8_t, 10> right {};
              std::memcpy(left.data(), body.data() + 3, left.size());
              std::memcpy(right.data(), body.data() + 13, right.size());
              fb.triggers(slot, body[0], body[1], body[2], left, right);
            }
            break;
          case KIND_PAD_PACKET:
            last_sound = std::chrono::steady_clock::now().time_since_epoch().count();
            if (fb.packet) {
              fb.packet(slot, body[0], get16(body.data() + 1), std::move(payload));
            }
            break;
        }
      }
    }

    /// With `mutex` held.
    void close_locked() {
      if (sock == NO_SOCK) {
        return;
      }
      shutdown_sock(sock);
      if (reader.joinable()) {
        reader.join();
      }
      close_sock(sock);
      sock = NO_SOCK;
      live = 0;
    }

    /// With `mutex` held. False when the frame did not go out; a broken connection is dropped,
    /// the broker unplugged everything with it, and the next frame starts over on a new one.
    bool send_locked(const std::uint8_t *data, std::size_t size) {
      if (sock == NO_SOCK) {
        const auto now = std::chrono::steady_clock::now();
        if (now < retry_at) {
          return false;
        }
        bool builds = false;
        std::string error;
        sock = open_stream(builds, error);
        if (sock == NO_SOCK) {
          USBRIDGE_LOG(warning) << "usbridge: "sv << error << " -- the device does not reach this machine"sv;
          retry_at = now + RETRY_INTERVAL;
          return false;
        }
        USBRIDGE_LOG(info) << "usbridge: connected to the USB broker"sv;
        reader = std::thread {[this, s = sock] {
          read_feedback(s);
        }};
        for (std::size_t slot = 0; slot < arrivals.size(); ++slot) {
          if (!arrivals[slot].empty()) {
            const auto frame = make_frame(KIND_PAD_ARRIVAL, static_cast<std::uint8_t>(slot), arrivals[slot].data(), static_cast<std::uint16_t>(arrivals[slot].size()));
            send_all(sock, frame.data(), frame.size());
          }
        }
      }
      if (!send_all(sock, data, size)) {
        USBRIDGE_LOG(warning) << "usbridge: USB broker connection lost"sv;
        close_locked();
        retry_at = std::chrono::steady_clock::now() + RETRY_INTERVAL;
        return false;
      }
      return true;
    }

    static std::vector<std::uint8_t> make_frame(std::uint8_t kind, std::uint8_t slot, const std::uint8_t *data, std::uint16_t size) {
      std::vector<std::uint8_t> frame(FRAME_HEADER_LEN + size);
      frame[0] = kind;
      frame[1] = slot;
      put16(frame.data() + 4, size);
      put16(frame.data() + 8, size);
      if (size > 0) {
        std::memcpy(frame.data() + FRAME_HEADER_LEN, data, size);
      }
      return frame;
    }

    bool send_frame_locked(std::uint8_t kind, std::uint8_t slot, const std::uint8_t *data, std::uint16_t size) {
      const auto frame = make_frame(kind, slot, data, size);
      return send_locked(frame.data(), frame.size());
    }
  };

  session_t::session_t():
      impl {std::make_unique<impl_t>()} {
  }

  session_t::~session_t() {
    close();
  }

  void session_t::set_feedback(feedback_t fb) {
    std::lock_guard lg {impl->feedback_mutex};
    impl->feedback = std::move(fb);
  }

  void session_t::raw_hid(const std::uint8_t *frame, std::size_t size) {
    std::lock_guard lg {impl->mutex};
    impl->send_locked(frame, size);
  }

  void session_t::pad_arrival(std::uint8_t slot, std::uint8_t type, std::uint16_t capabilities, std::uint32_t supported_buttons) {
    if (slot >= MAX_PADS) {
      return;
    }
    std::uint8_t d[7];
    d[0] = type;
    put16(d + 1, capabilities);
    put32(d + 3, supported_buttons);
    std::lock_guard lg {impl->mutex};
    // Kept after sending: a connection this opens replays the ones kept before it.
    impl->send_frame_locked(KIND_PAD_ARRIVAL, slot, d, sizeof(d));
    impl->arrivals[slot].assign(d, d + sizeof(d));
  }

  void session_t::pad_state(std::uint8_t slot, std::uint32_t buttons, std::uint8_t lt, std::uint8_t rt, std::int16_t lx, std::int16_t ly, std::int16_t rx, std::int16_t ry) {
    if (slot >= MAX_PADS) {
      return;
    }
    // The XInput half of the state, then the extended buttons (paddles, touchpad, misc).
    std::uint8_t d[14];
    put16(d, static_cast<std::uint16_t>(buttons & 0xFFFF));
    d[2] = lt;
    d[3] = rt;
    put16(d + 4, static_cast<std::uint16_t>(lx));
    put16(d + 6, static_cast<std::uint16_t>(ly));
    put16(d + 8, static_cast<std::uint16_t>(rx));
    put16(d + 10, static_cast<std::uint16_t>(ry));
    put16(d + 12, static_cast<std::uint16_t>(buttons >> 16));

    std::lock_guard lg {impl->mutex};
    if (impl->send_frame_locked(KIND_PAD_STATE, slot, d, sizeof(d))) {
      impl->live |= static_cast<std::uint16_t>(1U << slot);
    }
  }

  void session_t::pad_touch(std::uint8_t slot, std::uint8_t event_type, std::uint32_t pointer_id, float x, float y) {
    if (slot >= MAX_PADS) {
      return;
    }
    std::uint8_t d[13];
    d[0] = event_type;
    put32(d + 1, pointer_id);
    put_float(d + 5, x);
    put_float(d + 9, y);
    std::lock_guard lg {impl->mutex};
    if (impl->live & (1U << slot)) {
      impl->send_frame_locked(KIND_PAD_TOUCH, slot, d, sizeof(d));
    }
  }

  void session_t::pad_motion(std::uint8_t slot, std::uint8_t motion_type, float x, float y, float z) {
    if (slot >= MAX_PADS) {
      return;
    }
    std::uint8_t d[13];
    d[0] = motion_type;
    put_float(d + 1, x);
    put_float(d + 5, y);
    put_float(d + 9, z);
    std::lock_guard lg {impl->mutex};
    if (impl->live & (1U << slot)) {
      impl->send_frame_locked(KIND_PAD_MOTION, slot, d, sizeof(d));
    }
  }

  void session_t::pad_battery(std::uint8_t slot, std::uint8_t state, std::uint8_t percent) {
    if (slot >= MAX_PADS) {
      return;
    }
    const std::uint8_t d[2] {state, percent};
    std::lock_guard lg {impl->mutex};
    if (impl->live & (1U << slot)) {
      impl->send_frame_locked(KIND_PAD_BATTERY, slot, d, sizeof(d));
    }
  }

  void session_t::pad_gone(std::uint8_t slot) {
    if (slot >= MAX_PADS) {
      return;
    }
    std::lock_guard lg {impl->mutex};
    const auto bit = static_cast<std::uint16_t>(1U << slot);
    if (impl->live & bit) {
      // Only a pad that was there: an arrival can come a packet before the mask that has it.
      impl->arrivals[slot].clear();
      impl->live &= static_cast<std::uint16_t>(~bit);
      impl->send_frame_locked(KIND_PAD_GONE, slot, nullptr, 0);
    }
  }

  void session_t::close() {
    std::lock_guard lg {impl->mutex};
    impl->close_locked();
  }
}  // namespace usbridge
