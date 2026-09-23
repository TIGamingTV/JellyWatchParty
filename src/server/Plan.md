# Plan: Fix RUSTSEC-2026-0258 at the Root (Migrate off `warp`)

> **Status: implemented.** `warp` and `h2` are both gone from the dependency
> tree, `src/server/.cargo/audit.toml` has been deleted, and `cargo audit`
> passes clean with no ignore list. See "Deviations & corrections" at the
> bottom for where reality differed from the original plan.

## Background

PR #64 (`fix/cargo-audit-h2-rustsec-2026-0258`) suppressed `RUSTSEC-2026-0258`
(h2 unbounded empty DATA frames DoS) via `src/server/.cargo/audit.toml`. That
is a **workaround**, not a fix: the vulnerable code (`h2 v0.3.27`) is still
compiled into the binary and still runs in production. It was pulled in
transitively via:

```
warp 0.3.7 -> hyper 0.14.32 -> h2 0.3.27
```

The advisory's only remediation is `h2 >=0.4.16`, which only exists on the
`hyper 1.x` line. `warp 0.3` has never shipped a `hyper 1`-compatible
release and shows no sign of doing so (project has had long maintenance
gaps). **There is no way to fix this by bumping a version number** — the
only real fix is to stop depending on `warp`/`hyper 0.14`.

## Goal

Replace `warp` with `axum` (built directly on `hyper 1.x`, actively
maintained by the Tokio team), eliminating the vulnerable dependency entirely
rather than muting the scanner.

Why `axum` specifically:
- Built on `hyper 1.x`.
- Actively maintained, same ecosystem (tokio, tower) already in use.
- Websocket support (`axum::extract::ws`) is a near-drop-in replacement for
  `warp::ws`.
- Avoids introducing a second unrelated framework migration risk (e.g.
  actix-web would be a bigger behavioral/ecosystem change).

## Scope of `warp` usage

`warp` was referenced in **10** files (~39 call sites), all fairly mechanical:

| File | Usage |
|---|---|
| `src/main.rs` | `warp::serve(...).bind_with_graceful_shutdown(...)` — server bootstrap |
| `src/routes.rs` | Route filters: CORS, origin-header check + custom rejection, query-param extraction, `GET /ws` upgrade, `GET /health` |
| `src/types.rs` | `Client.sender: mpsc::Sender<Result<warp::ws::Message, warp::Error>>` |
| `src/ws/connection.rs` | Takes/holds `warp::ws::WebSocket`, forwards `warp::ws::Message` |
| `src/ws/dispatch.rs` | Dispatches on `warp::ws::Message` (text/close/size checks) |
| `src/messaging.rs` | Constructs `warp::ws::Message::text(json)` for broadcast/unicast sends |
| `src/room/leave.rs` | Constructs `warp::ws::Message::text(...)` for leave notifications |
| `src/ws/handlers/chat.rs` | `BroadcastData` typed on `mpsc::Sender<Result<warp::ws::Message, warp::Error>>`; chat fan-out |
| `src/ws/handlers/playback.rs` | Player-event / state-update fan-out |
| `src/test_helpers.rs` | Test doubles typed on `warp::ws::Message`/`warp::Error` |

No middleware/extractor was exotic (CORS, origin-header check, query params,
websocket upgrade, static health route) — all have direct `axum`/`tower-http`
equivalents.

## Step-by-step plan

### 1. Dependencies
- [x] Swap `warp` for `axum` + `tower-http` in a single atomic change — the
      surface area is small enough (10 files) that a dual-dependency window
      isn't worth the complexity.
- [x] `axum = { version = "0.8", default-features = false, features = [...] }`
      and `tower-http = { version = "0.6", features = ["cors"] }`.
      **The `http2` feature is deliberately left off** (see corrections).
- [x] Add `tokio`'s `net` feature (needed by `axum::serve`'s `TcpListener`).

