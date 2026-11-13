# Conformance-owned kernel runtime: startup, vectors, and minimal helpers.
# It implements only the platform ABI (docs/platform/) and deliberately does
# not track the student template, so OS refactors cannot break conformance.

ARM_PREFIX ?= arm-none-eabi-
CC := $(ARM_PREFIX)gcc
AR := $(ARM_PREFIX)ar

BUILD := build
LIBRARY := $(BUILD)/libconformance.a
OBJECTS := $(patsubst src/%.c,$(BUILD)/%.o,$(wildcard src/*.c)) \
	$(patsubst src/%.S,$(BUILD)/%.o,$(wildcard src/*.S))
DEPENDENCIES := $(OBJECTS:.o=.d)

ARCH_FLAGS := -mcpu=cortex-a9 -marm -mfloat-abi=soft
CPPFLAGS := -Iinclude
CFLAGS := $(ARCH_FLAGS) -ffreestanding -fno-builtin -fdata-sections \
	-ffunction-sections -fno-stack-protector -std=c11 -Wall -Wextra -Werror -MMD -MP
ASFLAGS := $(ARCH_FLAGS) -ffreestanding -MMD -MP

.PHONY: all clean

all: $(LIBRARY)

$(LIBRARY): $(OBJECTS)
	@mkdir -p $(dir $@)
	$(AR) rcs $@ $^

$(BUILD)/%.o: src/%.c
	@mkdir -p $(dir $@)
	$(CC) $(CPPFLAGS) $(CFLAGS) -c $< -o $@

$(BUILD)/%.o: src/%.S
	@mkdir -p $(dir $@)
	$(CC) $(CPPFLAGS) $(ASFLAGS) -c $< -o $@

clean:
	rm -rf $(BUILD)

-include $(DEPENDENCIES)
