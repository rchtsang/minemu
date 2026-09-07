# Supplied Kernel Runtime

> **Status: Informative.** This documents the exception stubs shipped by the
> `minimum-template`. The machine contract ends at the architectural state
> transition in
> [Exceptions and MMU ABI v1](../platform/exceptions-and-mmu-v1.md).

## Vector Stubs

The supplied vector table places eight A32 branch instructions at VBAR. Reset
and FIQ route to unsupported handlers; UND, SVC, prefetch abort, data abort, and
IRQ route to assembly stubs.

The synchronous stubs pass these template dispatcher IDs:

| Exception | Dispatcher ID |
|---|---:|
| Undefined instruction | `-4` |
| Supervisor call | `-3` |
| Prefetch abort | `-2` |
| Data abort | `-1` |

These numbers are a convention between the supplied assembly and C runtime,
not values written by the machine.

## Trap Frame

The supplied stubs save a runtime-defined frame containing general registers,
the exception LR, and SPSR, then call C hooks. Its byte layout and hook names
are private to the template and may change without a platform ABI revision.
Student kernels may replace the frame and dispatch code entirely.

## IRQ Flow

The supplied IRQ handler follows this policy:

1. Read the interrupt controller `CLAIM` register.
2. Dispatch the returned source ID to the corresponding driver.
3. Acknowledge the peripheral so its level can deassert.
4. Write the same source ID to controller `EOI`.
5. Restore the saved context and return using the architectural IRQ LR offset.

The required claim/EOI and peripheral-level semantics are normative; this
handler sequencing and C dispatch organization are reference implementation
choices.
