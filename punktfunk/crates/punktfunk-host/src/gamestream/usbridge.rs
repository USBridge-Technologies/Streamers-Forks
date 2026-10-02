//! USBridge agent integration: the client's USB devices are built by the USBridge USB broker.
//!
//! When this host runs under the USBridge agent, `USBRIDGE_USB_BROKER_CONTROL` names the
//! broker's control port on this machine. Two things then go to it instead of an injector
//! here, over a `hid_stream` connection (rust-shine `bin/usb-broker/src/hid_stream.rs`):
//!
//! - **Raw HID** (`UB_RAW_HID_MAGIC`, the USBridge moonlight-common-c fork): a client's HID
//!   device (a Wacom tablet) sent as its model plus every input report. The body is forwarded
//!   verbatim; the broker rebuilds the device on a virtual USB port so the native driver binds.
//!   Offered to the client with [`SS_FF_USBRIDGE_RAW_HID`].
//! - **Gamepads** ([`Pads`]): each controller becomes a virtual Xbox 360 pad on a USB/IP port.
//!   The default on Windows, where the agent installs usbip-win2 and not this host's pad
//!   drivers; `USBRIDGE_PAD_BRIDGE=1|0` overrides either way.
//!
//! Without the variable nothing here runs and nothing is advertised.

use punktfunk_core::input::{GamepadEvent, GamepadFrame};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

const BROKER_ENV: &str = "USBRIDGE_USB_BROKER_CONTROL";
const PAD_ENV: &str = "USBRIDGE_PAD_BRIDGE";

/// `LI_FF_USBRIDGE_RAW_HID`: this host takes `LiSendRawHidEvent`. Stock clients ignore it.
pub const SS_FF_USBRIDGE_RAW_HID: u32 = 0x10000;

/// Same inner type as [`super::input`].
const INPUT_DATA_TYPE: u16 = 0x0206;
const UB_RAW_HID_MAGIC: u32 = 0x5542_0001;
/// `kind slot endpoint reserved total:LE16 offset:LE16 length:LE16`, then the data.
const FRAME_HEADER_LEN: usize = 10;

const KIND_PAD_STATE: u8 = 0x10;
const KIND_PAD_GONE: u8 = 0x11;
const KIND_PAD_RUMBLE: u8 = 0x12;
/// The broker builds this many pads.
const MAX_PADS: u16 = 4;

const CONNECT_TIMEOUT: Duration = Duration::from_millis(300);
const REPLY_TIMEOUT: Duration = Duration::from_millis(500);
const WRITE_TIMEOUT: Duration = Duration::from_millis(200);
const RETRY_INTERVAL: Duration = Duration::from_secs(2);

fn broker_addr() -> Option<SocketAddr> {
    std::env::var(BROKER_ENV).ok()?.trim().parse().ok()
}

/// What `punktfunk-host usbridge-bridge` prints: the agent asks a binary whether it has this
/// module before it tells a client that raw HID works.
pub const PROBE_LINE: &str = "usbridge-bridge 1";

/// Opens a `hid_stream`. The reply says whether the broker builds raw HID devices right now
/// (a license matter on its side).
fn open(addr: SocketAddr) -> std::io::Result<(TcpStream, bool)> {
    let mut conn = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)?;
    conn.set_nodelay(true)?;
    conn.set_read_timeout(Some(REPLY_TIMEOUT))?;
    conn.set_write_timeout(Some(WRITE_TIMEOUT))?;
    conn.write_all(b"{\"cmd\":\"hid_stream\"}\n")?;
    let mut reply = String::new();
    // Byte at a time: nothing after the line may be consumed.
    let mut reader = BufReader::with_capacity(1, conn.try_clone()?);
    reader.read_line(&mut reply)?;
    if !reply.contains("\"ok\":true") {
        return Err(std::io::Error::other(format!(
            "the USB broker refused hid_stream: {}",
            reply.trim()
        )));
    }
    conn.set_read_timeout(None)?;
    Ok((conn, reply.contains("\"raw_hid\":true")))
}

