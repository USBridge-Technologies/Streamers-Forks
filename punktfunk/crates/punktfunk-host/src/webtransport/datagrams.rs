//! [`Transport`] over one browser's WebTransport datagrams, host side.
//!
//! Send-only. The session pump is synchronous, on its own thread, and calls `send_gso` from the
//! pacer; `Connection::send_datagram` is itself non-blocking, so this is one call deep. The
//! pump's read side is never used on the host — every inbound datagram (input, mic, reports) is
//! read by the session's own loop through `SessionLink::read_datagram`, on both carriers — so
//! `recv` here is honestly empty rather than a second consumer racing that loop.
//!
//! Nothing here knows about video. The whole point of `Transport` being a trait is that
//! `punktfunk_core::session::Session` cannot tell this from a UDP socket.

use punktfunk_core::transport::Transport;
use std::io;
use wtransport::Connection;

/// The pump's view of one browser connection.
pub(crate) struct WebTransportPlane {
    conn: Connection,
}

impl WebTransportPlane {
    pub(crate) fn new(conn: Connection) -> WebTransportPlane {
        WebTransportPlane { conn }
    }
}

impl Transport for WebTransportPlane {
    /// `Ok(false)` for a datagram the connection would not take — the same lossy contract a full
    /// UDP send buffer has, which the caller counts and FEC covers.
    fn send(&self, packet: &[u8]) -> io::Result<bool> {
        Ok(self.conn.send_datagram(packet).is_ok())
    }

    /// Nothing, always: the session loop owns inbound datagrams (see the module doc).
    fn recv(&self) -> io::Result<Option<Vec<u8>>> {
        Ok(None)
    }
}
