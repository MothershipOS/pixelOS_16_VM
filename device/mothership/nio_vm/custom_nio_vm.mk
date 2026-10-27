#
# mothershipOS nio VM: PixelOS (with GMS) on the Cuttlefish arm64 virtual device.
#
# The guest is a pure virtual device (virtio GPU/input/net/sound/block, vsock); all real nio hardware stays
# on the mothershipOS host and is offered to this guest through virtio devices or host services over vsock.
# Vendor side = Cuttlefish vsoc_arm64 (device_vendor.mk + bootloader); system/product = PixelOS. Not inherited
# from Cuttlefish: aosp_product.mk, generic_system.mk and PRODUCT_IGNORE_ALL_ANDROIDMK (GMS uses Android.mk).
#
# System side composed like a regular PixelOS device (not the Cuttlefish generic_system.mk, whose artifact
# path requirements forbid the PixelOS GMS / face unlock files in /system).
$(call inherit-product, $(SRC_TARGET_DIR)/product/core_64_bit.mk)
$(call inherit-product, $(SRC_TARGET_DIR)/product/full_base_telephony.mk)

# PixelOS (LineageOS base + Pixel GMS)
TARGET_SCREEN_WIDTH := 1080
TARGET_SCREEN_HEIGHT := 2520
TARGET_DISABLE_EPPE := true
$(call inherit-product, vendor/custom/config/common_full_phone.mk)

# Battery (mship.battery): AOSP's health HAL, which reads the goldfish battery that the host keeps current with the
# phone's real battery. Cuttlefish's own health HAL reports fixed values (85 %, charging). Set before
# device_vendor.mk, which only picks its default when this is empty.
LOCAL_HEALTH_PRODUCT_PACKAGE := \
    android.hardware.health-service.example \
    android.hardware.health-service.example_recovery

# Cuttlefish virtual hardware (vendor side)
# Telephony (mship.modem): Cuttlefish's stock rild and RIL, which talk AT over vsock to the host's modem slot (port
# androidboot.modem_simulator_ports); the host passes them to the phone's real modem. An empty slot = radio
# unavailable (the RIL keeps waiting for answers; IRadio is registered either way).
# Cameras (mship.camera): the host's real cameras through the external camera HAL (device.mk), no emulated ones
TARGET_CF_NO_EMULATED_CAMERA := true
$(call inherit-product, device/google/cuttlefish/shared/phone/device_vendor.mk)
$(call inherit-product, packages/modules/Virtualization/apex/product_packages.mk)
$(call inherit-product, device/google/cuttlefish/vsoc_arm64/bootloader.mk)

# mothershipOS layer
$(call inherit-product, device/mothership/nio_vm/device.mk)

PRODUCT_NAME := custom_nio_vm
PRODUCT_DEVICE := nio_vm
PRODUCT_BRAND := mothership
PRODUCT_MANUFACTURER := mothershipOS
PRODUCT_MODEL := nio VM
PRODUCT_MAX_PAGE_SIZE_SUPPORTED := 16384

PRODUCT_VENDOR_PROPERTIES += \
    ro.soc.manufacturer=$(PRODUCT_MANUFACTURER) \
    ro.soc.model=$(PRODUCT_DEVICE)

# adb trusts the mothershipOS host (root@nio ~/.android/adbkey.pub): adb connect vsock:3:5555, no on-screen prompt
PRODUCT_ADB_KEYS := device/mothership/nio_vm/adb_keys
