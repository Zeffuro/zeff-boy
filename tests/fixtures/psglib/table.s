        .module table_dispatch
        .globl _table_play
        .globl _song_table
        .globl _psglib_song_0
        .globl _psglib_song_1
        .globl _PSGPlay

        .area _CODE
_table_play:
        cp #2
        ret nc
        ld l,a
        ld h,#0
        add hl,hl
        ld de,#_song_table
        add hl,de
        ld e,(hl)
        inc hl
        ld d,(hl)
        ex de,hl
        jp _PSGPlay

_song_table:
        .dw _psglib_song_0
        .dw _psglib_song_1
