//! Play with friends for vanilla Minecraft. The invite code's tunnel carries
//! the vanilla protocol unchanged, so only its two ends are missing outside
//! Horizon: `join` puts a friend's code behind a local port a vanilla 26.3
//! client connects to like to any server, and `host` (main.rs) puts a
//! vanilla server or LAN world behind a code for Horizon players. The design
//! is in docs/horizon-connect.md.
use horizon_core::version::{PROTOCOL_VERSION, VANILLA_VERSION};
use horizon_friends::{
    PathMode,
    guest::{self, GuestConfig},
};
use std::{
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    sync::Arc,
    time::Duration,
};

/// What a client's handshake asks for (`ClientIntentionPacket`'s intention).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intent {
    /// The server list's ping.
    Status,
    /// Joining, or a transfer from another server.
    Login,
    /// The legacy server list ping (0xFE) or bytes that aren't a handshake.
    Other,
}

/// The longest handshake frame read: protocol, a 255-character address,
/// port and intention fit well within it.
const MAX_HANDSHAKE: usize = 1024;
/// How long a client may take to send its handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

pub struct JoinConfig {
    /// The invite code as pasted, or a relay's short code.
    pub code: String,
    /// The relay for a short code (a full code names its own).
    pub relay: Option<String>,
    pub path: PathMode,
}

/// Where a line for the player goes (stdout in the program, nowhere in tests).
pub type Log = fn(&str);

/// Accept vanilla clients on `listener` until it fails. Every connection,
/// a server list ping as much as a join, gets its own session with the
/// friend's world, as Horizon's own client opens one per connection.
pub fn serve_join(listener: TcpListener, config: JoinConfig, log: Log) -> io::Result<()> {
    let config = Arc::new(config);
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        // Only this computer's game: whoever reaches the port plays in the
        // friend's world under the code.
        if !stream.peer_addr().is_ok_and(|a| a.ip().is_loopback()) {
            continue;
        }
        let config = config.clone();
        std::thread::Builder::new()
            .name("horizon-connect-client".into())
            .spawn(move || {
                if let Err(e) = carry_client(stream, &config, log) {
                    log(&format!("connection dropped: {e}"));
                }
            })?;
    }
    Ok(())
}

/// One vanilla connection: read its handshake, reach the friend's world,
/// then carry the stream; when the world can't be reached, say why the way
/// the client shows it (the server list's description, or the disconnect
/// screen).
fn carry_client(mut stream: TcpStream, config: &JoinConfig, log: Log) -> io::Result<()> {
    stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
    let (handshake, intent) = read_handshake(&mut stream)?;
    if intent == Intent::Other {
        return Ok(());
    }
    let joined = guest::join(&GuestConfig {
        code: config.code.clone(),
        relay: config.relay.clone(),
        protocol: PROTOCOL_VERSION,
        game: VANILLA_VERSION.to_owned(),
        path: config.path,
    });
    match joined {
        Ok(tunnel) => {
            if intent == Intent::Login {
                log(if tunnel.direct {
                    "Joining your friend's world (connected straight to your friend)."
                } else {
                    "Joining your friend's world (through the relay)."
                });
            }
            stream.set_read_timeout(None)?;
            let done = tunnel.carry(stream, &handshake)?;
            let _ = done.join();
            if intent == Intent::Login {
                log("Left your friend's world.");
            }
            Ok(())
        }
        Err(e) => {
            let reason = e.to_string();
            if intent == Intent::Login {
                log(&format!("Couldn't join: {reason}"));
            }
            refuse(&mut stream, intent, &reason)
        }
    }
}

/// Read the client's first frame: the bytes as read (to forward them
/// unchanged) and what it asks for.
pub fn read_handshake(stream: &mut impl Read) -> io::Result<(Vec<u8>, Intent)> {
    let mut raw = Vec::new();
    let mut length = 0usize;
    for i in 0..3 {
        let mut byte = [0u8; 1];
        stream.read_exact(&mut byte)?;
        raw.push(byte[0]);
        if i == 0 && byte[0] == 0xFE {
            return Ok((raw, Intent::Other));
        }
        length |= usize::from(byte[0] & 0x7F) << (7 * i);
        if byte[0] & 0x80 == 0 {
            break;
        }
        if i == 2 {
            return Ok((raw, Intent::Other));
        }
    }
    if length == 0 || length > MAX_HANDSHAKE {
        return Ok((raw, Intent::Other));
    }
    let start = raw.len();
    raw.resize(start + length, 0);
    stream.read_exact(&mut raw[start..])?;
    let intent = parse_intent(&raw[start..]);
    Ok((raw, intent))
}

