PROGRAM := baseline

include ../case.mk

USER_ELF := $(BUILD)/user/minimum-user.elf

prepare: $(USER_ELF)

$(IMAGE): $(USER_ELF)

$(USER_ELF): user/user.S user/user.ld
	@mkdir -p $(dir $@)
	$(CC) $(ARCH_FLAGS) -nostdlib -Wl,-T,user/user.ld user/user.S -o $@
