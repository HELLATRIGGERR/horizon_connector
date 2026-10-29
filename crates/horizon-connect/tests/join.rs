//! A host in front of an echo server standing in for the friend's
//! integrated server, horizon-connect's port in front of its code, and a
//! "vanilla client" that speaks the handshake, all on loopback. Port mapping
//! and STUN are off, as in horizon-relay's tunnel tests.
use horizon_connect::{
    JoinConfig, packet, read_frame, read_varint, serve_join, string, write_varint,
};
use horizon_friends::{Host, HostConfig, PathMode, reach};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    time::{Duration, Instant},
};

/// Echoes every connection back until it closes.
fn echo_server() -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let a = l.local_addr().unwrap();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            std::thread::spawn(move || {
                let mut out = s.try_clone().unwrap();
                let _ = std::io::copy(&mut s, &mut out);
            });
        }
    });
    a
}

fn share(lan: SocketAddr) -> Host {
    Host::start(HostConfig {
        lan,
        protocol: 777,
        game: "26.3".into(),
        relay: None,
        path: PathMode::DirectOnly,
        port: 0,
        reach: reach::Options {
            map_port: false,
            stun: Vec::new(),
            loopback: true,
        },
    })
    .unwrap()
}

fn invite(host: &Host) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(code) = host.state().invite {
            return code;
        }
        assert!(Instant::now() < deadline, "no code");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// horizon-connect's port for `code`.
fn connect_port(code: String) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let config = JoinConfig {
        code,
        relay: None,
        path: PathMode::DirectOnly,
    };
    std::thread::spawn(move || serve_join(listener, config, |_| {}));
    port
}

fn handshake(intent: i32) -> Vec<u8> {
    let mut fields = Vec::new();
    write_varint(&mut fields, 777);
    fields.extend(string("localhost"));
    fields.extend(25565u16.to_be_bytes());
    write_varint(&mut fields, intent);
    packet(0x00, &fields)
}

fn client(port: u16) -> TcpStream {
    let s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    s
}

#[test]
fn a_join_reaches_the_friends_world_byte_for_byte() {
    let host = share(echo_server());
    let port = connect_port(invite(&host));
    let mut s = client(port);
    // The handshake, a login start, then more than one tunnel frame of game bytes.
    let mut sent = handshake(2);
    sent.extend(packet(0x00, &string("Alex")));
    sent.extend((0..200_000u32).map(|i| (i * 31 % 251) as u8));
    let mut w = s.try_clone().unwrap();
    let payload = sent.clone();
    let writer = std::thread::spawn(move || w.write_all(&payload).unwrap());
    let mut got = vec![0u8; sent.len()];
    s.read_exact(&mut got).unwrap();
    writer.join().unwrap();
    assert!(got == sent, "the friend's server got other bytes");
}

#[test]
fn server_list_pings_each_get_a_session() {
    let host = share(echo_server());
    let port = connect_port(invite(&host));
    for _ in 0..3 {
        let mut s = client(port);
        let mut sent = handshake(1);
        sent.extend(packet(0x00, &[]));
        s.write_all(&sent).unwrap();
        let mut got = vec![0u8; sent.len()];
        s.read_exact(&mut got).unwrap();
        assert_eq!(got, sent);
    }
}

#[test]
fn an_ended_invite_shows_why_in_the_server_list() {
    let mut host = share(echo_server());
    let code = invite(&host);
    host.stop();
    let port = connect_port(code);
    let mut s = client(port);
    let mut sent = handshake(1);
    sent.extend(packet(0x00, &[]));
    s.write_all(&sent).unwrap();

    let body = read_frame(&mut s).unwrap();
    let mut at = 0;
    assert_eq!(read_varint(&body, &mut at), Some(0x00), "status response");
    let len = read_varint(&body, &mut at).unwrap() as usize;
    let status: serde_json::Value = serde_json::from_slice(&body[at..at + len]).unwrap();
    assert_eq!(status["version"]["protocol"], 777);
    let reason = status["description"]["text"].as_str().unwrap();
    assert!(reason.contains("Couldn't reach"), "{reason}");

    // The ping is answered too, so the list shows the reason instead of an error.
    s.write_all(&packet(0x01, &42i64.to_be_bytes())).unwrap();
    assert_eq!(
        read_frame(&mut s).unwrap(),
        [&[0x01][..], &42i64.to_be_bytes()].concat()
    );
}

#[test]
fn an_ended_invite_disconnects_a_join_with_the_reason() {
    let mut host = share(echo_server());
    let code = invite(&host);
    host.stop();
    let port = connect_port(code);
    let mut s = client(port);
    let mut sent = handshake(2);
    sent.extend(packet(0x00, &string("Alex")));
    s.write_all(&sent).unwrap();
    let body = read_frame(&mut s).unwrap();
    let mut at = 0;
    assert_eq!(read_varint(&body, &mut at), Some(0x00), "login disconnect");
    let len = read_varint(&body, &mut at).unwrap() as usize;
    let text: serde_json::Value = serde_json::from_slice(&body[at..at + len]).unwrap();
    assert!(text["text"].as_str().unwrap().contains("Couldn't reach"));
}
