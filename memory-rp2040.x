/* Raspberry Pi Pico (RP2040), 2 MiB Flash (W25Q16), nach embassy/examples/rp */
MEMORY {
    /* 256 Byte Second-Stage-Bootloader (boot2), liefert embassy-rp */
    BOOT2 : ORIGIN = 0x10000000, LENGTH = 0x100
    FLASH : ORIGIN = 0x10000100, LENGTH = 2048K - 0x100
    /* Alle RAM-Bänke als ein Block */
    RAM   : ORIGIN = 0x20000000, LENGTH = 264K
}
