# Module Format and Loading

> **Status: Informative.** The serialized module records are normative in
> [System-image format v1](../platform/system-image-v1.md). Frame allocation,
> address-space construction, process objects, and scheduling are kernel policy.

## Packaged Metadata

Each module has a UTF-8 name, an A32 entry VA, and fixed-VA load segments. Each
segment supplies initialized bytes, an in-memory size, and readable, writable,
and executable flags. No module record assigns a physical address or serializes
user accessibility; the kernel adds `MINEMU_PTE_USER` when its address-space
policy maps the segment for user execution.

## Typical Loader

A kernel loader can:

1. Locate the module table through the boot-info record and System ROM PA.
2. Validate or trust the host-validated module metadata according to its threat
   model.
3. Allocate one physical frame for each covered module VA page.
4. Copy each segment's initialized bytes from its System ROM offset.
5. Zero the remainder through `memory_size`.
6. Install PTE permissions derived from the segment flags.
7. Execute TLBIALL before relying on changed active mappings.
8. Initialize a user context at the module entry VA.

The reference runtime exposes helper hooks for these steps, but their names and
replacement algorithm are not platform ABI.

## Address Policy

The image format forbids module segments from overlapping the kernel direct
map, but it does not reserve a universal user stack, heap, IPC, or shared-memory
layout. A kernel must define those regions and reject conflicts with packaged
module segments.
