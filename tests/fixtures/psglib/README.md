# PSGlib fixture

Authored two-song Master System fixture using the public-domain PSGlib source
pinned and recorded by the builder. It requires a relocatable SDCC 4.5.0 toolchain.

Build and verify in new ignored directories:

```powershell
python scripts/build-psglib-fixture.py --sdcc C:\toolchain\bin\sdcc.exe --out-dir .tmp\psglib-fixture
python scripts/verify-psglib-fixture.py --zeff-boy target\release\zeff-boy.exe --fixture-dir .tmp\psglib-fixture --out-dir .tmp\psglib-proof --automatic
```

Use `--code-loc 0x380 --data-loc 0xc100` for relocated code and state.

To verify the guarded table when only one entry boots:

```powershell
python scripts/build-psglib-fixture.py --sdcc C:\toolchain\bin\sdcc.exe --table-selector 0 --out-dir .tmp\psglib-table
python scripts/verify-psglib-table.py --zeff-boy target\debug\zeff-boy.exe --fixture-dir .tmp\psglib-table --out-dir .tmp\psglib-table-proof
```

This qualifies the stated pinned driver and table contract only; it does not
claim arbitrary PSGlib builds, retail coverage, or complete soundtrack discovery.