### 2. Abstract the WebSocket message type (`src/types.rs`)
- [x] Introduced `OutboundMessage`, a newtype over the serialized JSON
      payload, plus `ClientSender` / `ClientReceiver` aliases, so
      `Client.sender` no longer leaks the framework's message type into
      `messaging.rs`, `room/leave.rs`, `ws/handlers/*` or `test_helpers.rs`.
- [x] Updated every construction site to build `OutboundMessage`.

### 3. Replace the router (`src/routes.rs`, `src/main.rs`)
- [x] CORS on `/health` via `tower_http::cors::CorsLayer` (same allowed
      origins / methods / headers, including the `*` wildcard branch).
- [x] The `origin` allow-list check is now `axum::middleware::from_fn_with_state`
      applied as a `route_layer` on `/ws`, replacing `OriginRejected` +
      `warp::reject::custom`. A **missing** `Origin` still passes (see
      corrections) and a disallowed one now returns a real `403`.
- [x] `GET /ws` is an `axum::extract::ws::WebSocketUpgrade` handler pulling
      `client_id` from `Query<HashMap<String, String>>`.
- [x] `GET /health` returns `axum::Json(...)`.
- [x] `main.rs` uses `tokio::net::TcpListener` + `axum::serve(...)
      .with_graceful_shutdown(...)`, driven by the same `tasks::setup_shutdown_signal()`
      oneshot as before.

### 4. Migrate the WebSocket connection handling
- [x] `warp::ws::WebSocket` → `axum::extract::ws::WebSocket`; the
      split-sink/stream pattern is unchanged. The channel now carries
      `OutboundMessage`, mapped to `Message::Text` in the single forwarding
      task in `ws/connection.rs`.
- [x] `dispatch.rs` matches on `Message::Text(..)` instead of using
      `is_text()`/`to_str()`, with a `payload_len()` helper replacing
      `msg.as_bytes().len()` for the 64 KB check.

### 5. Update `messaging.rs`, `room/leave.rs`, `ws/handlers/{chat,playback}.rs`
- [x] All now build `OutboundMessage::text(json)`; none reference a web
      framework type.

### 6. Update tests
- [x] `test_helpers.rs` retyped onto `ClientReceiver`; `recv_msg` no longer
      unwraps a `Result`.
- [x] The 102 pre-existing tests pass unchanged.

### 7. Remove `warp` and the audit ignore
- [x] `warp` deleted from `Cargo.toml`; `Cargo.lock` regenerated.
- [x] `cargo tree -i h2` → *"package ID specification `h2` did not match any
      packages"*. `h2` is not in the tree at any version; `hyper` is 1.11.1.
- [x] `src/server/.cargo/audit.toml` deleted outright.
- [x] `cargo audit` passes clean with **no ignore list and no warnings** —
      the `rand` and `spin` advisories listed as non-goals below also came in
      via `warp` and are gone too.

