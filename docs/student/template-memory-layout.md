# Template Memory Layout

> **Status: Informative.** This describes the supplied `minimum-template`, not a
> platform requirement. Alternative kernels may choose different page-table,
> stack, and temporary-mapping locations while satisfying the normative
> [platform specifications](../platform/abi-v1.md).

## Bootstrap Tables

The reference linker script and bootstrap use this physical RAM layout:

| Half-open PA range | Purpose |
|---|---|
| `[0x4000_6000, 0x4000_7000)` | Firmware stack workspace reserved by the boot ABI |
| `[0x4000_7000, 0x4000_7040)` | Boot-info record reserved by the boot ABI |
| `0x4000_8000` | Required physical bootstrap entry |
| `[0x4000_a000, 0x4000_b000)` | Initial page directory |
| `[0x4000_b000, 0x4000_c000)` | Identity-map page table |
| `[0x4000_c000, 0x4000_d000)` | Initial MMIO page table |
| `[0x4000_d000, 0x4001_1000)` | Sixteen RAM direct-map page tables |

The bootstrap identity-maps enough low RAM to survive enabling translation,
maps the platform MMIO pages for early drivers, and installs the complete
64-MiB high-half RAM direct map. It then switches to a high-half stack and
branches to the kernel's high VA entry point.

Only the resulting direct map and the ordering required by the boot/MMU ABI are
normative. The table locations and temporary mappings above are replaceable.

## Kernel Stacks

The supplied runtime allocates downward-growing 4-KiB stacks for SVC, IRQ, ABT,
and UND modes at template-selected high virtual addresses. Their placement and
size are implementation policy. Kernels may replace them, provided valid
banked SP values exist before the corresponding exception can be delivered.

## Build-Derived Addresses

The template build derives its physical bootstrap address and high kernel
entry from linker symbols and packages them into the system image. Do not infer
platform-wide section ordering or page-table locations from the reference
linker script.
