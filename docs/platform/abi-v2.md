# Platform ABI Version 2

> **Status: Normative.** This document and the linked component specifications
> define the current guest-visible `minemu` platform contract.

Platform ABI v2 is an additive device revision. It preserves the CPU, reset,
physical map, virtual-time, exception/MMU, boot-info, and serialized system-image
contracts from Platform ABI v1. Existing ABI-v1 guests remain valid and continue
to use block unit 0 because the new `UNIT` register resets to zero.

| Specification | Contract |
|---|---|
| [Platform ABI v1](abi-v1.md) | Unchanged CPU, terminology, reset, physical map, and virtual-time requirements |
| [Boot ABI v1](boot-v1.md) | Reset firmware, boot locations, kernel handoff, and boot info |
| [System-image format v1](system-image-v1.md) | Serialized image records and validity |
| [Exceptions and MMU ABI v1](exceptions-and-mmu-v1.md) | Exception entry, CP15, translation, and faults |
| [Device ABI v2](devices-v2.md) | Device ABI v1 plus the two-unit block-controller revision |

**MINEMU-ABI2-SCOPE-001:** Platform ABI v2 MUST satisfy every Platform ABI v1
requirement except where Device ABI v2 explicitly replaces a Device ABI v1
requirement.

**MINEMU-ABI2-SCOPE-002:** The Boot ABI and system-image format version fields
remain `1`. Platform ABI v2 does not change either serialized wire format.

**MINEMU-ABI2-SCOPE-003:** Versioned platform documents take precedence over
implementation comments, host-tool documentation, and informative guides.
