#include <stddef.h>

#include "minemu/block.h"
#include "minemu/conformance.h"
#include "minemu/platform.h"

static uint8_t dma[512] __attribute__((aligned(512)));

extern uint64_t minemu_block_exact_32(uint32_t operation);

static void wait_complete(uint32_t expected_error) {
    while ((MINEMU_BLOCK->status & MINEMU_BLOCK_STATUS_COMPLETE) == 0U) {
    }
    if (MINEMU_BLOCK->error != expected_error) {
        minemu_trace_event(
            UINT32_C(0xe0030000) | (expected_error << 8) | MINEMU_BLOCK->error);
        minemu_conformance_fail(10 + expected_error);
    }
    MINEMU_REQUIRE(
        ((MINEMU_BLOCK->status & MINEMU_BLOCK_STATUS_ERROR) != 0U) ==
            (expected_error != MINEMU_BLOCK_ERROR_NONE),
        20 + expected_error);
    MINEMU_BLOCK->ack = MINEMU_BLOCK_ACK;
}

static void command(uint32_t unit, uint32_t operation, uint32_t lba, uint32_t sectors,
                    uint32_t address) {
    MINEMU_BLOCK->unit = unit;
    MINEMU_BLOCK->lba = lba;
    MINEMU_BLOCK->sector_count = sectors;
    MINEMU_BLOCK->dma_paddr = address;
    MINEMU_BLOCK->command = operation;
}

static void exact_command(uint32_t unit, uint32_t operation, uint32_t lba, uint32_t sectors,
                          uint32_t address, uint32_t expected_error) {
    MINEMU_BLOCK->unit = unit;
    MINEMU_BLOCK->lba = lba;
    MINEMU_BLOCK->sector_count = sectors;
    MINEMU_BLOCK->dma_paddr = address;
    uint64_t samples = minemu_block_exact_32(operation);
    MINEMU_REQUIRE((uint32_t)samples == MINEMU_BLOCK_STATUS_BUSY, 6);
    uint32_t completion = (uint32_t)(samples >> 32);
    MINEMU_REQUIRE((completion & MINEMU_BLOCK_STATUS_BUSY) == 0U &&
                       (completion & MINEMU_BLOCK_STATUS_COMPLETE) != 0U,
                   7);
    wait_complete(expected_error);
}

