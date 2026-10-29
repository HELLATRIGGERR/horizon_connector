# horizon-connect: Play with Friends for vanilla Minecraft

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
Horizon's own client opens one per connection.

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
