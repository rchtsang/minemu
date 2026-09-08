#!/bin/sh
set -eu

test "$(id -u)" -ne 0
test "$LANG" = "C.UTF-8"
test -f /opt/minemu/Cargo.toml

minemu --help >/dev/null
opencode --version >/dev/null
arm-none-eabi-gcc --version >/dev/null
arm-none-eabi-ar --version >/dev/null
arm-none-eabi-objcopy --version >/dev/null
arm-none-eabi-readelf --version >/dev/null
arm-none-eabi-nm --version >/dev/null
make --version >/dev/null
git --version >/dev/null
rg --version >/dev/null
