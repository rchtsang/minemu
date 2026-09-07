# Exceptions and MMU ABI Version 1

> **Status: Normative.** This document defines machine exception entry, the
> supported CP15 subset, translation, page-table entries, and fault reporting.

## Machine Exception Entry

Exception vector offsets are relative to VBAR.

| Exception | Vector offset | Destination mode | Banked LR value |
|---|---:|---|---|
| Undefined instruction | `0x04` | UND (`0x1b`) | exception PC + `4` |
| Supervisor call | `0x08` | SVC (`0x13`) | exception PC + `4` |
| Prefetch abort | `0x0c` | ABT (`0x17`) | exception PC + `4` |
| Data abort | `0x10` | ABT (`0x17`) | exception PC + `8` |
| IRQ | `0x18` | IRQ (`0x12`) | boundary PC + `4` |

For SVC and undefined instruction, the exception PC is the address of the
instruction. For an abort, it is the PC at the failed instruction boundary.
For IRQ, the boundary PC is the next instruction that would otherwise execute.

**MINEMU-EXC1-ENTRY-001:** On exception entry, the machine MUST perform this
state transition atomically at the exception-entry boundary:

1. Save the complete old CPSR in the destination mode's SPSR.
2. Switch to the destination mode listed above, selecting its banked SP and LR.
3. Clear `CPSR.T` and set `CPSR.I`; preserve all other CPSR bits.
4. Set the destination mode's LR to the listed value.
5. Set PC to `VBAR + vector offset`.

