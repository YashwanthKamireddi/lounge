#!/bin/sh
set -eu

# Use the webOS NDK's ARM soft-float ABI and sysroot for Rust AND SDL linking.
: "${WEBOS_NDK:?Set WEBOS_NDK to the extracted and relocated webOS native toolchain}"
ROOT=$(dirname "$(dirname "$(realpath "$0")")")
WEBOS_NDK=$(realpath "$WEBOS_NDK")
CC="$WEBOS_NDK/bin/arm-webos-linux-gnueabi-gcc"
[ -x "$CC" ] || { printf '%s\n' "Missing webOS compiler: $CC" >&2; exit 1; }
SYSROOT=$("$CC" -print-sysroot)
# Some relocated macOS SDKs report a nonexistent compiler-relative sysroot.
if [ ! -f "$SYSROOT/usr/include/stdint.h" ]; then
  SYSROOT="$WEBOS_NDK/arm-webos-linux-gnueabi/sysroot"
fi
[ -f "$SYSROOT/usr/include/stdint.h" ] || { printf '%s\n' "Missing webOS sysroot headers: $SYSROOT" >&2; exit 1; }
node "$ROOT/tools/webos-runtime.mjs"
export CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABI_LINKER="$CC"
# The target's soft-float *calling convention* is required by webOS, but its
# default software FP codegen is not: hardware FP is used while retaining the
# gnueabi ABI. Code is tuned for this TV's CPU (MT5889: four Cortex-A73 cores,
# NEON); WEBOS_CPU=cortex-a9 builds the portable package, NEON off, that runs
# on any webOS TV.
WEBOS_CPU="${WEBOS_CPU:-cortex-a73}"
if [ "$WEBOS_CPU" = cortex-a9 ]; then WEBOS_NEON=-neon; else WEBOS_NEON=+neon; fi
export CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABI_RUSTFLAGS="${CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABI_RUSTFLAGS:-} -C target-cpu=$WEBOS_CPU -C target-feature=$WEBOS_NEON,-soft-float -C link-arg=--sysroot=$SYSROOT -C link-arg=-Wl,-rpath,\$ORIGIN/lib"
# As the TV's Home the executable runs mounted at /usr/bin, where $ORIGIN/lib does not exist
# and LG's own older SDL would load instead: the installed package's lib comes next.
while IFS= read -r directory; do
  [ -n "$directory" ] || continue
  CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABI_RUSTFLAGS="$CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABI_RUSTFLAGS -C link-arg=-Wl,-rpath,$directory/lib"
done < "$ROOT/tv/install-directories.txt"
export CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABI_RUSTFLAGS
export CC_armv7_unknown_linux_gnueabi="$CC"
export AR_armv7_unknown_linux_gnueabi="$WEBOS_NDK/bin/arm-webos-linux-gnueabi-ar"
export CFLAGS_armv7_unknown_linux_gnueabi="${CFLAGS_armv7_unknown_linux_gnueabi:-} --sysroot=$SYSROOT"
export PKG_CONFIG_ALLOW_CROSS=1
# Nix's desktop SDL pkg-config path must not leak into this ARM build.
export PKG_CONFIG_PATH=
# This metadata contains absolute paths to the verified cross-built SDL runtime,
# so pkg-config must not prepend the target sysroot to those build-host paths.
export PKG_CONFIG_SYSROOT_DIR=/
export PKG_CONFIG_LIBDIR="$ROOT/.build/sdl-webos/pkgconfig"
exec cargo build --locked --manifest-path "$ROOT/Cargo.toml" --package lounge --target armv7-unknown-linux-gnueabi --features webos --release
