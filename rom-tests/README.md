# ROM tests

ROM binaries, local paths, caches, and results are ignored. Keep only manifests and baselines here.

```powershell
just romtest-status
just romtest-fetch
just romtest-smoke
just romtest-run
```

Local compatibility entries belong in an ignored `local*.toml` copied from
`manifests/compat-games/example.toml`.

```powershell
just romtest-generate-compat D:\Roms rom-tests/manifests/compat-games/local-games.toml
just romtest-run-compat
```

Some local suites must be built first:

```powershell
just romtest-build-ws-suite
just romtest-build-sega8-smoke
just romtest-build-pce-cd-fixture
```

The WonderSwan suite is MIT-licensed; keep its generated license notice with
any redistribution. The generated PCE CD and Sega 8-bit fixtures are local
test artifacts. Do not commit commercial games, ROM dumps, or local reports.
