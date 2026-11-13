#include <stddef.h>
#include <stdint.h>

#include "minemu/conformance.h"
#include "minemu/mmu.h"

#define TEST_VADDR UINT32_C(0x20000000)
#define TEST_DIRECTORY_INDEX (TEST_VADDR >> 22)
#define PAGE_FLAGS (MINEMU_PTE_VALID | MINEMU_PTE_READABLE | MINEMU_PTE_WRITABLE)

static uint32_t root_b[1024] __attribute__((aligned(4096)));
static uint32_t table_a[1024] __attribute__((aligned(4096)));
static uint32_t table_b[1024] __attribute__((aligned(4096)));
static uint32_t page_a[1024] __attribute__((aligned(4096)));
static uint32_t page_b[1024] __attribute__((aligned(4096)));

void minemu_kernel_main(const void *boot_info) {
    (void)boot_info;
    volatile uint32_t *root_a = (volatile uint32_t *)(uintptr_t)UINT32_C(0xc0010000);
    volatile uint32_t *test = (volatile uint32_t *)(uintptr_t)TEST_VADDR;

    for (size_t i = 0; i < 1024; ++i) {
        root_b[i] = root_a[i];
        table_a[i] = 0;
        table_b[i] = 0;
    }
    page_a[0] = UINT32_C(0xaaaaaaaa);
    page_b[0] = UINT32_C(0xbbbbbbbb);
    table_a[0] = minemu_test_paddr(page_a) | PAGE_FLAGS;
    table_b[0] = minemu_test_paddr(page_b) | PAGE_FLAGS;
    root_a[TEST_DIRECTORY_INDEX] = minemu_test_paddr(table_a) | MINEMU_PTE_VALID;
    root_b[TEST_DIRECTORY_INDEX] = minemu_test_paddr(table_b) | MINEMU_PTE_VALID;

    minemu_mmu_tlbiall();
    MINEMU_REQUIRE(*test == UINT32_C(0xaaaaaaaa), 1);

    minemu_mmu_set_ttbr0(minemu_test_paddr(root_b));
    MINEMU_REQUIRE(*test == UINT32_C(0xaaaaaaaa), 2);
    minemu_mmu_tlbiall();
    MINEMU_REQUIRE(*test == UINT32_C(0xbbbbbbbb), 3);

    minemu_mmu_set_ttbr0(UINT32_C(0x40010000));
    minemu_mmu_tlbiall();
    MINEMU_REQUIRE(*test == UINT32_C(0xaaaaaaaa), 4);

    minemu_trace_event(UINT32_C(0x20040001));
    minemu_fail_stop();
}
