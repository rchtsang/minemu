#include <stddef.h>

#include "minemu/conformance.h"
#include "minemu/mmu.h"
#include "minemu/trap.h"

#define TEST_VADDR UINT32_C(0x20000000)
#define TEST_DIRECTORY_INDEX (TEST_VADDR >> 22)
#define METADATA (UINT32_C(0x15) << 7)

static uint32_t table[1024] __attribute__((aligned(4096)));
static uint32_t page[1024] __attribute__((aligned(4096)));
static volatile uint32_t fault_stage;

void minemu_abort_dispatch(struct minemu_trap_frame *frame) {
    if (frame->exception_id != MINEMU_EXCEPTION_DATA_ABORT) {
        minemu_trace_event(UINT32_C(0xe0060000) |
                           (((uint32_t)frame->exception_id & UINT32_C(0xff)) << 8) |
                           (frame->dfsr & UINT32_C(0xff)));
        minemu_conformance_fail(20);
    }
    MINEMU_REQUIRE(frame->dfar == TEST_VADDR, 21);
    MINEMU_REQUIRE((table[0] & (MINEMU_PTE_ACCESSED | MINEMU_PTE_DIRTY)) == 0U, 22);
    if (fault_stage == 1U) {
        MINEMU_REQUIRE(frame->dfsr == UINT32_C(2), 23);
        table[0] |= MINEMU_PTE_READABLE;
    } else if (fault_stage == 2U) {
        MINEMU_REQUIRE(frame->dfsr == UINT32_C(0x203), 24);
        table[0] |= MINEMU_PTE_WRITABLE;
    } else if (fault_stage == 3U) {
        MINEMU_REQUIRE(frame->dfsr == UINT32_C(0x203), 25);
        frame->return_lr += 4;
    } else {
        minemu_conformance_fail(26);
    }
    minemu_mmu_tlbiall();
    fault_stage = 0;
}

void minemu_kernel_main(const void *boot_info) {
    (void)boot_info;
    volatile uint32_t *root = (volatile uint32_t *)(uintptr_t)UINT32_C(0xc0010000);
    volatile uint32_t *test = (volatile uint32_t *)(uintptr_t)TEST_VADDR;
    for (size_t i = 0; i < 1024; ++i) {
        table[i] = 0;
    }
    root[TEST_DIRECTORY_INDEX] = minemu_test_paddr(table) | MINEMU_PTE_VALID;

    table[0] = minemu_test_paddr(page) | MINEMU_PTE_VALID | MINEMU_PTE_READABLE |
               MINEMU_PTE_WRITABLE | METADATA;
    minemu_mmu_tlbiall();
    *test = UINT32_C(0x12345678);
    MINEMU_REQUIRE(page[0] == UINT32_C(0x12345678), 1);
    MINEMU_REQUIRE(
        (table[0] & (MINEMU_PTE_ACCESSED | MINEMU_PTE_DIRTY)) ==
            (MINEMU_PTE_ACCESSED | MINEMU_PTE_DIRTY),
        2);
    MINEMU_REQUIRE((table[0] & (UINT32_C(0x1f) << 7)) == METADATA, 3);

    table[0] &= ~(MINEMU_PTE_ACCESSED | MINEMU_PTE_DIRTY);
    minemu_mmu_tlbiall();
    MINEMU_REQUIRE(*test == UINT32_C(0x12345678), 4);
    MINEMU_REQUIRE((table[0] & MINEMU_PTE_ACCESSED) != 0U, 5);
    MINEMU_REQUIRE((table[0] & MINEMU_PTE_DIRTY) == 0U, 6);

    table[0] = minemu_test_paddr(page) | MINEMU_PTE_VALID | MINEMU_PTE_WRITABLE | METADATA;
    minemu_mmu_tlbiall();
    fault_stage = 1;
    MINEMU_REQUIRE(*test == UINT32_C(0x12345678), 7);
    MINEMU_REQUIRE(fault_stage == 0U, 8);

    table[0] = minemu_test_paddr(page) | MINEMU_PTE_VALID | MINEMU_PTE_READABLE | METADATA;
    minemu_mmu_tlbiall();
    fault_stage = 2;
    *test = UINT32_C(0x87654321);
    MINEMU_REQUIRE(page[0] == UINT32_C(0x87654321), 9);
    MINEMU_REQUIRE(fault_stage == 0U, 10);

    table[0] = UINT32_C(0x08000000) | MINEMU_PTE_VALID | MINEMU_PTE_READABLE |
               MINEMU_PTE_WRITABLE;
    minemu_mmu_tlbiall();
    fault_stage = 3;
    *test = 0;
    MINEMU_REQUIRE(fault_stage == 0U, 11);

    minemu_trace_event(UINT32_C(0x20060001));
    minemu_fail_stop();
}
