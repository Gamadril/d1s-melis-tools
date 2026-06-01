OUTPUT_ARCH(riscv)
ENTRY(_start)

SECTIONS
{
    . = 0x00020000;
    .text : {
        *(.text._start)
        *(.text .text.*)
    }
    .rodata : {
        *(.rodata .rodata.*)
    }
    .data : {
        *(.data .data.*)
    }
    .bss : {
        *(.bss .bss.*)
    }
}
