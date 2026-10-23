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
//! - **Gamepads** ([`Pads`]): each controller becomes a virtual pad on a USB/IP port -- an
//!   Xbox 360 pad, or for a PlayStation one a DualShock 4, DualSense or DualSense Edge (the
//!   broker picks it from the arrival), with its touchpad, motion sensors, battery, light,
//!   adaptive triggers and, when the client plays it, its sound device. The controller
//!   extensions (`SS_CONTROLLER_ARRIVAL/TOUCH/MOTION/BATTERY`) go to the broker as they come
//!   ([`controller_frame`]); what a game sends back comes from it as control packets to send
//!   ([`Pads::pump`]). The default on Windows, where the agent installs usbip-win2 and not this
//!   host's pad drivers; `USBRIDGE_PAD_BRIDGE=1|0` overrides either way.
//!
//! Without the variable nothing here runs and nothing is advertised.

use punktfunk_core::input::{GamepadEvent, GamepadFrame};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::collections::HashMap;
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
const KIND_PAD_ARRIVAL: u8 = 0x13;
const KIND_PAD_TOUCH: u8 = 0x14;
const KIND_PAD_MOTION: u8 = 0x15;
const KIND_PAD_BATTERY: u8 = 0x16;
const KIND_PAD_LIGHT: u8 = 0x17;
const KIND_PAD_MOTION_REQUEST: u8 = 0x18;
const KIND_PAD_TRIGGERS: u8 = 0x19;
const KIND_PAD_PACKET: u8 = 0x1A;
/// The broker builds this many pads (a Moonlight client's whole `activeGamepadMask`).
const MAX_PADS: u16 = 16;

/// `LI_FF_CONTROLLER_TOUCH_EVENTS`: the client may send controller touch, motion and battery.
pub const SS_FF_CONTROLLER_TOUCH_EVENTS: u32 = 0x02;

/// Sunshine controller extensions (`Input.h`).
const MAGIC_CONTROLLER_ARRIVAL: u32 = 0x5500_0004;
const MAGIC_CONTROLLER_TOUCH: u32 = 0x5500_0005;
const MAGIC_CONTROLLER_MOTION: u32 = 0x5500_0006;
const MAGIC_CONTROLLER_BATTERY: u32 = 0x5500_0007;

/// Host -> client control packet types.
const RUMBLE_TYPE: u16 = 0x010B;
const SET_MOTION_EVENT_TYPE: u16 = 0x5501;
const SET_RGB_LED_TYPE: u16 = 0x5502;
const ADAPTIVE_TRIGGERS_TYPE: u16 = 0x5503;
const LI_MOTION_TYPE_ACCEL: u8 = 0x01;
const LI_MOTION_TYPE_GYRO: u8 = 0x02;

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

/// Opens a `hid_stream`. The reply says whether the broker builds raw HID devices; which of
/// them need a license (a Wacom tablet) is decided on its side, per device.
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

/// Whether to advertise [`SS_FF_USBRIDGE_RAW_HID`]: a broker is there and builds raw HID
/// devices. Asked per DESCRIBE, so a broker that comes up later needs no restart.
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

/// A controller extension packet as the broker's frame `(kind, controller, data)`: an arrival
/// as `type capabilities:LE16 supportedButtons:LE32`, a touch as `eventType pointerId x y`, a
/// motion sample as `motionType x y z`, a battery report as `state percent` (floats LE, as on
/// the wire). `None` for any other packet.
pub fn controller_frame(plaintext: &[u8]) -> Option<(u8, u8, Vec<u8>)> {
    if plaintext.len() < 4 || u16::from_le_bytes([plaintext[0], plaintext[1]]) != INPUT_DATA_TYPE {
        return None;
    }
    let p = plaintext.get(4..)?;
    let magic = u32::from_le_bytes(p.get(4..8)?.try_into().ok()?);
    let b = p.get(8..)?;
    let slot = *b.first()?;
    match magic {
        // controllerNumber type capabilities:16 supportedButtonFlags:32
        MAGIC_CONTROLLER_ARRIVAL => Some((KIND_PAD_ARRIVAL, slot, b.get(1..8)?.to_vec())),
        // controllerNumber eventType zero[2] pointerId:32 x y pressure
        MAGIC_CONTROLLER_TOUCH => {
            let mut d = vec![*b.get(1)?];
            d.extend_from_slice(b.get(4..16)?);
            Some((KIND_PAD_TOUCH, slot, d))
        }
        // controllerNumber motionType zero[2] x y z
        MAGIC_CONTROLLER_MOTION => {
            let mut d = vec![*b.get(1)?];
            d.extend_from_slice(b.get(4..16)?);
            Some((KIND_PAD_MOTION, slot, d))
        }
        // controllerNumber batteryState batteryPercentage zero
        MAGIC_CONTROLLER_BATTERY => Some((KIND_PAD_BATTERY, slot, b.get(1..3)?.to_vec())),
        _ => None,
    }
}

