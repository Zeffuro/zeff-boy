Native proof for two-controller NES lockstep (loopback by default):

```sh
cargo run -p zeff-netplay --release --bin zeff-netplay-proof -- --frames 300 --scenario all
```

Uses an embedded synthetic game, authenticated admission, two-frame input
delay, and a local reference. Reports JSON; save mutations are discarded.
Direct trusted-private TCP is opt-in. Rollback remains unqualified. The
portable scheduler builds without default features.

The native App exposes **Tools > NES Netplay**. Load an unmodified NTSC, PAL or Dendy `.nes`,
select 48 kHz audio and use identical executable bytes and game content on both
players' Apps. **Same computer** is the default host/join scope; host and copy
the invitation into a second App on that computer. For separate computers,
both players select **Private LAN / network**. The host enters its own numeric
private IP and an editable TCP port (provisional default 8766), then shares the
secret invitation only with the other player. Same-computer hosting uses an
automatically assigned loopback port. The host address and TCP port must be
reachable from the other computer; check host firewall rules. TCP authenticates
peers but does not encrypt traffic, so use a trusted private network.

Both players use their local Player 1 controls. Sessions start
from a fresh cartridge load; disconnect discards session saves and leaves the
restored cartridge paused. Unfocused input is neutral. Either player can pause;
both must release their pause request to resume. Disconnect to change the game.

The native App worker has a separate headless check (PowerShell, fresh output):

```powershell
$env:ZEFF_MUTE_AUDIO = "1"
$env:ZEFF_CONFIG_DIR = "$PWD/.tmp/netplay-worker/config"
cargo run -p zeff-boy --bin zeff-boy -- --netplay-worker-proof .tmp/netplay-worker 300
```

This checks loader admission, delayed input, frame agreement, cancellation and
save restoration in the emulation worker.

Append `--timing pal` or `--timing dendy` to check a regional synthetic fixture. Append
`--rom PATH_TO_CARTRIDGE.nes` to check a direct NTSC, PAL or Dendy cartridge (up to
64 MiB), including coordinated pause and resume. The proof uses isolated media
copies with save writes disabled. Use a fresh output directory for each run.

For a headless trusted-network experiment, set `ZEFF_NETPLAY_PROOF_SECRET` to
the same random 64-digit hexadecimal capability on both machines. Start the
proof binary with `--listen PRIVATE_IP:PORT --frames 300`, then run the other
with `--network-peer PRIVATE_IP:PORT --frames 300`. Bind a specific numeric
address. This TCP experiment authenticates peers but does not encrypt traffic.

The default identity requires identical executable bytes. `--test-build HEX`
is a proof-only synthetic identity override for experiments with independently
verified source pins; reports retain the actual executable digest. It does not
enable cross-build App sessions. Production cross-OS App sessions remain
unsupported; the App requires identical executable bytes.

Different-version consent is per session and requires both players' opt-in.
The control remains unavailable until the local build has a qualified
core/session contract; consent cannot bypass game or contract mismatches.
