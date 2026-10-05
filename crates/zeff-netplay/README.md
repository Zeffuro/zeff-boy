# Netplay

Open **Netplay** on the quickbar. NES, Master System and SG-1000 use a shared console with rollback.
Game Boy and WonderSwan use separate devices connected by a TCP link.

Load the same unmodified `.nes`, `.sms` or `.sg` cartridge on both devices.
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
