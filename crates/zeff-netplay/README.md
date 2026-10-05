# Netplay

Open **Netplay** on the quickbar. NES, Master System, SG-1000 and PC Engine use a shared console.
WonderSwan uses two linked machines with rollback. Game Boy has a direct TCP cable connection.

Load the same unmodified `.nes`, `.sms`, `.sg`, `.pce`, `.ws` or `.wsc` ROM, directly or from a ZIP.
Use matching builds and 48 kHz audio. NES also accepts qualified pairs.
Host, copy the invitation, then join from the other device.
Both players use their local Player 1 controls.
Sessions start fresh and discard their saves when disconnected.

Same PC and LAN use authenticated TCP. Lobby uses encrypted peer connections.
Choose a lobby address and enter a key only for private servers.
The [lobby deployment guide](../../infra/lobby/README.md) covers hosting.

Run the synthetic native proof:

```sh
cargo run -p zeff-netplay --release --bin zeff-netplay-proof -- --frames 300 --scenario all
```
