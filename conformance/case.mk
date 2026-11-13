# Builds one conformance case: every *.c and *.S in the case directory is
# linked with the conformance runtime, packaged as a system image, and run
# with `minemu test test.toml`. Nothing here depends on the student template.

ARM_PREFIX ?= arm-none-eabi-
CC := $(ARM_PREFIX)gcc
MINEMU ?= minemu

CONFORMANCE_DIR := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
RUNTIME_DIR := $(CONFORMANCE_DIR)/runtime
RUNTIME := $(RUNTIME_DIR)/build/libconformance.a
RUNTIME_INPUTS := $(wildcard $(RUNTIME_DIR)/src/*) \
	$(wildcard $(RUNTIME_DIR)/include/minemu/*.h) $(RUNTIME_DIR)/Makefile
LINKER_SCRIPT := $(RUNTIME_DIR)/kernel.ld

PROGRAM ?= $(notdir $(CURDIR))
BUILD := build
ELF := $(BUILD)/$(PROGRAM).elf
IMAGE := $(BUILD)/$(PROGRAM).img
OBJECTS := $(patsubst %.c,$(BUILD)/%.o,$(wildcard *.c)) \
	$(patsubst %.S,$(BUILD)/%.o,$(wildcard *.S))
DEPENDENCIES := $(OBJECTS:.o=.d)

ARCH_FLAGS := -mcpu=cortex-a9 -marm -mfloat-abi=soft
CPPFLAGS := -I$(RUNTIME_DIR)/include
CFLAGS := $(ARCH_FLAGS) -ffreestanding -fno-builtin -fdata-sections \
	-ffunction-sections -fno-stack-protector -std=c11 -Wall -Wextra -Werror -MMD -MP
ASFLAGS := $(ARCH_FLAGS) -ffreestanding -MMD -MP
LDFLAGS := $(ARCH_FLAGS) -nostdlib -Wl,-T,$(LINKER_SCRIPT) \
	-Wl,--gc-sections -Wl,-u,minemu_bootstrap -Wl,-Map,$(BUILD)/$(PROGRAM).map
LIBGCC := $(shell $(CC) $(ARCH_FLAGS) -print-libgcc-file-name)

.PHONY: all image test prepare clean

all: $(ELF)

image: $(IMAGE)

prepare:

test: prepare $(IMAGE) test.toml
	$(MINEMU) test test.toml

$(RUNTIME): $(RUNTIME_INPUTS)
	$(MAKE) -C $(RUNTIME_DIR)

$(ELF): $(OBJECTS) $(RUNTIME) $(LINKER_SCRIPT)
	@mkdir -p $(dir $@)
	$(CC) $(LDFLAGS) $(OBJECTS) $(RUNTIME) $(LIBGCC) -o $@

$(IMAGE): image.toml $(ELF)
	$(MINEMU) image image.toml --output $@

$(BUILD)/%.o: %.c
	@mkdir -p $(dir $@)
	$(CC) $(CPPFLAGS) $(CFLAGS) -c $< -o $@

$(BUILD)/%.o: %.S
	@mkdir -p $(dir $@)
	$(CC) $(CPPFLAGS) $(ASFLAGS) -c $< -o $@

clean:
	rm -rf $(BUILD)

-include $(DEPENDENCIES)
