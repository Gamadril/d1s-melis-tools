// Read-only flash stub for AB53xx "CRWN" UART bootloader.
// Installed via SET_CMD_HANDLER; bootloader calls handler(ctx) for unknown cmds.
#define R(a) (*(volatile unsigned *)(a))
#define SPI0CON R(0xa8)
#define SPI0BUF R(0xac)
#define GPIOG   R(0x708)

struct ctx {                      // CRWN UART bootloader context @ $2F810
    unsigned char *tx;            // +0  tx buffer
    unsigned char *cb;            // +4  received command block
    unsigned pad[13];
    unsigned arg1;                // +60
    unsigned arg3;                // +64
};

#define rom_send     ((int (*)(int))0x80068)
#define rom_codekey  ((unsigned (*)(void))0x80064)
#define rom_spi_init ((void (*)(void))0x8153a)   // SPI0 on GPIOG, CS=G2 (same as boot)

static void cs(int on) { if (on) GPIOG &= ~4; else GPIOG |= 4; for (volatile int i = 0; i < 20; i++); }

static unsigned char xfer(int rx, unsigned char b)
{
    if (rx) SPI0CON |= 0x10; else SPI0CON &= ~0x10;
    SPI0BUF = rx ? 0xff : b;
    while (!(SPI0CON & 0x10000));
    return SPI0BUF;
}

static void cmd(unsigned char c, int naddr, unsigned addr)
{
    cs(1);
    xfer(0, c);
    while (naddr--) xfer(0, addr >> (8 * naddr));
}

__attribute__((section(".entry"))) int handler(struct ctx *c)
{
    unsigned char *t = c->tx;
    switch (c->cb[0]) {
    case 0x00: {                  // INIT: codekey(4) jedec(4) uid(16)
        rom_spi_init();
        cs(0);
        cmd(0xab, 3, 0); cs(0);   // release from power-down
        *(unsigned *)t = rom_codekey();
        cmd(0x9f, 0, 0);
        unsigned id = 0;
        for (int i = 0; i < 3; i++) id = id << 8 | xfer(1, 0);
        cs(0);
        *(unsigned *)(t + 4) = id;
        cmd(0x4b, 4, 0);
        for (int i = 0; i < 16; i++) t[8 + i] = xfer(1, 0);
        cs(0);
        rom_send(24);
        break;
    }
    case 0x01: {                  // READ: arg1 = addr, arg3 = length
        unsigned a = c->arg1, n = c->arg3;
        while (n) {
            unsigned k = n < 512 ? n : 512;
            cmd(0x03, 3, a);
            for (unsigned i = 0; i < k; i++) t[i] = xfer(1, 0);
            cs(0);
            rom_send(k);
            a += k; n -= k;
        }
        break;
    }
    }
    return 0;
}
