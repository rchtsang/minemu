MINEMU ?= minemu

CASES := baseline uart interrupts block block-unattached rng-trace mmu exceptions ttbr-switch
IMAGE_TARGETS := $(addprefix image-,$(CASES))
TEST_TARGETS := $(addprefix test-,$(CASES))

# Cases share the runtime library; build it once and run cases in order.
.NOTPARALLEL:
.PHONY: images test clean bootrom-check runtime $(IMAGE_TARGETS) $(TEST_TARGETS)

images: $(IMAGE_TARGETS)

test: $(TEST_TARGETS)

bootrom-check:
	$(MAKE) -C bootrom check

runtime:
	$(MAKE) -C runtime

$(IMAGE_TARGETS) $(TEST_TARGETS): bootrom-check runtime

$(IMAGE_TARGETS): image-%:
	$(MAKE) -C $* MINEMU="$(MINEMU)" image

$(TEST_TARGETS): test-%:
	$(MAKE) -C $* MINEMU="$(MINEMU)" test

clean:
	$(MAKE) -C bootrom clean
	$(MAKE) -C runtime clean
	@for case in $(CASES); do $(MAKE) -C $$case clean; done
