OUTPUT_ARCH(riscv)
ENTRY(_start)
/* BASE_ADDRESS is defined in build.rs via generated linker fragment */

SECTIONS
{
    . = BASE_ADDRESS;
    skernel = .;
    stext = .;
    .text : {
        *(.text.entry)
        *(.text .text.*)
    }

    . = ALIGN(4K);
    etext = .;
    srodata = .;
    .rodata : {
        *(.rodata .rodata.*)
        *(.srodata .srodata.*)
    }

    . = ALIGN(4K);
    erodata = .;
    sdata = .;
    .data : {
        *(.data .data.*)
        *(.sdata .sdata.*)
    }

    . = ALIGN(4K);
    edata = .;
    .bss : {
        *(.bss.stack)
        sbss = .;
        *(.bss .bss.*)
        *(.sbss .sbss.*)
        *( .bss.queue)
    }

    . = ALIGN(4K);
    ebss = .;
    ekernel = .;

    /DISCARD/ : {
        *(.eh_frame)
        *(.comment)    /* 加上这一行，彻底丢弃编译器注释 */
        *(.riscv.attributes) /* 建议顺便把这个也加上，防止它作妖 */
    }

    _end = .;
}
