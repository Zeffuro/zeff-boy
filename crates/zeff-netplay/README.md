# Netplay

Open **Tools > Netplay**. NES uses a shared console with rollback.
Game Boy and WonderSwan use separate devices connected by a TCP link.

For NES, load the same unmodified `.nes` cartridge on both devices.
Use matching builds or a qualified pair and 48 kHz audio.
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
