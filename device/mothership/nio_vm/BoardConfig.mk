#
# mothershipOS nio_vm board: the Cuttlefish vsoc_arm64 board plus mothershipOS overrides.
# Cuttlefish splits its board config per subsystem (shared/<camera|gnss|sensors|telephony|...>/BoardConfig.mk);
# those are the pieces the mothershipOS layer replaces as real-hardware host services come online.
#
include device/google/cuttlefish/vsoc_arm64/BoardConfig.mk

# Kernel: keep Cuttlefish's prebuilt GKI kernel handling; skip the LineageOS build-kernel-from-source logic
# (vendor/lineage/build/tasks/kernel.mk wants BOARD_KERNEL_IMAGE_NAME + TARGET_KERNEL_SOURCE).
TARGET_NO_KERNEL_OVERRIDE := true
BOARD_VENDOR_SEPOLICY_DIRS += device/mothership/nio_vm/sepolicy
