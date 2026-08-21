# Zeff-boy decoder patch

This directory is pinned from `sevenz-rust2` 0.21.5, upstream revision
`e8994adc584650d7b179f4d1d68685e064cc7534`, under Apache-2.0.

The local delta adds configurable parser/allocation and decoder-memory limits,
fallible reservations for archive-declared collections, LZMA memory accounting,
encoded-header limits, CRC visibility, and COPY/LZMA/LZMA2-only decoding.
Raw LZMA decoding uses statically linked `liblzma` 0.4.8 while retaining the
bounded container parser and its streaming `Read` contract. LZMA2 continues to
use `lzma-rust2`. Zeff-boy uses one decoder thread and does not use filesystem
extraction.

Modified upstream files are `Cargo.toml`, `src/archive.rs`, `src/decoder.rs`,
`src/lib.rs`, `src/reader.rs`, and `src/writer/unpack_info.rs`. Each modified
source file carries a local-change notice as required by Apache-2.0 section 4.

OxiArc was evaluated and benchmarked as an alternative archive implementation,
but no OxiArc source code was copied, modified, linked, or distributed here.
