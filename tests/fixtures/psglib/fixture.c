typedef unsigned char u8;

void PSGPlayLoops(void *song, u8 loops);
u8 PSGGetStatus(void);

const u8 psglib_song_0[] = {
    0x80, 0x42, 0x90, 0x38,
    0x0c, 0x0e, 0x00, 0x38,
    0x01,
    0x84, 0x41, 0x92, 0x38, 0x00,
    0x82, 0x43, 0x91, 0xa4, 0x44, 0xb2, 0xe4, 0xf2,
};

const u8 psglib_song_1[] = {
    0x86, 0x41, 0x93, 0x3a,
    0x80, 0x44, 0x90, 0x38, 0x00,
};

static void wait_for_song_end(void) {
    while (PSGGetStatus()) {
        __asm
            halt
        __endasm;
    }
}

void main(void) {
    PSGPlayLoops((void *)psglib_song_0, 0);
    wait_for_song_end();
    PSGPlayLoops((void *)psglib_song_1, 0);
    wait_for_song_end();
    for (;;) {
        __asm
            halt
        __endasm;
    }
}
