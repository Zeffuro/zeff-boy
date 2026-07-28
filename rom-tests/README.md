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

Do not commit commercial games or random ROM dumps.