**MINEMU-EXC1-ENTRY-002:** Each exception entry MUST cost one tick in addition
to any cost assigned to the instruction by
[Platform ABI v1](abi-v1.md#virtual-time).

**MINEMU-EXC1-ENTRY-003:** IRQ entry MUST occur only at an instruction boundary
when `CPSR.I` is clear and an interrupt-controller source is claimable. The
source becomes the controller's active claim before vector execution begins.

**MINEMU-EXC1-ENTRY-004:** Version 1 defines no FIQ delivery. Reset vector
offset `0x00` is entered only through reset, not through this exception-entry
transition.

The machine does not push a stack frame, change SP, preserve general-purpose
registers, call C, or perform exception return. Vector stubs and the supplied
runtime's trap-frame convention are informative template behavior described in
[Supplied kernel runtime](../student/kernel-runtime.md).

## Supported CP15 Interface

The notation follows A32 `MCR`/`MRC` operands
`p15, opc1, Rt, CRn, CRm, opc2`.

| Operation | Direction and operands | Effect |
|---|---|---|
| TTBR0 | `MCR p15,0,Rt,c2,c0,0` | Set page-directory PA |
| SCTLR | `MCR p15,0,Rt,c1,c0,0` | Set translation enable from `Rt[0]` |
| TLBIALL | `MCR p15,0,Rt,c8,c7,0` | Invalidate all cached translations; `Rt` value ignored |
| VBAR | `MCR p15,0,Rt,c12,c0,0` | Set vector-base VA |
| DFSR | `MRC p15,0,Rt,c5,c0,0` | Read latest fault status, or zero before any fault |
| DFAR | `MRC p15,0,Rt,c6,c0,0` | Read latest fault VA, or zero before any fault |

**MINEMU-MMU1-CP15-001:** The table above is the complete supported CP15 subset.
Each operation is privileged and conditionally executed according to its A32
condition field. An executed unsupported operation, an executed CP15 operation
in USR mode, or an invalid TTBR0/VBAR value MUST cause an undefined-instruction
exception.

**MINEMU-MMU1-CP15-002:** TTBR0 MUST be 4-KiB aligned and its complete 4-KiB
page MUST lie in RAM. VBAR MUST be 32-byte aligned. SCTLR bits other than bit 0
are ignored and readback of SCTLR, TTBR0, and VBAR is not supported.

**MINEMU-MMU1-CP15-003:** Changing TTBR0, changing SCTLR.M, or writing page-table
memory MUST NOT be assumed to invalidate cached translations. Guest software
MUST execute TLBIALL after any such change before relying on it.

**MINEMU-MMU1-CP15-004:** The required initial enable sequence is: fully write
the page directory and page tables, write TTBR0, execute TLBIALL, then set
SCTLR.M. The required update sequence while enabled is: write all changed
entries, then execute TLBIALL before relying on the changes.

Version 1 defines translation and TLBIALL effects but no data or instruction
cache model. Cache-maintenance operations are unsupported and unnecessary.

## Translation

When SCTLR.M is clear, an effective address is used as the PA without a page
walk. When SCTLR.M is set, translation uses two 1024-entry little-endian tables:

```text
directory_index = VA[31:22]
table_index     = VA[21:12]
page_offset     = VA[11:0]
PDE_PA          = TTBR0 + 4 * directory_index
PTE_PA          = (PDE & 0xffff_f000) + 4 * table_index
result_PA       = (PTE & 0xffff_f000) | page_offset
```

**MINEMU-MMU1-WALK-001:** Page directories and page tables MUST each occupy one
4-KiB-aligned 4-KiB RAM page. All 1024 entries are 32-bit little-endian words.

**MINEMU-MMU1-WALK-002:** A valid PDE contains only bit 0 and bits `[31:12]`.
Bit 0 is Valid and bits `[31:12]` name the page-table PA. Bits `[11:1]` are
reserved and MUST be zero.

**MINEMU-MMU1-WALK-003:** A valid PTE uses this layout:

| Bits | Name | Meaning |
|---|---|---|
| `[31:12]` | Page PA | 4-KiB physical target page |
| `[11:7]` | Software | Kernel-owned metadata, ignored by translation |
| `6` | Dirty | Set by the MMU after a successful write |
| `5` | Accessed | Set by the MMU after any successful access |
| `4` | Readable | Read permission |
| `3` | Executable | Instruction-fetch permission |
| `2` | User | Permit USR access when the access-specific bit also permits it |
| `1` | Writable | Write permission |
| `0` | Valid | Entry participates in translation |

**MINEMU-MMU1-WALK-004:** A valid PTE target page MUST be a complete mapped
physical page. A PTE with User set MUST NOT target an MMIO page. Writes to Boot
ROM or System ROM MUST fault even when a PTE grants write permission.

**MINEMU-MMU1-WALK-005:** Supervisor modes require the access-specific Readable,
Writable, or Executable bit. USR mode requires both User and that
access-specific bit.

**MINEMU-MMU1-WALK-006:** After a successful translation, the machine MUST set
Accessed in the in-memory PTE and MUST additionally set Dirty for a write. A
failed access MUST NOT set either bit. Guest software owns clearing these bits
and replacement policy.

**MINEMU-MMU1-WALK-007:** Translation caches, if present, MUST preserve the
observable permissions and Accessed/Dirty behavior in this specification after
the guest uses the required TLBIALL ordering.

## Faults

| DFSR low-byte value | Cause |
|---:|---|
| `1` | Translation |
| `2` | Read protection |
| `3` | Write protection |
| `4` | Execute protection |
| `5` | Invalid device access |

DFSR also has bit 8 `FROM_USER`, bit 9 `IS_WRITE`, and bit 10 `IS_FETCH`.
All other DFSR bits are zero.

**MINEMU-MMU1-FAULT-001:** A missing/invalid PDE or PTE, inaccessible table
memory, invalid physical target, or failure to update Accessed/Dirty MUST report
a translation fault.

**MINEMU-MMU1-FAULT-002:** A denied read, write, or fetch MUST report the matching
protection cause. A User PTE targeting MMIO MUST report the matching protection
cause rather than a device-access cause.

**MINEMU-MMU1-FAULT-003:** An aligned translated access to an invalid MMIO
offset, wrong-direction register, or invalid register value MUST report cause 5.
DFAR MUST contain the faulting VA, and DFSR bits 8 through 10 MUST identify the
origin and access type.

**MINEMU-MMU1-FAULT-004:** A failed instruction fetch MUST enter Prefetch Abort.
A failed data read or write, including an invalid MMIO access, MUST enter Data
Abort. The faulting instruction itself costs zero ticks; exception entry costs
one tick.

**MINEMU-MMU1-FAULT-005:** DFSR and DFAR retain the most recently recorded fault
until another fault or reset. Reading them has no side effect.
