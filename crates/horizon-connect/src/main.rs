//! `horizon-connect join CODE`: a vanilla 26.3 client joins a Horizon
//! friend's world at localhost. `horizon-connect host`: Horizon players join
//! a vanilla server or LAN world from an invite code. docs/horizon-connect.md.
use horizon_connect::{JoinConfig, serve_join};
use horizon_core::version::{PROTOCOL_VERSION, VANILLA_VERSION};
use horizon_friends::{
    Host, HostConfig, HostEvent, PathMode,
    host::DEFAULT_PORT,
    invite::{self, Parsed},
    reach, stun,
};
use std::{
    io::IsTerminal,
    net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs},
    process::exit,
    sync::mpsc::channel,
    time::Duration,
};

const USAGE: &str = "\
horizon-connect: Play with Friends between vanilla Minecraft 26.3 and Horizon

  horizon-connect join CODE [--port 25565] [--relay HOST:PORT]
      Join a Horizon friend's world from vanilla Minecraft: then connect to
      localhost (Multiplayer > Direct Connection).

  horizon-connect host [--server 127.0.0.1:25565] [--port 25580] [--relay HOST:PORT]
      Share a vanilla server or a world opened to LAN: Horizon players paste
      the code into Join a friend.

The HORIZON_FRIENDS_* and HORIZON_STUN_SERVERS variables work as in the game
(docs/play-with-friends.md).";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("join") => join(&args[1..]),
        Some("host") => host(&args[1..]),
        _ => {
            println!("{USAGE}");
            exit(if args.is_empty() { 0 } else { 2 });
        }
    }
}

/// `--name value` or `--name=value`; the other arguments in order.
fn split_options(args: &[String], names: &[&str]) -> (Vec<(String, String)>, Vec<String>) {
    let mut options = Vec::new();
    let mut rest = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let Some(name) = names
            .iter()
            .find(|n| arg == *n || arg.starts_with(&format!("{n}=")))
        else {
            rest.push(arg.clone());
            continue;
        };
        let value = match arg.split_once('=') {
            Some((_, v)) => v.to_owned(),
            None => it
                .next()
                .cloned()
                .unwrap_or_else(|| fail(&format!("{name} needs a value"))),
        };
        options.push(((*name).to_owned(), value));
    }
    (options, rest)
}

fn option<'a>(options: &'a [(String, String)], name: &str) -> Option<&'a str> {
    options
        .iter()
        .rev()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
}

fn fail(message: &str) -> ! {
    eprintln!("horizon-connect: {message}");
    exit(1);
}

fn log(line: &str) {
    println!("{line}");
}

fn env_flag(name: &str, default: bool) -> bool {
    std::env::var(name).map_or(default, |v| v != "0")
}

fn join(args: &[String]) {
    let (options, rest) = split_options(args, &["--port", "--relay"]);
    // Codes are read ignoring spaces, so a code pasted as several words works.
    let code = rest.join(" ");
    if code.trim().is_empty() {
        fail("join needs your friend's invite code: horizon-connect join CODE");
    }
    let relay = option(&options, "--relay")
        .map(str::to_owned)
        .or_else(horizon_friends::relay_url);
    match invite::parse_detailed(&code) {
        Err(e) => fail(&e.to_string()),
        Ok(Parsed::Short(_)) if relay.is_none() => fail(
            "this short code works through your friend's relay: add --relay HOST:PORT, or ask for the full code",
        ),
        Ok(_) => {}
    }
    let port: u16 = option(&options, "--port")
        .map(|p| p.parse().unwrap_or_else(|_| fail("--port takes a number")))
        .unwrap_or(25565);
    let listener = TcpListener::bind(("127.0.0.1", port)).unwrap_or_else(|e| {
        fail(&format!(
            "can't listen on 127.0.0.1:{port} ({e}); pick another with --port"
        ))
    });
    let address = if port == 25565 {
        "localhost".to_owned()
    } else {
        format!("localhost:{port}")
    };
    println!(
        "Your friend's world is at {address}.\n\
         In Minecraft {VANILLA_VERSION}: Multiplayer > Direct Connection > {address}. Ctrl-C stops."
    );
    let config = JoinConfig {
        code,
        relay,
        path: PathMode::from_env(),
    };
    if let Err(e) = serve_join(listener, config, log) {
        fail(&e.to_string());
    }
}