/// One host -> client control packet a broker pad produced, before sealing: `plaintext` is
/// `type:LE16 length:LE16 payload`. `channel` is where a pad's sound goes, unreliably; `None`
/// is the generic channel, reliably.
#[derive(Debug, PartialEq, Eq)]
pub struct Outgoing {
    pub plaintext: Vec<u8>,
    pub channel: Option<u8>,
}

fn control_plaintext(packet_type: u16, payload: &[u8]) -> Vec<u8> {
    let mut pt = Vec::with_capacity(4 + payload.len());
    pt.extend_from_slice(&packet_type.to_le_bytes());
    pt.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    pt.extend_from_slice(payload);
    pt
}

/// The broker's messages back: `kind slot` and a body whose size the kind says (a packet's
/// carries its own length). An unknown kind ends the reading: the stream can't be followed.
fn read_feedback(conn: &mut impl Read) -> Option<(u8, u8, Vec<u8>)> {
    let mut head = [0u8; 2];
    conn.read_exact(&mut head).ok()?;
    let size = match head[0] {
        KIND_PAD_RUMBLE | KIND_PAD_MOTION_REQUEST => 2,
        KIND_PAD_LIGHT => 3,
        KIND_PAD_TRIGGERS => 23,
        KIND_PAD_PACKET => 5,
        other => {
            tracing::warn!(kind = other, "usbridge: unknown message from the USB broker");
            return None;
        }
    };
    let mut body = vec![0u8; size];
    conn.read_exact(&mut body).ok()?;
    if head[0] == KIND_PAD_PACKET {
        let len = u16::from_le_bytes([body[3], body[4]]) as usize;
        let mut payload = vec![0u8; len];
        conn.read_exact(&mut payload).ok()?;
        body.extend_from_slice(&payload);
    }
    Some((head[0], head[1], body))
}

/// What a broker message becomes on the control stream.
fn outgoing(kind: u8, slot: u8, body: &[u8]) -> Vec<Outgoing> {
    let cn = u16::from(slot).to_le_bytes();
    let reliable = |packet_type: u16, payload: &[u8]| Outgoing {
        plaintext: control_plaintext(packet_type, payload),
        channel: None,
    };
    match kind {
        KIND_PAD_RUMBLE => {
            let wide = |v: u8| u16::from(v) << 8 | u16::from(v);
            let mut p = 0x00C0_FFEEu32.to_le_bytes().to_vec();
            p.extend_from_slice(&cn);
            p.extend_from_slice(&wide(body[0]).to_le_bytes());
            p.extend_from_slice(&wide(body[1]).to_le_bytes());
            vec![reliable(RUMBLE_TYPE, &p)]
        }
        KIND_PAD_LIGHT => vec![reliable(SET_RGB_LED_TYPE, &[cn[0], cn[1], body[0], body[1], body[2]])],
        // Both sensors, as Sunshine asks for a virtual DualShock 4's.
        KIND_PAD_MOTION_REQUEST => [LI_MOTION_TYPE_ACCEL, LI_MOTION_TYPE_GYRO]
            .into_iter()
            .map(|t| reliable(SET_MOTION_EVENT_TYPE, &[cn[0], cn[1], body[0], body[1], t]))
            .collect(),
        KIND_PAD_TRIGGERS => {
            let mut p = cn.to_vec();
            p.extend_from_slice(body);
            vec![reliable(ADAPTIVE_TRIGGERS_TYPE, &p)]
        }
        KIND_PAD_PACKET => vec![Outgoing {
            plaintext: control_plaintext(u16::from_le_bytes([body[1], body[2]]), &body[5..]),
            channel: Some(body[0]),
        }],
        _ => Vec::new(),
    }
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
    /// What the broker sends back, read on a thread of its own so the control loop never
    /// waits on the socket.
    feedback: Option<Receiver<(u8, u8, Vec<u8>)>>,
    retry_at: Option<Instant>,
}

