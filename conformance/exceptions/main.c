#include <stddef.h>

#include "minemu/conformance.h"
#include "minemu/mmu.h"
#include "minemu/platform.h"
#include "minemu/trap.h"

#define DATA_VADDR UINT32_C(0x20000000)

extern void minemu_trigger_undefined(void);
extern void minemu_cp15_condition_false(void);
extern void minemu_run_user_probe(void);
extern void minemu_user_probe(void);
extern void minemu_prefetch_probe(void);

static uint32_t data_table[1024] __attribute__((aligned(4096)));
static uint32_t data_page[1024] __attribute__((aligned(4096)));
static volatile uint32_t undefined_stage;
static volatile uint32_t abort_stage;
static volatile uint32_t svc_count;
static volatile uint32_t irq_count;
static volatile uint32_t user_device_stage;
static uintptr_t expected_svc_frame;

extern uint8_t __minemu_svc_stack_top[];
extern uint8_t __minemu_irq_stack_top[];
extern uint8_t __minemu_abt_stack_top[];
extern uint8_t __minemu_und_stack_top[];

static void require_exact_stack(const void *frame, const uint8_t *top, uint32_t code) {
    uintptr_t address = (uintptr_t)frame;
    uintptr_t stack_top = (uintptr_t)top;
    MINEMU_REQUIRE(address == stack_top - sizeof(struct minemu_trap_frame) &&
                       (address & 7U) == 0U,
                   code);
}

static volatile uint32_t *direct_pte(const void *address) {
    uint32_t value = (uint32_t)(uintptr_t)address;
    return (volatile uint32_t *)(uintptr_t)(UINT32_C(0xc0012000) +
                                            (((value >> 12) & UINT32_C(0x3ff)) * 4U));
}

struct minemu_trap_frame *minemu_svc_dispatch(struct minemu_trap_frame *frame) {
    uintptr_t address = (uintptr_t)frame;
    uintptr_t stack_top = (uintptr_t)__minemu_svc_stack_top;
    MINEMU_REQUIRE(address == expected_svc_frame && address >= stack_top - 4096U &&
                       address < stack_top && (address & 7U) == 0U,
                   59);
    MINEMU_REQUIRE(frame->exception_id == MINEMU_EXCEPTION_SVC, 30);
    MINEMU_REQUIRE(frame->return_lr == frame->fault_pc + 4U && frame->reserved == 0U, 53);
    MINEMU_REQUIRE((frame->spsr & UINT32_C(0x1f)) == (svc_count == 0U ? 0x13U : 0x10U), 31);
    ++svc_count;
    if ((frame->spsr & UINT32_C(0x1f)) == 0x10U) {
        frame->spsr = (frame->spsr & ~UINT32_C(0x1f)) | UINT32_C(0x13);
    }
    return frame;
}

struct minemu_trap_frame *minemu_irq_dispatch(struct minemu_trap_frame *frame) {
    uint32_t cpsr;
    __asm__ volatile("mrs %0, cpsr" : "=r"(cpsr));
    require_exact_stack(frame, __minemu_irq_stack_top, 60);
    MINEMU_REQUIRE((cpsr & UINT32_C(0x80)) != 0U, 61);
    MINEMU_REQUIRE(frame->exception_id == (int32_t)MINEMU_IRQ_SYSTICK, 32);
    MINEMU_REQUIRE(frame->return_lr == frame->fault_pc + 4U && frame->reserved == 0U, 54);
    MINEMU_REQUIRE((MINEMU_SYSTICK->status & MINEMU_SYSTICK_STATUS_PENDING) != 0U, 33);
    MINEMU_SYSTICK->ack = MINEMU_SYSTICK_ACK;
    MINEMU_INTERRUPT->eoi = MINEMU_IRQ_SYSTICK;
    ++irq_count;
    return frame;
}

void minemu_undefined_dispatch(struct minemu_trap_frame *frame) {
    require_exact_stack(frame, __minemu_und_stack_top, 62);
    MINEMU_REQUIRE(frame->exception_id == MINEMU_EXCEPTION_UNDEFINED, 34);
    MINEMU_REQUIRE(frame->return_lr == frame->fault_pc + 4U && frame->reserved == 0U, 55);
    if (undefined_stage == 1U) {
        MINEMU_REQUIRE((frame->spsr & UINT32_C(0x1f)) == UINT32_C(0x13), 35);
    } else if (undefined_stage == 2U) {
        MINEMU_REQUIRE((frame->spsr & UINT32_C(0x1f)) == UINT32_C(0x10), 36);
    } else {
        minemu_conformance_fail(37);
    }
    undefined_stage = 0;
}

