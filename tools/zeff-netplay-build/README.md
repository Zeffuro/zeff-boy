Diagnostic build-input collector for native NES netplay:

```sh
cargo +1.99.0 build --locked -p zeff-netplay-build
target/debug/zeff-netplay-build build --root . -- +1.99.0 build --locked -p zeff-boy --bin zeff-boy
```

On Windows, use `target/debug/zeff-netplay-build.exe`. Observations are written
under `.tmp/netplay-build-observations`; build outputs use `target/netplay-observed`.
The collector records source, compiler arguments and dependency artifacts. It
does not qualify builds or enable different-version play. Native linker/library
binding and frozen-source publication still need qualification.
