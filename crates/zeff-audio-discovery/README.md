# zeff-audio-discovery

Portable, bounded game-audio inspection for Zeff-boy. The library takes immutable
bytes and returns serializable inventories; the app handles media, playback and
file exports.

- `scan`: cartridge audio structures.
- `drivers::scan`: driver fingerprints and table evidence.
- `scan_standalone_tracker`, `vgm::scan`, `rips::scan`: tracker, VGM/VGZ and GBS/NSF inspection.
- `native_rips::encode`: qualified original-driver exports.
- `ScanReport::song` and `asset_relations`: selected-item lookup and source relationships.

Discovery evidence does not guarantee a complete soundtrack or playable export.
Native playback and exports require their own source validation.

```sh
cargo test --locked -p zeff-audio-discovery --all-features
cargo check --locked -p zeff-audio-discovery --target wasm32-unknown-unknown
```

CLI options and export formats are listed by `zeff-boy --help`.
