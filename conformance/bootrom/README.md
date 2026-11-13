# Reference Boot ROM

This is the conformance suite's reset firmware: the reference implementation of
[Boot ABI v1](../../docs/platform/boot-v1.md). Every case boots through it. It
began as a copy of the `minimum-template` Boot ROM but is owned here and does
not track the template; change it only when the Boot ABI changes.

Rebuild it and compare against the canonical binary with:

```sh
make -C conformance/bootrom check
```

After an intentional change, `make -C conformance/bootrom update` replaces
`bootloader.bin`.
