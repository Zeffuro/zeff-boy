# Zeff-Boy

**Zeff-Boy** is a Game Boy, Game Boy Color, Game Boy Advance, NES, ColecoVision, PC Engine, WonderSwan, and Sega 8-bit emulator written in Rust.
I've mainly started this project to help me learn about low level programming and emulation.

Oh and I like making cool shit.


<img src="images/GBC.png" height="250" alt="GBC"> <img src="images/NES.png" height="250" alt="NES"> <img src="images/GBA.png" height="250" alt="GBA"> <img src="images/GB%20Camera.png" height="250" alt="GB Camera">
<img src="images/IDE.png" height="700" alt="IDE Mode">

## Audio discovery

Audio Explorer finds supported music drivers, previews their tracks and exports
WAV audio and source assets. Support depends on the driver and ROM version;
some entries are sound effects or hardware presets.

Scan a ROM or render a supported PSG VGM/VGZ capture:

```text
zeff-boy --audio-discover scan.json game.gb
zeff-boy --audio-discover scan.json capture.vgm --audio-export wav capture.wav --audio-song-offset 0 --audio-max-seconds 60
```

The experimental PSGlib and hUGEDriver exporters support specific driver builds:

```text
zeff-boy --audio-psglib streams.zip game.sms --psglib-rate 60 --psglib-auto
zeff-boy --audio-huge music.zip music.gb
```

See the [PSGlib](tests/fixtures/psglib) and [hUGEDriver](tests/fixtures/huge)
fixtures for supported inputs. Choose 50 or 60 Hz for PSGlib. Captured VGM playback
runs once; loop markers are not repeated.

Capture and check native audio, or try a few input sequences automatically:

```text
zeff-boy --headless --max-frames 600 --audio-trace capture.zip --audio-dump native.f32 --input start@180-181 game.gb
zeff-boy --audio-capture-check playback.json capture.zip --audio-capture-reference-f32 native.f32 --audio-capture-wav capture.wav --audio-max-seconds 15
zeff-boy --audio-capture-sweep captures game.gb
```

Native captures support GB/GBC, base NES, Sega 8-bit/Coleco PSG, HuCard PSG and
ordinary WonderSwan audio. Captures record executed audio; they do not discover
unplayed songs. Output files and directories must be new. Run `zeff-boy --help`
for selection, duration, validation and corpus-report options.

## License

Licensed under either of:

* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

Third-party components and their licenses are listed in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall
be dual-licensed as above, without any additional terms or conditions.
