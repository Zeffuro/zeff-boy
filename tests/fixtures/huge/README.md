# hUGEDriver fixture

Authored four-channel, two-order ROM using public-domain hUGEDriver v6.1.3 and
RGBDS v1.0.3. The build script pins and records the upstream revision.

Build and verify in new ignored directories:

```powershell
python scripts/build-huge-fixture.py --rgbds C:\tools\rgbds\bin --out-dir .tmp\huge-fixture
cargo build -p zeff-audio-discovery --example huge-inspect
python scripts/verify-huge-fixture.py --zeff-boy target\debug\zeff-boy.exe --inspector target\debug\examples\huge-inspect.exe --fixture-dir .tmp\huge-fixture --out-dir .tmp\huge-proof
```

Use `--code 0x800 --song 0x3000 --ram 0xc200` for a second layout and
`--compare-proof .tmp\huge-proof\proof.json` when verifying it.

For the isolated copied-audio check:

```powershell
cargo build --example huge-closure
cargo build -p zeff-audio-discovery --example huge-isolate
python scripts/verify-huge-isolation.py --zeff-boy target\debug\zeff-boy.exe --isolator target\debug\examples\huge-isolate.exe --closure target\debug\examples\huge-closure.exe --fixture-dir .tmp\huge-fixture --original-proof .tmp\huge-proof\proof.json --out-dir .tmp\huge-isolation
```

The fixture proves only its stated binding and bootstrap contract; it does not
qualify arbitrary games or complete soundtrack coverage.
