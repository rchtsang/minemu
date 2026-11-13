PROGRAM := block-unattached

prepare:
	@mkdir -p build
	@rm -f build/filesystem.img
	@truncate -s 512 build/filesystem.img

include ../case.mk