void minemu_abort_dispatch(struct minemu_trap_frame *frame) {
    require_exact_stack(frame, __minemu_abt_stack_top, 63);
    if (frame->exception_id == MINEMU_EXCEPTION_DATA_ABORT) {
        MINEMU_REQUIRE(frame->return_lr == frame->fault_pc + 8U, 56);
    } else if (frame->exception_id == MINEMU_EXCEPTION_PREFETCH_ABORT) {
        MINEMU_REQUIRE(frame->return_lr == frame->fault_pc + 4U, 57);
    }
    MINEMU_REQUIRE(frame->reserved == 0U, 58);
    if (user_device_stage == 1U && frame->exception_id == MINEMU_EXCEPTION_DATA_ABORT &&
        frame->dfar == MINEMU_RNG_BASE) {
        MINEMU_REQUIRE(frame->dfsr == UINT32_C(0x102), 50);
        frame->return_lr += 4;
        user_device_stage = 2;
        return;
    } else if (user_device_stage == 2U &&
               frame->exception_id == MINEMU_EXCEPTION_DATA_ABORT &&
               frame->dfar == MINEMU_TRACE_BASE) {
        MINEMU_REQUIRE(frame->dfsr == UINT32_C(0x303), 52);
        frame->return_lr += 4;
        user_device_stage = 3;
        return;
    } else if (abort_stage == 1U) {
        MINEMU_REQUIRE(frame->exception_id == MINEMU_EXCEPTION_DATA_ABORT, 40);
        MINEMU_REQUIRE(frame->dfar == DATA_VADDR && frame->dfsr == 1U, 41);
        data_table[0] = minemu_test_paddr(data_page) | MINEMU_PTE_VALID |
                        MINEMU_PTE_READABLE | MINEMU_PTE_WRITABLE;
    } else if (abort_stage == 2U) {
        MINEMU_REQUIRE(frame->exception_id == MINEMU_EXCEPTION_PREFETCH_ABORT, 42);
        MINEMU_REQUIRE(frame->dfar == (uint32_t)(uintptr_t)minemu_prefetch_probe, 43);
        MINEMU_REQUIRE(frame->dfsr == UINT32_C(0x404), 44);
        *direct_pte(minemu_prefetch_probe) |= MINEMU_PTE_EXECUTABLE;
    } else if (abort_stage == 3U) {
        MINEMU_REQUIRE(frame->exception_id == MINEMU_EXCEPTION_PREFETCH_ABORT, 45);
        MINEMU_REQUIRE(frame->dfar == (uint32_t)(uintptr_t)minemu_user_probe, 46);
        MINEMU_REQUIRE(frame->dfsr == UINT32_C(0x504), 47);
        *direct_pte(minemu_user_probe) |= MINEMU_PTE_USER;
    } else {
        minemu_trace_event(UINT32_C(0xe0070000) |
                           (((uint32_t)frame->exception_id & UINT32_C(0xff)) << 8) |
                           (frame->dfsr & UINT32_C(0xff)));
        minemu_trace_event(frame->dfar);
        minemu_conformance_fail(48);
    }
    minemu_mmu_tlbiall();
    abort_stage = 0;
}

void minemu_kernel_main(const void *boot_info) {
    (void)boot_info;
    volatile uint32_t *root = (volatile uint32_t *)(uintptr_t)UINT32_C(0xc0010000);
    volatile uint32_t *data = (volatile uint32_t *)(uintptr_t)DATA_VADDR;
    for (size_t i = 0; i < 1024; ++i) {
        data_table[i] = 0;
    }
    root[DATA_VADDR >> 22] = minemu_test_paddr(data_table) | MINEMU_PTE_VALID;

    __asm__ volatile("mov %0, sp" : "=r"(expected_svc_frame));
    expected_svc_frame -= sizeof(struct minemu_trap_frame);
    __asm__ volatile("svc #0");
    MINEMU_REQUIRE(svc_count == 1U, 1);

    undefined_stage = 1;
    minemu_trigger_undefined();
    MINEMU_REQUIRE(undefined_stage == 0U, 2);

    minemu_cp15_condition_false();

    abort_stage = 1;
    MINEMU_REQUIRE(*data == 0U, 3);
    MINEMU_REQUIRE(abort_stage == 0U, 4);

    volatile uint32_t *prefetch_pte = direct_pte(minemu_prefetch_probe);
    *prefetch_pte &= ~MINEMU_PTE_EXECUTABLE;
    minemu_mmu_tlbiall();
    abort_stage = 2;
    minemu_prefetch_probe();
    MINEMU_REQUIRE(abort_stage == 0U, 5);

    *direct_pte(minemu_user_probe) &= ~MINEMU_PTE_USER;
    minemu_mmu_tlbiall();
    abort_stage = 3;
    undefined_stage = 2;
    user_device_stage = 1;
    __asm__ volatile("mov %0, sp" : "=r"(expected_svc_frame));
    expected_svc_frame -= sizeof(struct minemu_trap_frame);
    minemu_run_user_probe();
    MINEMU_REQUIRE(abort_stage == 0U, 6);
    MINEMU_REQUIRE(undefined_stage == 0U, 7);
    MINEMU_REQUIRE(svc_count == 2U, 8);
    MINEMU_REQUIRE(user_device_stage == 3U, 10);

    MINEMU_INTERRUPT->enable = UINT32_C(1);
    MINEMU_SYSTICK->period = 64;
    MINEMU_SYSTICK->control =
        MINEMU_SYSTICK_CONTROL_ENABLE | MINEMU_SYSTICK_CONTROL_IRQ_ENABLE;
    __asm__ volatile("cpsie i" ::: "memory");
    while (irq_count == 0U) {
    }
    __asm__ volatile("cpsid i" ::: "memory");
    MINEMU_REQUIRE(irq_count == 1U, 9);

    minemu_trace_event(UINT32_C(0x20070001));
    minemu_fail_stop();
}
