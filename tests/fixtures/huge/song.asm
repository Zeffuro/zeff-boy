INCLUDE "include/hUGE.inc"

SECTION "Fixture song", ROM0
FixtureSong::
    db 1
    dw FixtureOrderCount
    dw FixtureOrder1, FixtureOrder2, FixtureOrder3, FixtureOrder4
    dw FixtureDuty, FixtureWaveInstrument, FixtureNoise
    dw 0, FixtureWaves
FixtureOrderCount:: db 4
FixtureOrder1:: dw Pulse1A, Pulse1B
FixtureOrder2:: dw Pulse2A, Pulse2B
FixtureOrder3:: dw WaveA, WaveB
FixtureOrder4:: dw NoiseA, NoiseB

MACRO pattern
\1::
    dn \2, 1, 0
    REPT 15
        dn ___, 0, 0
    ENDR
    dn \3, 1, 0
    REPT 47
        dn ___, 0, 0
    ENDR
ENDM

    pattern Pulse1A, C_5, G_5
    pattern Pulse1B, C_6, G_6
    pattern Pulse2A, E_5, B_5
    pattern Pulse2B, E_6, B_6
    pattern WaveA, C_4, G_4
    pattern WaveB, C_5, G_5
    pattern NoiseA, C_6, G_6
    pattern NoiseB, C_7, G_7

FixtureDuty:: db 0, $80, $f0, 0, 0, $80
FixtureWaveInstrument:: db 0, $20, 0, 0, 0, $80
FixtureNoise:: db $f0, 0, 0, 0, 0, 0
FixtureWaves:: db $01, $23, $45, $67, $89, $ab, $cd, $ef, $fe, $dc, $ba, $98, $76, $54, $32, $10
