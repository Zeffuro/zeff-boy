        .module fixture_crt
        .globl _main
        .globl _PSGFrame

        .area _HEADER (ABS)
        .org 0x0000
        di
        im 1
        ld sp,#0xdff0
        jp fixture_start

        .org 0x0038
        jp fixture_irq

fixture_start:
        ld hl,#0xc000
        xor a
        ld (hl),a
        ld de,#0xc001
        ld bc,#0x1fff
        ldir
        ld a,#0x20
        out (#0xbf),a
        ld a,#0x81
        out (#0xbf),a
        ei
        call _main
fixture_halt:
        halt
        jr fixture_halt

fixture_irq:
        push af
        push bc
        push de
        push hl
        push ix
        push iy
        in a,(#0xbf)
        call _PSGFrame
        pop iy
        pop ix
        pop hl
        pop de
        pop bc
        pop af
        ei
        reti

        .area _CODE
