.syntax unified
.cpu cortex-a9
.arm

.global minemu_svc_trampoline
.type minemu_svc_trampoline, %function
.extern minemu_svc_dispatch
minemu_svc_trampoline:
    sub sp, sp, #88
    stmia sp, {r0-r12}
    str lr, [sp, #52]
    mrs r1, spsr
    str r1, [sp, #56]
    ldr r1, =-3
    str r1, [sp, #60]
    sub r1, lr, #4
    str r1, [sp, #64]
    mov r1, #0
    str r1, [sp, #68]
    str r1, [sp, #72]
    add r1, sp, #76
    stmia r1, {sp, lr}^
    mov r1, #0
    str r1, [sp, #84]
    mov r0, sp
    bl minemu_svc_dispatch
    mov sp, r0
    ldr lr, [sp, #52]
    ldr r1, [sp, #56]
    msr spsr_cxsf, r1
    add r1, sp, #76
    ldmia r1, {sp, lr}^
    ldmia sp, {r0-r12}
    add sp, sp, #88
    movs pc, lr

.global minemu_irq_trampoline
.type minemu_irq_trampoline, %function
.extern minemu_irq_dispatch
minemu_irq_trampoline:
    sub sp, sp, #88
    stmia sp, {r0-r12}
    str lr, [sp, #52]
    mrs r1, spsr
    str r1, [sp, #56]
    ldr r1, =0x10000000
    ldr r1, [r1, #8]
    str r1, [sp, #60]
    sub r1, lr, #4
    str r1, [sp, #64]
    mov r1, #0
    str r1, [sp, #68]
    str r1, [sp, #72]
    add r1, sp, #76
    stmia r1, {sp, lr}^
    mov r1, #0
    str r1, [sp, #84]
    mov r0, sp
    bl minemu_irq_dispatch
    mov sp, r0
    ldr lr, [sp, #52]
    ldr r1, [sp, #56]
    msr spsr_cxsf, r1
    add r1, sp, #76
    ldmia r1, {sp, lr}^
    ldmia sp, {r0-r12}
    add sp, sp, #88
    subs pc, lr, #4

.global minemu_trigger_undefined
.type minemu_trigger_undefined, %function
minemu_trigger_undefined:
    .word 0xe7f000f0
    bx lr

.global minemu_cp15_condition_false
.type minemu_cp15_condition_false, %function
minemu_cp15_condition_false:
    mov r0, #0
    cmp r0, #1
    .word 0x0e010f10
    bx lr

.global minemu_run_user_probe
.type minemu_run_user_probe, %function
minemu_run_user_probe:
    mov r4, lr
    ldr r0, =0x20001000
    msr cpsr_c, #0xdf
    mov sp, r0
    msr cpsr_c, #0xd3
    ldr r0, =minemu_user_probe
    msr spsr_cxsf, #0x10
    movs pc, r0

.section .text.user_probe, "ax", %progbits
.balign 4096
.global minemu_user_probe
.type minemu_user_probe, %function
minemu_user_probe:
    ldr r0, =0x10003000
    ldr r1, [r0]
    ldr r0, =0x1000f000
    str r1, [r0]
    mrc p15, 0, r0, c5, c0, 0
    svc #0
    bx r4

.section .text.prefetch_probe, "ax", %progbits
.balign 4096
.global minemu_prefetch_probe
.type minemu_prefetch_probe, %function
minemu_prefetch_probe:
    bx lr