/// The intention of a handshake packet's body: id 0, protocol, address,
/// port, intention (1 status, 2 login, 3 transfer).
pub fn parse_intent(body: &[u8]) -> Intent {
    let mut at = 0;
    let parsed = (|| {
        if read_varint(body, &mut at)? != 0 {
            return None;
        }
        read_varint(body, &mut at)?;
        let address = usize::try_from(read_varint(body, &mut at)?).ok()?;
        at = at.checked_add(address)?.checked_add(2)?;
        read_varint(body, &mut at)
    })();
    match parsed {
        Some(1) => Intent::Status,
        Some(2 | 3) => Intent::Login,
        _ => Intent::Other,
    }
}

/// Tell the client why the friend's world can't be reached: a status
/// response whose description is the reason (then the ping's pong), or a
/// login disconnect.
pub fn refuse(stream: &mut TcpStream, intent: Intent, reason: &str) -> io::Result<()> {
    match intent {
        Intent::Status => {
            let status = serde_json::json!({
                "version": { "name": VANILLA_VERSION, "protocol": PROTOCOL_VERSION },
                "players": { "max": 0, "online": 0 },
                "description": { "text": reason },
            });
            stream.write_all(&packet(0x00, &string(&status.to_string())))?;
            // The status request, then the ping request (a long) to echo.
            stream.set_read_timeout(Some(Duration::from_secs(5)))?;
            for _ in 0..2 {
                let Ok(body) = read_frame(stream) else { break };
                if body.first() == Some(&0x01) {
                    stream.write_all(&packet(0x01, &body[1..]))?;
                    break;
                }
            }
        }
        Intent::Login => {
            let text = serde_json::json!({ "text": reason });
            stream.write_all(&packet(0x00, &string(&text.to_string())))?;
        }
        Intent::Other => {}
    }
    stream.flush()
}

/// One frame's body (packet id and fields), uncompressed as during status and
/// before login's compression.
pub fn read_frame(stream: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut length = 0usize;
    for i in 0..3 {
        let mut byte = [0u8; 1];
        stream.read_exact(&mut byte)?;
        length |= usize::from(byte[0] & 0x7F) << (7 * i);
        if byte[0] & 0x80 == 0 {
            break;
        }
    }
    if length > 1 << 16 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too long"));
    }
    let mut body = vec![0u8; length];
    stream.read_exact(&mut body)?;
    Ok(body)
}

pub fn read_varint(buf: &[u8], at: &mut usize) -> Option<i32> {
    let mut value: u32 = 0;
    for i in 0..5 {
        let byte = *buf.get(*at)?;
        *at += 1;
        value |= u32::from(byte & 0x7F) << (7 * i);
        if byte & 0x80 == 0 {
            return Some(value as i32);
        }
    }
    None
}

pub fn write_varint(out: &mut Vec<u8>, value: i32) {
    let mut v = value as u32;
    loop {
        if v & !0x7F == 0 {
            out.push(v as u8);
            return;
        }
        out.push((v & 0x7F) as u8 | 0x80);
        v >>= 7;
    }
}

/// A protocol string: its UTF-8 length, then the bytes.
pub fn string(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() + 3);
    write_varint(&mut out, s.len() as i32);
    out.extend_from_slice(s.as_bytes());
    out
}

/// A frame: length, packet id, fields.
pub fn packet(id: i32, fields: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(fields.len() + 1);
    write_varint(&mut body, id);
    body.extend_from_slice(fields);
    let mut out = Vec::with_capacity(body.len() + 3);
    write_varint(&mut out, body.len() as i32);
    out.extend_from_slice(&body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handshake(intent: i32) -> Vec<u8> {
        let mut fields = Vec::new();
        write_varint(&mut fields, PROTOCOL_VERSION);
        fields.extend(string("localhost"));
        fields.extend(25565u16.to_be_bytes());
        write_varint(&mut fields, intent);
        packet(0x00, &fields)
    }

    #[test]
    fn reads_what_the_client_asks_for() {
        for (intent, expected) in [
            (1, Intent::Status),
            (2, Intent::Login),
            (3, Intent::Login),
            (7, Intent::Other),
        ] {
            let sent = handshake(intent);
            let mut more = sent.clone();
            more.extend([0x01, 0x00]);
            let (raw, got) = read_handshake(&mut more.as_slice()).unwrap();
            assert_eq!(got, expected);
            assert_eq!(raw, sent, "exactly the handshake frame is read");
        }
    }

    #[test]
    fn legacy_ping_and_garbage_are_other() {
        let (raw, intent) = read_handshake(&mut [0xFE, 0x01].as_slice()).unwrap();
        assert_eq!((raw, intent), (vec![0xFE], Intent::Other));
        assert_eq!(parse_intent(&[0x05, 0x01]), Intent::Other);
        assert_eq!(parse_intent(&[0x00, 0x80]), Intent::Other);
    }

    #[test]
    fn varints_round_trip() {
        for v in [0, 1, 127, 128, 255, 25565, 777, i32::MAX, -1, i32::MIN] {
            let mut out = Vec::new();
            write_varint(&mut out, v);
            let mut at = 0;
            assert_eq!(read_varint(&out, &mut at), Some(v));
            assert_eq!(at, out.len());
        }
    }
}
