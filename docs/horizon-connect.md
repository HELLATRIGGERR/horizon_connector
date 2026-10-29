# horizon-connect: Play with Friends for vanilla Minecraft

*English · [Русский](horizon-connect.ru.md)*

[Play with Friends](play-with-friends.md) carries vanilla's protocol unchanged through an encrypted tunnel
between two loopback ports, but only Horizon's game knows how to open that tunnel. `horizon-connect` is a
small program that opens it for vanilla Minecraft 26.3, in both directions:

- **`join`**: a player on vanilla Minecraft joins a Horizon friend's world from the friend's invite code.
- **`host`**: Horizon players join a vanilla server, or a vanilla world opened to LAN, from an invite code.

Neither game changes. The program is the tunnel's other end, built on the same crate as the game's
(`horizon-friends`), so codes, encryption, port mapping, STUN and relays behave exactly as described in
[play-with-friends.md](play-with-friends.md).

```
 join:  vanilla client ──▶ 127.0.0.1:25565 horizon-connect ══ tunnel ══▶ host's Horizon ──▶ integrated server
 host:  Horizon client ──▶ Join a friend  ══ tunnel ══▶ horizon-connect ──▶ vanilla server or LAN world
```

## Join a Horizon friend from vanilla Minecraft

The friend opens Play with Friends in Horizon (pause menu, Friends, Start Sharing) and sends the code. Then:

```sh
horizon-connect join 2A7KQ-M3XPD-...
```

and in Minecraft 26.3: **Multiplayer > Direct Connection > `localhost`**. The world shows in the server list
like any server. Each connection, the server list's ping as much as the join, gets its own session, as
Horizon's own client opens one per connection. Keep the program running while you play.

- `--port N` listens on `localhost:N` instead of 25565 (for example when a server already runs there).
- `--relay HOST:PORT` is needed for a relay's short code; a full code names its own relay.
- Only connections from this computer are accepted: whoever reaches the port plays under the code.
- When the friend's world can't be reached, the reason (the same text Horizon shows) appears as the server's
  description in the server list, or on the disconnect screen when joining.

The integrated server is in offline mode for friends ([Identity](play-with-friends.md#identity)), so the
vanilla player joins under the name their launcher uses, without an account check.

## Share a vanilla world with Horizon players

Start a vanilla 26.3 server, or open a single-player world to LAN (vanilla prints the port in chat), then:

```sh
horizon-connect host --server 127.0.0.1:25565
```

It prints the invite code; Horizon players paste it into **Join a friend** (or Direct Connection). Enter stops
sharing: the code ends, the direct port closes and the router mapping is deleted.

- `--port N` is the direct port (25580 by default, `HORIZON_FRIENDS_PORT` too).
- `--relay HOST:PORT` registers with a relay as well, for a short code and a fallback.
- `HORIZON_FRIENDS_UPNP=0` leaves the router alone, `HORIZON_STUN_SERVERS=none` skips STUN,
  `HORIZON_FRIENDS_PATH=relay|direct` restricts the path, as in the game.
- Vanilla's LAN worlds check accounts like an online-mode server, so a Horizon player needs to be signed in
  ([microsoft-account.md](microsoft-account.md)). A dedicated vanilla server with `online-mode=false` takes
  any name.
- Friends on the internet reach the host as described in
  [When friends can't reach the host](play-with-friends.md#when-friends-cant-reach-the-host): UPnP, a forwarded
  port, a VPN such as Tailscale (start it before `host`, so the code holds its address) or a relay.

## Building

```sh
cargo build --release -p horizon-connect
```

The program is `target/release/horizon-connect` (`.exe` on Windows), about 1 MB, with no other files.

## Latency

The tunnel adds well under a millisecond. Round trips on one computer (median of 300, an echo server behind
`host`, the tunnel, then `join`):

| Message | Direct | Through the tunnel |
| --- | ---: | ---: |
| 64 bytes (movement, chat) | 0.03 ms | 0.07 ms |
| 4 KiB (block updates) | 0.03 ms | 0.09 ms |
| 64 KiB (a chunk) | 0.05 ms | 0.31 ms |

Opening a session (dialling the code's endpoints and the key exchange) took 5-14 ms on loopback; over the
internet it takes a few round trips between the players more. It happens once per connection: when joining
and for each server list ping, not during play. Between two computers the game's latency is the network's, as
with any direct connection; through a relay it is the way to the relay and back.

## Versions

Both sides must play 26.3: the tunnel carries the protocol unchanged and doesn't translate it, so a client of
another version gets the server's own incompatible-version message at the handshake.

## Testing

- `crates/horizon-connect/tests/join.rs`: a host in front of an echo server, `join`'s port in front of its
  code, and a client speaking the handshake, all on loopback. A join arrives byte for byte (more than one tunnel
  frame of game bytes after the handshake), server list pings each get a session, and an ended code shows its
  reason in the server list (with the ping answered) and on the login disconnect.
- By hand, both ends at once on one computer: a vanilla server, `HORIZON_FRIENDS_UPNP=0
  HORIZON_STUN_SERVERS=none HORIZON_FRIENDS_LOOPBACK=1 horizon-connect host`, `horizon-connect join CODE
  --port 25570`, and a client on `localhost:25570`: the server list ping and the join each get a direct
  session; login, chunks and chat both ways.
- Against Horizon itself (nightly 2026-10-28): a world shared with Start Sharing, `horizon-connect join` with
  its code, and a client in front of it. The world showed in the server list, the player joined, its chat
  showed in Horizon, and it played 40 seconds (keep-alives answered, about 14,600 packets) before leaving.
