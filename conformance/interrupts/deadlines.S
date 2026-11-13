.syntax unified
.cpu cortex-a9
.arm

.global minemu_systick_exact_128
.type minemu_systick_exact_128, %function
minemu_systick_exact_128:
    ldr r2, =0x10001000
    str r0, [r2, #4]
    .rept 126
    nop
    .endr
    ldr r0, [r2, #8]
    ldr r1, [r2, #8]
    bx lr

.global minemu_systick_exact_64
.type minemu_systick_exact_64, %function
minemu_systick_exact_64:
    ldr r2, =0x10001000
    str r0, [r2, #4]
    .rept 62
    nop
    .endr
    ldr r0, [r2, #8]
    ldr r1, [r2, #8]
    mov r3, #1
    str r3, [r2, #12]
    .rept 60
    nop
    .endr
    ldr r3, [r2, #8]
    ldr r2, [r2, #8]
    orr r0, r0, r1, lsl #8
    orr r0, r0, r3, lsl #16
    orr r0, r0, r2, lsl #24
    bx lr
