# Zeff Boy compatibility patch

This directory vendors `block` 0.1.6 from upstream tag `0.1.6`, revision
`47178790cfc9d4a8b092051d8b413b78bd31254a`.
The original crates.io archive checksum is
`0d8c1fef690941d3e7788d328517591fecc684c084084702d6ff1641e993699a`.

The upstream `Class` placeholder is an empty enum. Rust's
`uninhabited_static` future-incompatibility lint rejects its use as the type of
the external `_NSConcreteStackBlock` static. The local source replaces that
enum with a zero-sized, inhabited `repr(C)` struct. It remains opaque and is
used only behind a pointer, so this does not change the Objective-C Blocks ABI.

The patch can be removed when the macOS camera dependency chain no longer uses
`block` 0.1.x.
