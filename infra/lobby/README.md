# Netplay lobby

Two-player signaling only. Gameplay goes directly between peers.
Open **Netplay** on the quickbar and choose **Lobby**. Native and browser gameplay are supported.

Deploy `Zeffuro/zeff-boy` branch `master` in Coolify using Docker Compose
and `/infra/lobby/compose.yml`. Route your HTTPS domain to port `8080`.
Health is `/health` and signaling is `/v1/ws`.

- Private default: set `ZEFF_LOBBY_ACCESS_TOKEN` to a random 32..256 byte token.
  Missing or empty tokens fail server startup.
- Public: set `ZEFF_LOBBY_PUBLIC=true` and omit the token.
- Browser origins: `ZEFF_LOBBY_ORIGINS`, default `https://zeffuro.github.io`.
- Capacity: `ZEFF_LOBBY_MAX_ROOMS` defaults to 4 (1..128),
  `ZEFF_LOBBY_MAX_CONNECTIONS` to 16 (2..256), `ZEFF_LOBBY_ROOM_TTL` to 120s (10..600).

Limits cover signaling occupancy. Completed games release their rooms.
Compose uses 0.5 CPU and 256 MiB. STUN defaults to Cloudflare and TURN stays off.
Private servers can opt into external TURN with `ZEFF_LOBBY_ALLOW_TURN=true`,
`ZEFF_LOBBY_TURN_URLS` and `ZEFF_LOBBY_TURN_SECRET`. Public TURN is rejected.

Compose pins `docker.io/zeffuro/zeff-boy-lobby:0.1.1`.
Set `LOBBY_VERSION` after publishing a new lobby release.
Actions requires `DOCKERHUB_USERNAME` and `DOCKERHUB_TOKEN` with image push access.
Release with `lobby-v<crate-version>`. Emulator tags use `v*`.