impl Link {
    fn to(broker: Option<SocketAddr>) -> Link {
        Link {
            broker,
            conn: None,
            feedback: None,
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
                    self.feedback = conn.try_clone().ok().map(spawn_feedback_reader);
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
        self.feedback = None;
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.close();
    }
}

fn spawn_feedback_reader(mut conn: TcpStream) -> Receiver<(u8, u8, Vec<u8>)> {
    let (tx, rx) = mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("usbridge-feedback".into())
        .spawn(move || {
            while let Some(msg) = read_feedback(&mut conn) {
                if tx.send(msg).is_err() {
                    break;
                }
            }
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

/// The session's gamepads as virtual pads on the broker's side.
pub struct Pads {
    link: Link,
    /// Controllers the broker holds a pad for.
    live: u16,
    /// Each controller's arrival frame, sent again on a new connection (the broker forgot it).
    arrivals: HashMap<u8, Vec<u8>>,
}

impl Pads {
    pub fn new() -> Pads {
        Pads::to(broker_addr())
    }

    fn to(broker: Option<SocketAddr>) -> Pads {
        Pads {
            link: Link::to(broker),
            live: 0,
            arrivals: HashMap::new(),
        }
    }

    /// Sends one frame; a connection this opens first gets the arrivals kept so far.
    fn send(&mut self, bytes: &[u8]) -> bool {
        if self.link.conn.is_none() && self.link.connect().is_some() {
            let replay: Vec<Vec<u8>> = self.arrivals.values().cloned().collect();
            for a in replay {
                self.link.send(&a);
            }
        }
        self.link.send(bytes)
    }

    /// One [`controller_frame`]: an arrival says which pad the controller becomes (kept for a
    /// reconnect); touch, motion and battery go to a pad the broker holds.
    pub fn extension(&mut self, (kind, slot, data): (u8, u8, Vec<u8>)) {
        if u16::from(slot) >= MAX_PADS {
            return;
        }
        let f = frame(kind, slot, &data);
        if kind == KIND_PAD_ARRIVAL {
            self.send(&f);
            self.arrivals.insert(slot, f);
        } else if self.live & (1 << slot) != 0 {
            self.send(&f);
        }
    }

    pub fn handle(&mut self, ev: &GamepadEvent) {
        // Arrivals come through `extension`, with their supported buttons.
        let GamepadEvent::State(f) = ev else {
            return;
        };
        for index in 0..MAX_PADS {
            let bit = 1 << index;
            if self.live & bit != 0 && f.active_mask & bit == 0 {
                self.live &= !bit;
                // Only a pad that was there: an arrival can come a packet before its mask.
                self.arrivals.remove(&(index as u8));
                self.send(&frame(KIND_PAD_GONE, index as u8, &[]));
            }
        }
        let Ok(index) = u16::try_from(f.index) else {
            return;
        };
        if index >= MAX_PADS || f.active_mask & (1 << index) == 0 {
            return;
        }
        if self.send(&frame(KIND_PAD_STATE, index as u8, &pad_state(f))) {
            self.live |= 1 << index;
        } else {
            // The connection went, and every pad with it.
            self.live = 0;
        }
    }

    /// Everything the broker passed back since the last call, as control packets: rumble,
    /// light, motion requests, adaptive triggers, and the pads' sound.
    pub fn pump(&mut self, mut out: impl FnMut(Outgoing)) {
        let Some(rx) = self.link.feedback.as_ref() else {
            return;
        };
        while let Ok((kind, slot, body)) = rx.try_recv() {
            outgoing(kind, slot, &body).into_iter().for_each(&mut out);
        }
    }

    /// `(index, low, high, 0, 0)` per rumble the broker passed back, motors as `0..=0xFFFF`;
    /// everything else it sent is dropped (no client to send it to).
    pub fn pump_rumble(&mut self, mut rumble: impl FnMut(u16, u16, u16, u16, u16)) {
        let Some(rx) = self.link.feedback.as_ref() else {
            return;
        };
        while let Ok((kind, slot, body)) = rx.try_recv() {
            if kind == KIND_PAD_RUMBLE {
                let wide = |v: u8| u16::from(v) << 8 | u16::from(v);
                rumble(u16::from(slot), wide(body[0]), wide(body[1]), 0, 0);
            }
        }
    }
}

/// `buttons:LE16 leftTrigger rightTrigger lx ly rx ry buttons2:LE16` — the XInput half of the
/// frame, then the extended buttons (paddles, touchpad click, misc).
fn pad_state(f: &GamepadFrame) -> [u8; 14] {
    let mut d = [0u8; 14];
    d[12..14].copy_from_slice(&((f.buttons >> 16) as u16).to_le_bytes());
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
        // Controller 1 is gone from the mask; controller 20 is one the broker has no pad for.
        pads.handle(&state(20, 0, 0));

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
            &[0x00, 0x10, 7, 0, 0, 0, 0xFE, 0xFF, 0, 0, 0, 0, 0x01, 0x00],
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

    #[test]
    fn controller_extensions_become_broker_frames() {
        let mut arrival = vec![3u8, 2];
        arrival.extend_from_slice(&0x8000u16.to_le_bytes());
        arrival.extend_from_slice(&0x0020_FFFFu32.to_le_bytes());
        let (kind, slot, d) = controller_frame(&input_packet(MAGIC_CONTROLLER_ARRIVAL, &arrival)).unwrap();
        assert_eq!((kind, slot), (KIND_PAD_ARRIVAL, 3));
        assert_eq!(d, &arrival[1..]);

        let mut touch = vec![1u8, 1, 0, 0];
        touch.extend_from_slice(&9u32.to_le_bytes());
        touch.extend_from_slice(&0.5f32.to_le_bytes());
        touch.extend_from_slice(&0.25f32.to_le_bytes());
        touch.extend_from_slice(&1f32.to_le_bytes());
        let (kind, _, d) = controller_frame(&input_packet(MAGIC_CONTROLLER_TOUCH, &touch)).unwrap();
        assert_eq!(kind, KIND_PAD_TOUCH);
        assert_eq!(d.len(), 13);
        assert_eq!(&d[1..5], &9u32.to_le_bytes());

        let (kind, _, d) = controller_frame(&input_packet(MAGIC_CONTROLLER_BATTERY, &[1, 2, 50, 0])).unwrap();
        assert_eq!((kind, d), (KIND_PAD_BATTERY, vec![2, 50]));
        assert!(controller_frame(&input_packet(UB_RAW_HID_MAGIC, &[0; 16])).is_none());
    }

    #[test]
    fn broker_messages_become_control_packets() {
        let mut stream: &[u8] = &[KIND_PAD_LIGHT, 2, 1, 2, 3, KIND_PAD_PACKET, 2, 8, 0x01, 0x56, 2, 0, 7, 9];
        let (k, s, b) = read_feedback(&mut stream).unwrap();
        assert_eq!(
            outgoing(k, s, &b),
            vec![Outgoing { plaintext: vec![0x02, 0x55, 5, 0, 2, 0, 1, 2, 3], channel: None }]
        );
        let (k, s, b) = read_feedback(&mut stream).unwrap();
        assert_eq!(
            outgoing(k, s, &b),
            vec![Outgoing { plaintext: vec![0x01, 0x56, 2, 0, 7, 9], channel: Some(8) }]
        );
        let mut motion: &[u8] = &[KIND_PAD_MOTION_REQUEST, 4, 250, 0];
        let (k, s, b) = read_feedback(&mut motion).unwrap();
        let out = outgoing(k, s, &b);
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].plaintext, vec![0x01, 0x55, 5, 0, 4, 0, 250, 0, LI_MOTION_TYPE_GYRO]);
        let mut unknown: &[u8] = &[0x7F, 0];
        assert!(read_feedback(&mut unknown).is_none());
    }
}
