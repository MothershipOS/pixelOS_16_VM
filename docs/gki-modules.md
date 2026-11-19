# Building guest kernel modules for a stock Android GKI kernel

Guest drivers (mothershipOS's `guest/drivers`: `mship_wifi`, `virtio_media`, `virtio_gpu`) must load on the unmodified GKI kernel the guest image ships (PixelOS 16:
`6.12.38-android16-5-g501f58f107ab-ab13974591-4k`). Done on the build machine (the buildfarm) in `~/mothership-build/gki`:

1. Source: `kernel/common` branch `android16-6.12` at the commit in the kernel's version string (`501f58f10`);
   Cuttlefish's `kernel/common-modules/virtual-device` for the module fragments.
2. Compiler: exactly the one in `/proc/version` (`clang-r536225`, Android build 12833971), fetched from
   `platform/prebuilts/clang/host/linux-x86`. kCFI and config depend on it.
3. Config: the guest's own `/proc/config.gz` + the Wi-Fi lines of the virtual-device fragments, `olddefconfig`
   with Rust 1.88, bindgen and pahole from the AOSP prebuilts on PATH (so `CONFIG_RUST`, BTF, `SCHED_CLASS_EXT` and
   `CFI_ICALL_NORMALIZE_INTEGERS` match). Diff against the guest config must show only module-only and toolchain-probe
   options.
4. `modules_prepare` in the `mothership-android-build:24.04` container (the toolchain from the AOSP prebuilts on
   PATH, as `cargo xtask gki-module` runs it; needs `libdw-dev` installed as root in the container). An empty
   `common/protected_module_names_list` is needed by modpost.
5. `Module.symvers`: **not** from the shipped `vmlinux` (it belongs to the `-allsyms` build and its CRCs differ from
   the trimmed kernel). `cargo xtask gki-symvers` reads the export and CRC tables out of the bootable `kernel-6.12`
   Image and verifies them against every shipped module's expected CRCs (must report 0 mismatches);
   `cargo xtask gki-symvers-elf` adds the CRCs of vendor modules such as `cfg80211.ko`.
6. `cargo xtask gki-module <gki dir> <aosp dir> <driver>` (`make O=../out-vd ARCH=arm64 LLVM=1 M=<driver>
   modules`). modpost fails if the driver uses a symbol the real kernel doesn't export.

The vermagic shows the plain release; with modversions only the flag part is compared, which matches.
