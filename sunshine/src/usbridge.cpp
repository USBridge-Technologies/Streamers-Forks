/**
 * @file src/usbridge.cpp
 * @brief Definitions for the USBridge USB broker bridge. Plain sockets and the standard library
 * only, so it builds and tests on its own (`USBRIDGE_STANDALONE`).
 */
// standard includes
#include <chrono>
#include <cstdlib>
#include <cstring>
#include <mutex>
#include <string>
#include <string_view>
#include <thread>

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

    constexpr auto CONNECT_TIMEOUT = 300ms;
    constexpr auto REPLY_TIMEOUT = 500ms;
    constexpr auto WRITE_TIMEOUT = 200ms;
    constexpr auto RETRY_INTERVAL = 2s;

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
    std::thread reader;  ///< Rumble the broker sends back, read so that sending never waits on it.
    std::chrono::steady_clock::time_point retry_at {};
    std::uint16_t live = 0;  ///< Controllers the broker holds a pad for.

    std::mutex rumble_mutex;
    rumble_fn rumble;

    void read_rumble(sock_t s) {
      std::uint8_t msg[4];
      std::size_t have = 0;
      while (true) {
        const auto n = recv(s, reinterpret_cast<char *>(msg) + have, static_cast<int>(sizeof(msg) - have), 0);
        if (n <= 0) {
          return;
        }
        have += static_cast<std::size_t>(n);
        if (have < sizeof(msg)) {
          continue;
        }
        have = 0;
        if (msg[0] != KIND_PAD_RUMBLE) {
          continue;
        }
        const auto wide = [](std::uint8_t v) {
          return static_cast<std::uint16_t>((v << 8) | v);
        };
        std::lock_guard lg {rumble_mutex};
        if (rumble) {
          rumble(msg[1], wide(msg[2]), wide(msg[3]));
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
          read_rumble(s);
        }};
      }
      if (!send_all(sock, data, size)) {
        USBRIDGE_LOG(warning) << "usbridge: USB broker connection lost"sv;
        close_locked();
        retry_at = std::chrono::steady_clock::now() + RETRY_INTERVAL;
        return false;
      }
      return true;
    }

    bool send_frame_locked(std::uint8_t kind, std::uint8_t slot, const std::uint8_t *data, std::uint16_t size) {
      std::uint8_t frame[FRAME_HEADER_LEN + 16] {};
      frame[0] = kind;
      frame[1] = slot;
      frame[4] = frame[8] = static_cast<std::uint8_t>(size & 0xFF);
      frame[5] = frame[9] = static_cast<std::uint8_t>(size >> 8);
      if (size > 0) {
        std::memcpy(frame + FRAME_HEADER_LEN, data, size);
      }
      return send_locked(frame, FRAME_HEADER_LEN + size);
    }
  };

  session_t::session_t():
      impl {std::make_unique<impl_t>()} {
  }

  session_t::~session_t() {
    close();
  }

  void session_t::set_rumble(rumble_fn fn) {
    std::lock_guard lg {impl->rumble_mutex};
    impl->rumble = std::move(fn);
  }

  void session_t::raw_hid(const std::uint8_t *frame, std::size_t size) {
    std::lock_guard lg {impl->mutex};
    impl->send_locked(frame, size);
  }

  void session_t::pad_state(std::uint8_t slot, std::uint16_t buttons, std::uint8_t lt, std::uint8_t rt, std::int16_t lx, std::int16_t ly, std::int16_t rx, std::int16_t ry) {
    if (slot >= MAX_PADS) {
      return;
    }
    const auto le = [](std::uint8_t *at, std::uint16_t v) {
      at[0] = static_cast<std::uint8_t>(v & 0xFF);
      at[1] = static_cast<std::uint8_t>(v >> 8);
    };
    // The XInput half of the state; the extended buttons have no place on an Xbox 360 pad.
    std::uint8_t d[12];
    le(d, buttons);
    d[2] = lt;
    d[3] = rt;
    le(d + 4, static_cast<std::uint16_t>(lx));
    le(d + 6, static_cast<std::uint16_t>(ly));
    le(d + 8, static_cast<std::uint16_t>(rx));
    le(d + 10, static_cast<std::uint16_t>(ry));

    std::lock_guard lg {impl->mutex};
    if (impl->send_frame_locked(KIND_PAD_STATE, slot, d, sizeof(d))) {
      impl->live |= static_cast<std::uint16_t>(1U << slot);
    }
  }

  void session_t::pad_gone(std::uint8_t slot) {
    if (slot >= MAX_PADS) {
      return;
    }
    std::lock_guard lg {impl->mutex};
    const auto bit = static_cast<std::uint16_t>(1U << slot);
    if (impl->live & bit) {
      impl->live &= static_cast<std::uint16_t>(~bit);
      impl->send_frame_locked(KIND_PAD_GONE, slot, nullptr, 0);
    }
  }

  void session_t::close() {
    std::lock_guard lg {impl->mutex};
    impl->close_locked();
  }
}  // namespace usbridge
