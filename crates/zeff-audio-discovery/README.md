# zeff-audio-discovery

Portable, bounded game-audio inspection used by Zeff-boy and its fuzz targets.
The library consumes immutable byte slices and produces serializable inventories.
It has no playback device, concrete console core, media loader, or filesystem
publication dependency.

The app owns loaded-media provenance, archives and disc access, playback,
rendering, codecs, and atomic exports. The CDDA inventory type lets the native
disc adapter use the same catalog without importing a console core here.

## Entry points

- `scan`: supported cartridge structures, selected by `zeff_emu_common::system::System`.
- `scan_standalone_tracker`: XM, MOD, S3M and IT inspection.
- `vgm::scan`: standalone VGM and strict single-member VGZ inspection.
- `rips::scan`: standalone GBS and NSF inspection.
- `native_rips::supported_format` / `encode`: qualified original-driver music
  exports with source validation and execution metadata.
- `ScanReport::song_ids` / `song`: the shared selected-item catalog.
- `ScanReport::asset_relations`: a bounded projection of one item's retained
  relationships, with source evidence, unresolved references and typed locations.

Scan and graph limits are independent, and both support cancellation. A complete
graph means that the available inventory was projected within its limits. It
does not imply complete discovery, faithful playback, or source authorization.
Graph IDs are deterministic within a selected inventory, not persistent IDs
across rescans or detector revisions.

File ranges, decompressed VGM ranges and CD track geometry have separate address
domains. MP2k sample nodes retain direction and decoding metadata even when they
share source bytes. Tracker/container counts do not create invented sequences,
instruments or sample locations. Validated conversion entry points keep their
own source checks; a graph is never an export admission token.

## Features and checks

The default feature set is empty. `fuzzing` adds bounded production-parser fuzz
entry points without removing normal APIs. `test-support` adds synthetic fixture
builders for integration tests; it does not bypass production validation.

```sh
cargo test --locked -p zeff-audio-discovery
cargo test --locked -p zeff-audio-discovery --all-features
cargo check --locked -p zeff-audio-discovery --target wasm32-unknown-unknown
```

The native application can write a separate selected-song graph without changing
its normal scan report:

```sh
zeff-boy --audio-discover scan.json game.gba --audio-relations relations.json --audio-song-offset 0x100
```

For CD audio, select a track with `--audio-track`. Output paths must be new.

## GBA engine playback

The native app provides preview and WAV, FLAC and Ogg recording for recognized
Engine Software-format banks, supported GAX songs, Krawall, MusyX, AAS, descriptor
MIDI, NSQ/NPF, RAdriver, GBASS, AAS streams/PCM cues and Natsume music. Engine
Software and XM-compatible GAX graphs use approximate tracker playback. Native
GAX and the other qualified native profiles execute the source sound driver in an isolated
GBA emulator. Hardware-bit-exact output and complete soundtracks are not implied.
Unrecognized driver versions and unresolved structures remain unsupported.

GAX 1.99 profiles request a 16 kHz native mixer rate because their song headers
do not declare one. The scan records this choice; the requested output rate is
applied separately by the emulator's audio renderer.

AAS profiles bind supported eight- or sixteen-channel MOD drivers and preserve
their original startup configuration. Invalid module cells and unbound mixer-only
variants remain excluded; retained entries use their original native selector.

Descriptor MIDI profiles bind sparse selectors to Standard MIDI Files and their
native player/bank configuration. MIDI export preserves the original file;
ordinary MIDI players may interpret its channels and banks differently. Native
playback uses the game's track allocation and loop markers. NSQ/NPF profiles
bind frame-timed sequences and instrument banks through a hashed filesystem,
preserving distinct selectors even when they share a sequence file.
Their mapped assets include the samples required to initialize the entire bank.

The same PCM session supplies preview and recording. Duration and fade settings
apply to recordings; loop-pass and MP2k-specific MIDI/gain controls do not.
Mapped asset exports contain only the retained source ranges. Engine Software
and compatible GAX graphs also support XM and tracker-pack exports.

Select an exact catalogue item with `--audio-song-id engine:index`, using a
zero-based index within that engine's report array. The new engine names are
`engine_software`, `gax_native`, `krawall`, `musyx`, `aas`, `descriptor_midi`, `nsq`,
`radriver`, `gbass`, `aas_stream`, `aas_pcm` and `natsume`; existing parsed GAX graphs
use `gax`. This also selects Krawall subsongs that share a module header offset.
IDs refer to one scan result and can change after detector or limit changes.
Large inventories can use `--audio-scan-candidates 4096`; the default remains 1024.

```sh
zeff-boy --audio-discover scan.json game.gba --audio-song-id krawall:0 --audio-export wav song.wav --audio-max-seconds 30 --audio-sample-rate 48000
```

GBASS profiles may expose separate banks with the same original song number.
Their catalog positions remain distinct. Qualified Hardcore Pinball banks retain
the original bank argument, RAM configuration and sample-format flags.

## Original-driver music exports

The desktop app and CLI offer GSF and miniGSF packs for qualified native GBA
profiles and supported stock MP2k drivers. Native GSF playback starts the source
driver without a preview-host handshake. The cartridge and playback bootstrap
are retained; these exports are not size-optimized soundtrack rips.

Native miniGSF packs contain a shared source GSFlib, small playback patches and
a manifest. Extract every file together. Packs from the same cartridge share
the source library. Native duration and fade settings become player tags, with
the fade included in the duration; automatic loop counts and alignment with
preview startup samples are not provided. MP2k keeps its sequence-based timing.

```sh
zeff-boy --audio-discover scan.json game.gba --audio-song-id gbass:0 --audio-export gsf song.gsf --audio-max-seconds 180 --audio-fade-seconds 5
zeff-boy --audio-discover scan.json game.gba --audio-song-id gbass:0 --audio-export minigsf song-minigsf.zip --audio-max-seconds 180 --audio-fade-seconds 5
```

**Export all songs…** writes every catalog entry supported by the selected
format to one ZIP, including entries hidden by the current list filter.
Entries keep distinct engine, bank and module identities. MiniGSF songs share matching
libraries in each engine folder; extract the ZIP with its folders intact.
`batch-report.json` records settings, file hashes, skipped entries and individual
failures; `scan-report.json` preserves the original catalog and source mappings.
Cancel discards the unfinished ZIP, and existing files are never
replaced. CLI batches with failed entries retain the report and return an error.

```sh
zeff-boy --audio-discover scan.json game.gba --audio-all-songs --audio-export minigsf all-songs.zip --audio-max-seconds 180 --audio-fade-seconds 5
```

Qualified returning drivers also export GBS for Game Boy, NSF for NES and SGC
for Sega. The format selector shows only supported combinations. These files
contain the original sound program and the selected cue; imported GBS/NSF files
retain their original bytes when exported. The portable encoder also returns
source identity, mapping, callback and timing metadata.

NSF rounds the NTSC frame period to microseconds. SGC uses its standard 60 Hz
calls and preserves each driver's frame divider. Drivers requiring original data
in SGC's reserved low-ROM area remain unavailable. Native rip timing, output
rate and looping are controlled by the external player.

```sh
zeff-boy --audio-discover scan.json game.nes --audio-song-id nes_native:0 --audio-export nsf song.nsf
zeff-boy --audio-discover scan.json game.gg --audio-song-id sega_psg:0 --audio-export sgc song.sgc
```
