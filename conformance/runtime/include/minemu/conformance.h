#ifndef MINEMU_CONFORMANCE_H
#define MINEMU_CONFORMANCE_H

#include <stdint.h>

#include "minemu/trace.h"
#include "minemu/trap.h"

static inline void minemu_conformance_fail(uint32_t code) {
    minemu_trace_event(UINT32_C(0xf0000000) | (code & UINT32_C(0x0fffffff)));
    minemu_fail_stop();
}

#define MINEMU_REQUIRE(condition, code) \
    do { \
        if (!(condition)) { \
            minemu_conformance_fail(code); \
        } \
    } while (0)

static inline uint32_t minemu_test_paddr(const volatile void *address) {
    return (uint32_t)(uintptr_t)address - UINT32_C(0x80000000);
}

#endif