### 8. Validate
- [x] `cargo test` — 109 passing (102 pre-existing + 7 new route tests).
- [x] `cargo clippy --all-targets -- -D warnings` — clean.
- [x] `cargo fmt --check` — clean.
- [x] Builds on Rust 1.88 (`infra/docker/server.Dockerfile`'s toolchain), so
      no MSRV bump is needed.
- [x] End-to-end smoke test against the release binary: `/health`
      (status/shape/content-type/CORS header), websocket handshake with an
      allowed origin, with **no** origin, and with a disallowed origin (403);
      `create_room` / `join_room` / `participants_update` / chat (incl.
      multi-byte UTF-8) / `player_event` / `state_update` / `ready` /
      application `ping`→`pong` / transport-level ping / reconnect-reattach /
      `host_changed` on host leave / room teardown.
- [x] Oversize (>64 KB) and malformed messages still produce an **in-band
      `error`** and leave the socket open.
- [x] A 40 KB `room_state` is delivered as a single unfragmented text frame
      (the plugin bridges decode each received chunk independently, so
      fragmentation would corrupt multi-byte characters).
- [x] Graceful shutdown: SIGTERM with an open websocket exits in <0.01s,
      code 0 — no regression vs `bind_with_graceful_shutdown`.

### 9. Ship
- [ ] Open PR `refactor: migrate session-server from warp to axum, resolving
      RUSTSEC-2026-0258 at the source` targeting `main`, noting it supersedes
      and removes the ignore added in #64.
- [ ] After merge, confirm the `Security` workflow's Cargo Audit job is green
      with **no** ignore list.

## Deviations & corrections to the original plan

1. **File count was wrong.** The plan listed 8 files; `ws/handlers/chat.rs`
   and `ws/handlers/playback.rs` also referenced `warp::ws::Message` and were
   missed.

2. **"Reject with 403 … the same status code the frontend/plugin expects"
   was a false premise.** `OriginRejected` was a `warp::reject::custom` with
   no `recover()` handler registered, so warp turned it into a **500
   "Unhandled rejection"**, not a 403. Nothing on either client reads the
   handshake status (browsers don't expose it; the C# bridge only sees a
   generic `WebSocketException`), so returning a proper 403 is a free
   improvement.

3. **A missing `Origin` header must keep being allowed.** This wasn't called
   out in the plan and is the single most dangerous thing to get wrong: the
   plugin's `SessionHostBridge` / `SessionFollowerBridge` use .NET's
   `ClientWebSocket`, which sends no `Origin` at all. They work only because
   of the old `None => Ok(())` branch. There is now a dedicated regression
   test for it.

4. **Control frames must keep refreshing `last_seen`.** `check_rate_limit`
   is what updates `last_seen` for the 60s zombie reaper, and it runs for
   *every* frame. Returning early on non-`Text` frames before that call
   would let a control-frame-only client get reaped. `client_msg` keeps the
   original ordering, with a comment explaining why.

5. **Dependency versions in the plan were stale.** `axum 0.7` / `tower 0.4` /
   `tower-http 0.5` → `axum 0.8` / `tower-http 0.6` (`tower` isn't needed as a
   direct dependency at all outside dev).

6. **`h2` is gone entirely, not merely patched.** The plan assumed axum would
   pull `h2 0.4.x`. `http2` is not an axum 0.8 default feature, and nothing
   that talks to this server speaks HTTP/2 — browsers use an HTTP/1.1
   `Upgrade` for WebSockets, .NET's `ClientWebSocket` defaults to HTTP/1.1,
   and the documented reverse-proxy configs pin `proxy_http_version 1.1`. So
   the feature is left off and `h2` never enters the tree. *This is the one
   intentional behavioural narrowing: an h2c prior-knowledge client would no
   longer be served. Add `features = ["http2"]` to restore parity if that
   ever matters.*

7. **`/ws` path matching is now exact.** `warp::path("ws")` without
   `path::end()` also matched `/ws/anything`; `Router::route("/ws", ...)`
   does not. No client or documented proxy config relies on trailing
   segments.

8. **Tests were added, not just ported.** The repo had zero coverage of the
   HTTP/websocket contract. `routes.rs` now has 7 tests, 5 of which bind a
   real ephemeral listener and perform an actual upgrade (`ServiceExt::oneshot`
   can't — with no `OnUpgrade` extension, `WebSocketUpgrade` always rejects
   with 426).

9. **Docs updated.** `docs/ARCHITECTURE.md`, `docs/technical/server.md` and
   `docs/core-structure.md` referenced warp. `docs/PROGRESS.md` was left
   alone: it is a historical changelog and was accurate when written.

## Non-goals

- Not migrating other parts of the stack (this plan is scoped to
  `src/server` only).
- Not changing the wire protocol / `WsMessage` JSON schema — this is purely
  a transport-layer/framework swap.
- Not fixing the pre-existing latent UTF-8 bug in the C# bridges, which
  decode each received websocket chunk independently
  (`Encoding.UTF8.GetString(buffer, 0, result.Count)` in
  `SessionHostBridge.cs` / `SessionFollowerBridge.cs`) and would corrupt a
  message split mid-character. Unchanged by this migration — the server
  still sends single unfragmented text frames — but worth a follow-up.
