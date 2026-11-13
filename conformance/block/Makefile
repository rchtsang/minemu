PROGRAM := block

prepare:
	@mkdir -p build
	@rm -f build/filesystem.img build/swap.img
	@truncate -s 1024 build/filesystem.img
	@truncate -s 1024 build/swap.img

include ../case.mk