/// Whether to advertise [`SS_FF_USBRIDGE_RAW_HID`]: a broker is there and would build a
/// device now. Asked per DESCRIBE, so a license change needs no restart.
pub fn raw_hid_offered() -> bool {
    broker_addr().is_some_and(builds_raw_hid)
}

fn builds_raw_hid(broker: SocketAddr) -> bool {
    matches!(open(broker), Ok((_, true)))
}

/// Whether this session's gamepads are the broker's.
pub fn pads_via_broker() -> bool {
    if broker_addr().is_none() {
        return false;
    }
    match std::env::var(PAD_ENV).ok().as_deref().map(str::trim) {
        Some("1") => true,
        Some("0") => false,
        _ => cfg!(target_os = "windows"),
    }
}

/// The body of a raw HID packet (`kind slot endpoint 0 total offset length data`), exactly as
/// the broker takes it. `None` for anything else, and for a length that runs past the packet.
pub fn raw_hid_frame(plaintext: &[u8]) -> Option<&[u8]> {
    if plaintext.len() < 4 || u16::from_le_bytes([plaintext[0], plaintext[1]]) != INPUT_DATA_TYPE {
        return None;
    }
    let p = plaintext.get(4..)?;
    if u32::from_le_bytes(p.get(4..8)?.try_into().ok()?) != UB_RAW_HID_MAGIC {
        return None;
    }
    let body = &p[8..];
    let length = u16::from_le_bytes([*body.get(8)?, *body.get(9)?]) as usize;
    body.get(..FRAME_HEADER_LEN + length)
}

fn frame(kind: u8, slot: u8, data: &[u8]) -> Vec<u8> {
    let len = (data.len() as u16).to_le_bytes();
    let mut f = Vec::with_capacity(FRAME_HEADER_LEN + data.len());
    f.extend_from_slice(&[kind, slot, 0, 0, len[0], len[1], 0, 0, len[0], len[1]]);
    f.extend_from_slice(data);
    f
}

/// One `hid_stream` connection, opened on first use. Dropping it unplugs what it carried.
struct Link {
    broker: Option<SocketAddr>,
    conn: Option<TcpStream>,
    /// Rumble the broker sends back, read on a thread of its own so the control loop
    /// never waits on the socket.
    rumble: Option<Receiver<[u8; 4]>>,
    retry_at: Option<Instant>,
}

impl Link {
    fn to(broker: Option<SocketAddr>) -> Link {
        Link {
            broker,
            conn: None,
            rumble: None,
            retry_at: None,
        }
    }

    fn connect(&mut self) -> Option<&mut TcpStream> {
        if self.conn.is_none() {
            if self.retry_at.is_some_and(|at| Instant::now() < at) {
                return None;
            }
            match self.broker.map(open) {
                Some(Ok((conn, _))) => {
                    self.rumble = conn.try_clone().ok().map(spawn_rumble_reader);
                    self.conn = Some(conn);
                    self.retry_at = None;
                    tracing::info!("usbridge: connected to the USB broker");
                }
                Some(Err(e)) => {
                    tracing::warn!(error = %e, "usbridge: USB broker unreachable — the device does not reach this machine");
                    self.retry_at = Some(Instant::now() + RETRY_INTERVAL);
                }
                None => self.retry_at = Some(Instant::now() + RETRY_INTERVAL),
            }
        }
        self.conn.as_mut()
    }

    /// `false` when the frame did not go out. A broken connection is dropped: the broker
    /// unplugged everything with it, so the next frame starts over on a new one.
    fn send(&mut self, bytes: &[u8]) -> bool {
        let Some(conn) = self.connect() else {
            return false;
        };
        if let Err(e) = conn.write_all(bytes) {
            tracing::warn!(error = %e, "usbridge: USB broker connection lost");
            self.close();
            self.retry_at = Some(Instant::now() + RETRY_INTERVAL);
            return false;
        }
        true
    }

    fn close(&mut self) {
        if let Some(conn) = self.conn.take() {
            let _ = conn.shutdown(Shutdown::Both);
        }
        self.rumble = None;
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.close();
    }
}