void minemu_kernel_main(const void *boot_info) {
    (void)boot_info;
    uint32_t dma_paddr = minemu_test_paddr(dma);

    MINEMU_REQUIRE(MINEMU_BLOCK->unit == MINEMU_BLOCK_UNIT_FILESYSTEM, 1);
    exact_command(MINEMU_BLOCK_UNIT_FILESYSTEM, MINEMU_BLOCK_COMMAND_READ, 0, 1, dma_paddr,
                  MINEMU_BLOCK_ERROR_NONE);
    for (size_t i = 0; i < sizeof(dma); ++i) {
        MINEMU_REQUIRE(dma[i] == 0, 2);
        dma[i] = UINT8_C(0xa5);
    }

    exact_command(MINEMU_BLOCK_UNIT_FILESYSTEM, MINEMU_BLOCK_COMMAND_WRITE, 1, 1, dma_paddr,
                  MINEMU_BLOCK_ERROR_NONE);

    for (size_t i = 0; i < sizeof(dma); ++i) {
        dma[i] = UINT8_C(0x5a);
    }
    exact_command(MINEMU_BLOCK_UNIT_SWAP, MINEMU_BLOCK_COMMAND_WRITE, 1, 1, dma_paddr,
                  MINEMU_BLOCK_ERROR_NONE);

    for (size_t i = 0; i < sizeof(dma); ++i) {
        dma[i] = 0;
    }
    exact_command(MINEMU_BLOCK_UNIT_FILESYSTEM, MINEMU_BLOCK_COMMAND_READ, 1, 1, dma_paddr,
                  MINEMU_BLOCK_ERROR_NONE);
    for (size_t i = 0; i < sizeof(dma); ++i) {
        MINEMU_REQUIRE(dma[i] == UINT8_C(0xa5), 4);
    }

    exact_command(MINEMU_BLOCK_UNIT_SWAP, MINEMU_BLOCK_COMMAND_READ, 1, 1, dma_paddr,
                  MINEMU_BLOCK_ERROR_NONE);
    for (size_t i = 0; i < sizeof(dma); ++i) {
        MINEMU_REQUIRE(dma[i] == UINT8_C(0x5a), 8);
    }

    exact_command(MINEMU_BLOCK_UNIT_FILESYSTEM, UINT32_C(99), 0, 1, dma_paddr,
                  MINEMU_BLOCK_ERROR_INVALID_COMMAND);
    exact_command(MINEMU_BLOCK_UNIT_FILESYSTEM, MINEMU_BLOCK_COMMAND_READ, 0, 1, dma_paddr + 1U,
                  MINEMU_BLOCK_ERROR_INVALID_DMA);
    exact_command(MINEMU_BLOCK_UNIT_FILESYSTEM, MINEMU_BLOCK_COMMAND_READ, 2, 1, dma_paddr,
                  MINEMU_BLOCK_ERROR_INVALID_LBA);
    exact_command(UINT32_C(2), MINEMU_BLOCK_COMMAND_READ, 0, 1, dma_paddr,
                  MINEMU_BLOCK_ERROR_INVALID_UNIT);

    for (size_t i = 0; i < sizeof(dma); ++i) {
        dma[i] = 0;
    }
    command(MINEMU_BLOCK_UNIT_FILESYSTEM, MINEMU_BLOCK_COMMAND_READ, 1, 1, dma_paddr);
    MINEMU_BLOCK->unit = MINEMU_BLOCK_UNIT_SWAP;
    MINEMU_BLOCK->command = MINEMU_BLOCK_COMMAND_READ;
    MINEMU_REQUIRE(MINEMU_BLOCK->error == MINEMU_BLOCK_ERROR_BUSY, 5);
    MINEMU_BLOCK->ack = MINEMU_BLOCK_ACK;
    wait_complete(MINEMU_BLOCK_ERROR_NONE);
    MINEMU_REQUIRE(MINEMU_BLOCK->unit == MINEMU_BLOCK_UNIT_SWAP, 9);
    for (size_t i = 0; i < sizeof(dma); ++i) {
        MINEMU_REQUIRE(dma[i] == UINT8_C(0xa5), 11);
    }

    MINEMU_INTERRUPT->enable = UINT32_C(1) << MINEMU_IRQ_BLOCK;
    MINEMU_BLOCK->control = MINEMU_BLOCK_CONTROL_IRQ_ENABLE;
    for (uint32_t unit = MINEMU_BLOCK_UNIT_FILESYSTEM; unit <= MINEMU_BLOCK_UNIT_SWAP; ++unit) {
        command(unit, MINEMU_BLOCK_COMMAND_READ, 0, 1, dma_paddr);
        while ((MINEMU_BLOCK->status & MINEMU_BLOCK_STATUS_COMPLETE) == 0U) {
        }
        MINEMU_REQUIRE((MINEMU_INTERRUPT->pending & (UINT32_C(1) << MINEMU_IRQ_BLOCK)) != 0U,
                       30 + unit * 3U);
        MINEMU_REQUIRE(MINEMU_INTERRUPT->claim == MINEMU_IRQ_BLOCK, 31 + unit * 3U);
        MINEMU_BLOCK->ack = MINEMU_BLOCK_ACK;
        MINEMU_INTERRUPT->eoi = MINEMU_IRQ_BLOCK;
        MINEMU_REQUIRE((MINEMU_INTERRUPT->pending & (UINT32_C(1) << MINEMU_IRQ_BLOCK)) == 0U,
                       32 + unit * 3U);
    }

    for (size_t i = 0; i < sizeof(dma); ++i) {
        dma[i] = UINT8_C(0x3c);
    }
    MINEMU_REQUIRE(minemu_block_write(MINEMU_BLOCK_UNIT_SWAP, 0, 1, dma_paddr) ==
                       MINEMU_BLOCK_ERROR_NONE,
                   40);
    for (size_t i = 0; i < sizeof(dma); ++i) {
        dma[i] = 0;
    }
    MINEMU_REQUIRE(minemu_block_read(MINEMU_BLOCK_UNIT_SWAP, 0, 1, dma_paddr) ==
                       MINEMU_BLOCK_ERROR_NONE,
                   41);
    for (size_t i = 0; i < sizeof(dma); ++i) {
        MINEMU_REQUIRE(dma[i] == UINT8_C(0x3c), 42);
    }
    MINEMU_REQUIRE(minemu_block_read(UINT32_C(2), 0, 1, dma_paddr) ==
                       MINEMU_BLOCK_ERROR_INVALID_UNIT,
                   43);
    MINEMU_REQUIRE((MINEMU_BLOCK->status &
                    (MINEMU_BLOCK_STATUS_COMPLETE | MINEMU_BLOCK_STATUS_ERROR)) == 0U,
                   44);

    minemu_trace_event(UINT32_C(0x20030001));
    minemu_fail_stop();
}
