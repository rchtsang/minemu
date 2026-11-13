#include "minemu/boot.h"
#include "minemu/trap.h"
#include "minemu/trace.h"

static volatile uint32_t initialized_data = UINT32_C(0x13579bdf);
static volatile uint32_t zeroed_bss[32];

void minemu_kernel_main(const struct minemu_boot_info *boot_info) {
    if ((uintptr_t)boot_info != MINEMU_BOOT_INFO_VADDR ||
        boot_info->magic != MINEMU_BOOT_INFO_MAGIC ||
        boot_info->version != MINEMU_ABI_VERSION ||
        boot_info->size != sizeof(*boot_info) ||
        boot_info->system_rom_base != UINT32_C(0x08000000) ||
        boot_info->image_size == 0U || boot_info->module_count != 1U ||
        boot_info->module_table_offset == 0U ||
        boot_info->direct_map_vaddr != UINT32_C(0xc0000000) ||
        boot_info->direct_map_paddr != UINT32_C(0x40000000) ||
        boot_info->direct_map_size != UINT32_C(0x04000000) ||
        initialized_data != UINT32_C(0x13579bdf)) {
        minemu_trace_event(UINT32_C(0xb007bad0));
        minemu_fail_stop();
    }
    for (size_t i = 0; i < sizeof(zeroed_bss) / sizeof(zeroed_bss[0]); ++i) {
        if (zeroed_bss[i] != 0U) {
            minemu_trace_event(UINT32_C(0xb007bad1));
            minemu_fail_stop();
        }
    }
    minemu_trace_event(UINT32_C(0x20000001));
    minemu_fail_stop();
}