fn spawn_rumble_reader(mut conn: TcpStream) -> Receiver<[u8; 4]> {
    let (tx, rx) = mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("usbridge-rumble".into())
        .spawn(move || {
            let mut msg = [0u8; 4];
            while conn.read_exact(&mut msg).is_ok() && tx.send(msg).is_ok() {}
        });
    rx
}

/// The session's raw HID devices.
pub struct RawHid {
    link: Link,
}

impl RawHid {
    pub fn new() -> RawHid {
        RawHid {
            link: Link::to(broker_addr()),
        }
    }

    /// Forward one [`raw_hid_frame`].
    pub fn forward(&mut self, frame: &[u8]) {
        self.link.send(frame);
    }
}

/// The session's gamepads as virtual Xbox 360 pads on the broker's side.
pub struct Pads {
    link: Link,
    /// Controllers the broker holds a pad for.
    live: u16,
}

impl Pads {
    pub fn new() -> Pads {
        Pads::to(broker_addr())
    }

    fn to(broker: Option<SocketAddr>) -> Pads {
        Pads {
            link: Link::to(broker),
            live: 0,
        }
    }

    pub fn handle(&mut self, ev: &GamepadEvent) {
        // An arrival only describes the controller; the pad is always an Xbox 360 one.
        let GamepadEvent::State(f) = ev else {
            return;
        };
        for index in 0..MAX_PADS {
            let bit = 1 << index;
            if self.live & bit != 0 && f.active_mask & bit == 0 {
                self.live &= !bit;
                self.link.send(&frame(KIND_PAD_GONE, index as u8, &[]));
            }
        }
        let Ok(index) = u16::try_from(f.index) else {
            return;
        };
        if index >= MAX_PADS || f.active_mask & (1 << index) == 0 {
            return;
        }
        if self.link.send(&frame(KIND_PAD_STATE, index as u8, &pad_state(f))) {
            self.live |= 1 << index;
        } else {
            // The connection went, and every pad with it.
            self.live = 0;
        }
    }

    /// `(index, low, high, 0, 0)` per rumble the broker passed back, motors as `0..=0xFFFF`.
    pub fn pump_rumble(&mut self, mut rumble: impl FnMut(u16, u16, u16, u16, u16)) {
        let Some(rx) = self.link.rumble.as_ref() else {
            return;
        };
        while let Ok([kind, slot, left, right]) = rx.try_recv() {
            if kind == KIND_PAD_RUMBLE {
                let wide = |v: u8| u16::from(v) << 8 | u16::from(v);
                rumble(u16::from(slot), wide(left), wide(right), 0, 0);
            }
        }
    }
}

