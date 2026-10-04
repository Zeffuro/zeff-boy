# Netplay lobby

Private two-player signaling service. Gameplay goes directly between peers;
this container never relays it. The new transport is not yet connected to the
emulator window or deployed web app.

## Deploy

Paste `compose.yml` into Coolify's Docker Compose resource. Set:

- Domain: `https://lobby.fakegaming.eu`; container port: `8080`.
- `ZEFF_LOBBY_ACCESS_TOKEN`: random 32..256 bytes, shared privately with testers.
- `ZEFF_LOBBY_ORIGINS`: comma-separated browser origins; default
  `https://zeffuro.github.io`. Native clients don't need an Origin header.

The image is `docker.io/zeffuro/zeff-boy-lobby:0.1.0`. Compose limits it to
0.5 CPU, 256 MiB, four rooms and 16 connections. No volume or host port is needed.
Health: `/health`; signaling: `/v1/ws`. Cloudflare's proxied CNAME supports WSS;
Coolify must route the hostname and provide a valid origin certificate.

Free STUN defaults to `stun:stun.cloudflare.com:3478`. Restrictive NATs may fail
without TURN. TURN is disabled. To use a separate coturn service, explicitly set
`ZEFF_LOBBY_ALLOW_TURN=true`, `ZEFF_LOBBY_TURN_URLS` and
`ZEFF_LOBBY_TURN_SECRET`. Credentials expire after one hour; renewal is pending.

## Release and test

Add Actions secrets `DOCKERHUB_USERNAME` and `DOCKERHUB_TOKEN` with push access
to `zeffuro/zeff-boy-lobby`. Tag the crate version as `lobby-v0.1.0` to test,
build, smoke-test and publish it. Emulator releases use separate `v*` tags.

Local build: `docker build -f Dockerfile.lobby -t zeff-boy-lobby:local .`.
Native proof: set `ZEFF_LOBBY_TOKEN`, then run
`cargo run -p zeff-netplay-connect --features native --example native-proof --
create wss://lobby.fakegaming.eu/v1/ws`. The other peer uses `join URL ROOM`.
`probe.html` and `probe.mjs` test the Rust WASM adapter with generated
`wasm-bindgen --target web` assets in `wasm/`.
