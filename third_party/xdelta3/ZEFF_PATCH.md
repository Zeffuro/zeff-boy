# Zeff Boy changes

This is the crates.io `xdelta3` 0.1.5 wrapper with xdelta 3.0.12 APL from
`jmacd/xdelta` commit `0525275fe4b553a10f38e455d30c60dc6ed9b45d`.

The C compile closure is tracked under `vendor/`; it is not a nested submodule.
Its Apache-2.0 license is preserved as `vendor/LICENSE`.

- Replaced generated bindings with the two memory API declarations used here.
- Removed the libclang/bindgen build dependency.
- Derive target C type sizes from Cargo target metadata instead of executing a
  temporary target binary during the build.
- Enabled the DJW and FGK secondary compressors.
- Replaced the upstream fixture paths with self-contained round-trip coverage.