/// `buttons:LE16 leftTrigger rightTrigger lx ly rx ry` — the XInput half of the frame; the
/// extended buttons (paddles, Share) have no place on an Xbox 360 pad.
fn pad_state(f: &GamepadFrame) -> [u8; 12] {
    let mut d = [0u8; 12];
    d[0..2].copy_from_slice(&(f.buttons as u16).to_le_bytes());
    d[2] = f.left_trigger;
    d[3] = f.right_trigger;
    d[4..6].copy_from_slice(&f.ls_x.to_le_bytes());
    d[6..8].copy_from_slice(&f.ls_y.to_le_bytes());
    d[8..10].copy_from_slice(&f.rs_x.to_le_bytes());
    d[10..12].copy_from_slice(&f.rs_y.to_le_bytes());
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn input_packet(magic: u32, body: &[u8]) -> Vec<u8> {
        let mut inp = Vec::new();
        inp.extend_from_slice(&((4 + body.len()) as u32).to_be_bytes());
        inp.extend_from_slice(&magic.to_le_bytes());
        inp.extend_from_slice(body);
        let mut pt = INPUT_DATA_TYPE.to_le_bytes().to_vec();
        pt.extend_from_slice(&(inp.len() as u16).to_le_bytes());
        pt.extend_from_slice(&inp);
        pt
    }

    #[test]
    fn raw_hid_frame_is_the_packet_body() {
        // A report of 3 bytes on endpoint 0x81, slot 2.
        let body = [1, 2, 0x81, 0, 3, 0, 0, 0, 3, 0, 0x10, 0x61, 0x7f];
        assert_eq!(
            raw_hid_frame(&input_packet(UB_RAW_HID_MAGIC, &body)),
            Some(&body[..])
        );
        // Trailing bytes past `length` are not part of the frame.
        let mut padded = body.to_vec();
        padded.push(0xEE);
        assert_eq!(
            raw_hid_frame(&input_packet(UB_RAW_HID_MAGIC, &padded)),
            Some(&body[..])
        );
        // A length that runs past the packet is refused, not truncated.
        let short = [1, 0, 0x81, 0, 9, 0, 0, 0, 9, 0, 1, 2];
        assert_eq!(raw_hid_frame(&input_packet(UB_RAW_HID_MAGIC, &short)), None);
        assert_eq!(raw_hid_frame(&input_packet(0x0C, &body)), None);
    }

    /// A broker that accepts one `hid_stream`, answers `reply`, and returns what it was sent.
    fn fake_broker(reply: &'static str) -> (SocketAddr, std::thread::JoinHandle<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let served = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let mut line = String::new();
            let mut reader = BufReader::new(conn.try_clone().unwrap());
            reader.read_line(&mut line).unwrap();
            assert_eq!(line, "{\"cmd\":\"hid_stream\"}\n");
            conn.write_all(reply.as_bytes()).unwrap();
            conn.write_all(&[KIND_PAD_RUMBLE, 1, 0xFF, 0x80]).unwrap();
            let mut rest = Vec::new();
            // A caller that leaves without reading the rumble resets the connection.
            let _ = reader.read_to_end(&mut rest);
            rest
        });
        (addr, served)
    }

    #[test]
    fn pads_follow_the_active_mask_and_rumble_comes_back() {
        let (addr, served) = fake_broker("{\"ok\":true,\"raw_hid\":false}\n");
        let mut pads = Pads::to(Some(addr));
        let state = |index, active_mask, buttons| {
            GamepadEvent::State(GamepadFrame {
                index,
                active_mask,
                buttons,
                left_trigger: 7,
                ls_y: -2,
                ..Default::default()
            })
        };
        pads.handle(&state(1, 0b10, 0x0001_1000));
        // Controller 1 is gone from the mask; controller 7 is one the broker has no pad for.
        pads.handle(&state(7, 0b1000_0000, 0));

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut got = None;
        while got.is_none() && Instant::now() < deadline {
            pads.pump_rumble(|i, low, high, _, _| got = Some((i, low, high)));
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(got, Some((1, 0xFFFF, 0x8080)));

        drop(pads);
        let mut want = frame(
            KIND_PAD_STATE,
            1,
            &[0x00, 0x10, 7, 0, 0, 0, 0xFE, 0xFF, 0, 0, 0, 0],
        );
        want.extend(frame(KIND_PAD_GONE, 1, &[]));
        assert_eq!(served.join().unwrap(), want);
    }

    #[test]
    fn raw_hid_is_offered_only_when_the_broker_would_build_it() {
        for (reply, offered) in [
            ("{\"ok\":true,\"raw_hid\":true}\n", true),
            ("{\"ok\":true,\"raw_hid\":false}\n", false),
            ("{\"ok\":false,\"error\":\"unknown cmd hid_stream\"}\n", false),
        ] {
            let (addr, served) = fake_broker(reply);
            assert_eq!(builds_raw_hid(addr), offered, "{reply}");
            served.join().unwrap();
        }
    }

    /// With no broker a pad event goes nowhere and nothing blocks.
    #[test]
    fn without_a_broker_nothing_is_sent() {
        let mut pads = Pads::to(None);
        pads.handle(&GamepadEvent::State(GamepadFrame {
            index: 0,
            active_mask: 1,
            ..Default::default()
        }));
        assert_eq!(pads.live, 0);
        pads.pump_rumble(|_, _, _, _, _| panic!("no rumble without a broker"));
    }
}
