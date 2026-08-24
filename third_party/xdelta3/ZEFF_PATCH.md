# Zeff Boy changes

This is the crates.io `xdelta3` 0.1.5 source, including xdelta3 3.0.12.

- Replaced generated bindings with the two memory API declarations used here.
- Removed the libclang/bindgen build dependency.
- Derive target C type sizes from Cargo target metadata instead of executing a
  temporary target binary during the build.
- Enabled the DJW and FGK secondary compressors.
