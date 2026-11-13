.syntax unified
.cpu cortex-a9
.arm

.global minemu_block_exact_32
.type minemu_block_exact_32, %function
minemu_block_exact_32:
    ldr r2, =0x10002000
    str r0, [r2]
    .rept 30
    nop
    .endr
    ldr r0, [r2, #16]
    ldr r1, [r2, #16]
    bx lr
