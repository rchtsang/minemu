#include "minemu/conformance.h"
#include "minemu/platform.h"

static uint8_t dma[MINEMU_BLOCK_SECTOR_SIZE] __attribute__((aligned(512)));

extern uint64_t minemu_block_exact_32(uint32_t operation);

void minemu_kernel_main(const void *boot_info) {
    (void)boot_info;
    MINEMU_BLOCK->unit = MINEMU_BLOCK_UNIT_SWAP;
    MINEMU_BLOCK->lba = 0;
    MINEMU_BLOCK->sector_count = 1;
    MINEMU_BLOCK->dma_paddr = minemu_test_paddr(dma);

    uint64_t samples = minemu_block_exact_32(MINEMU_BLOCK_COMMAND_READ);
    MINEMU_REQUIRE((uint32_t)samples == MINEMU_BLOCK_STATUS_BUSY, 1);
    MINEMU_REQUIRE(((uint32_t)(samples >> 32) & MINEMU_BLOCK_STATUS_COMPLETE) != 0U, 2);
    MINEMU_REQUIRE(MINEMU_BLOCK->error == MINEMU_BLOCK_ERROR_NO_MEDIA, 3);
    MINEMU_BLOCK->ack = MINEMU_BLOCK_ACK;

    minemu_trace_event(UINT32_C(0x20030002));
    minemu_fail_stop();
}
