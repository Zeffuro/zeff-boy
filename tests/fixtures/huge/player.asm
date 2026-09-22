INCLUDE "include/hardware.inc"

SECTION "VBlank", ROM0[$40]
    jp FixtureTick
SECTION "Header", ROM0[$100]
    nop
    jp FixtureStart
    ds $150 - @, 0

SECTION "Fixture player", ROM0[$150]
FixtureStart::
    di
    ld sp, $fffe
    xor a
    ldh [rIE], a
    ldh [rIF], a
    ldh [rAUDENA], a
    ldh [$ff81], a
    ld a, $80
    ldh [rAUDENA], a
    ld a, $ff
    ldh [rAUDTERM], a
    ld a, $77
    ldh [rAUDVOL], a
    ld hl, FixtureSong
FixtureInitCall::
    call hUGE_init
    xor a
    ldh [rIF], a
    inc a
    ldh [$ff80], a
    ldh [rIE], a
    ei
.wait:
    halt
    nop
    jr .wait

FixtureTick::
    push af
    push bc
    push de
    push hl
FixtureUpdateCall::
    call hUGE_dosound
    ldh a, [$ff81]
    inc a
    ldh [$ff81], a
    pop hl
    pop de
    pop bc
    pop af
    reti