fn host(args: &[String]) {
    let (options, rest) = split_options(args, &["--server", "--port", "--relay"]);
    if let Some(extra) = rest.first() {
        fail(&format!("unexpected argument {extra}"));
    }
    let server = option(&options, "--server").unwrap_or("127.0.0.1:25565");
    let lan: SocketAddr = server
        .to_socket_addrs()
        .ok()
        .and_then(|mut a| a.next())
        .unwrap_or_else(|| fail(&format!("can't resolve {server}")));
    if TcpStream::connect_timeout(&lan, Duration::from_secs(3)).is_err() {
        fail(&format!(
            "nothing answers at {lan}: start the server, or open the world to LAN and pass --server 127.0.0.1:PORT with the port the game shows"
        ));
    }
    let port = option(&options, "--port")
        .map(|p| p.parse().unwrap_or_else(|_| fail("--port takes a number")))
        .or_else(|| {
            std::env::var("HORIZON_FRIENDS_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
        })
        .unwrap_or(DEFAULT_PORT);
    let relay = option(&options, "--relay")
        .map(str::to_owned)
        .or_else(horizon_friends::relay_url);
    let mut host = Host::start(HostConfig {
        lan,
        protocol: PROTOCOL_VERSION,
        game: VANILLA_VERSION.to_owned(),
        relay: relay.clone(),
        path: PathMode::from_env(),
        port,
        reach: reach::Options {
            map_port: env_flag("HORIZON_FRIENDS_UPNP", true),
            stun: stun::servers_from_env(),
            loopback: env_flag("HORIZON_FRIENDS_LOOPBACK", false),
        },
    })
    .unwrap_or_else(|e| fail(&format!("can't start sharing: {e}")));
    println!("Sharing {lan}. Finding this computer's addresses...");

    // Enter stops sharing cleanly (the router mapping is deleted); without a
    // terminal it runs until killed.
    let (stop_tx, stop_rx) = channel::<()>();
    let interactive = std::io::stdin().is_terminal();
    if interactive {
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
            let _ = stop_tx.send(());
        });
    }
    let mut summary = String::new();
    let mut relay_problem = None;
    loop {
        for event in host.poll() {
            match event {
                HostEvent::Code(code) => {
                    println!(
                        "\nInvite code (Horizon: Join a friend, or Direct Connection):\n  {code}"
                    );
                    if let (Some(short), Some(r)) = (host.state().short, relay.as_deref()) {
                        println!("Short code, through the relay {r}:\n  {short}");
                    }
                    if interactive {
                        println!("Press Enter to stop sharing.");
                    }
                    println!();
                }
                HostEvent::Connected { session, direct } => println!(
                    "A Horizon player is joining (session {session}, {}).",
                    if direct {
                        "direct"
                    } else {
                        "through the relay"
                    }
                ),
                HostEvent::Disconnected { session } => println!("Session {session} ended."),
                HostEvent::WrongCode { count } => {
                    println!("Someone tried a wrong code ({count} so far).")
                }
            }
        }
        let state = host.state();
        if !state.summary.is_empty() && state.summary != summary {
            println!("{}", state.summary);
            summary = state.summary;
        }
        if state.relay_problem.is_some() && state.relay_problem != relay_problem {
            println!(
                "Relay: {}",
                state.relay_problem.as_deref().unwrap_or_default()
            );
            relay_problem = state.relay_problem;
        }
        if stop_rx.try_recv().is_ok() {
            host.stop();
            println!("Stopped sharing: the code has ended.");
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}
