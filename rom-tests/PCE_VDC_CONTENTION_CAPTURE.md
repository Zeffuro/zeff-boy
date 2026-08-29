# PC Engine VDC contention diagnostic

Build with `just romtest-build-pce-vdc-contention-fixture`. The generated HuCard
uses raster IRQs to run the same 16-byte CPU-to-VRAM transfer on 16 active lines
for each MWR access mode. The sampled active rows are 32-62, 64-94, 96-126,
and 128-158 in two-row steps. MWR 0-3 end in red, blue, green, and white.

The black-to-color edge in each row is the measurement. Capture unscaled RGB
from a stock PC Engine after a cold boot, repeat after a warm reset, and record
the first colored pixel of every row relative to the active-picture origin.
Record the console revision, HuCard/flash device, capture chain, and every raw
row result. A menu-driven flash device may change initial VDC phase, so retain
both cold-boot and reset samples.

`ZPCE` status means all four bands executed. The extended `VDCS` payload at
work RAM `$200F` identifies format version 1 and keeps four `$FFFF` split fields
until hardware values are captured. It is not a timing pass or an accuracy
oracle.

The slot modes follow the hardware-tested
[`pcetech.txt`](https://github.com/asterick/TurboSharp/blob/c726a2dafe8316ad249ffd3503016569a844a89a/Text/pcetech.txt#L808-L845).
[Chris Covell's hardware captures](https://chrismcovell.com/CPUTest/index.html)
informed the cold-boot warning. No code or data from that unlicensed test ROM is
included.
