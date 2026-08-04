# ROM tests

Metadata for test ROMs lives here. The ROMs themselves do not.

```powershell
just romtest-status
just romtest-fetch
just romtest-smoke
just romtest-run
just romtest-compare

# local-only ROM cache
just romtest-run-local
just romtest-status-local

# local user-owned compatibility games
just romtest-list-compat
just romtest-run-compat
just romtest-status-compat
```

`manifests/test-roms` is for public test ROMs.
`manifests/compat-games` is for local game compatibility notes. Copy `example.toml` to an ignored `local*.toml` file before adding real game paths or names.
`cache` and `results` are ignored.

Some local-tier suites are source-only. Build them into the ignored cache before running the
corresponding local tests:

```powershell
just romtest-build-ws-suite
just romtest-run-local-ws
```

`scripts/build-ws-test-suite.ps1` downloads the pinned MIT-licensed
`asiekierka/ws-test-suite` source archive, verifies its SHA-256, builds upstream's default
`TARGET=wswan/small`, and copies generated `.ws`/`.wsc` files from `build/roms/*` into
`rom-tests/cache/ws/asiekierka/ws-test-suite/`. It uses Docker by default when Docker is available;
run it from a Wonderful Toolchain shell with `-Builder local` if you prefer a local toolchain.
Generated ROMs remain out of git. If you redistribute them elsewhere, include the upstream MIT
license notice copied into the cache as `LICENSE.ws-test-suite.txt`.

Do not commit commercial games or random ROM dumps.
