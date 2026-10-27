#
# mothershipOS guest layer (vendor side). Each piece of real nio hardware is owned by the host; this layer
# holds the guest adapters (HALs) that talk to the matching host service:
#   standard virtio  : display/GPU (virtio-gpu), touch/buttons (virtio-input), net, sound, USB  -> no HAL needed
#   host services    : modem (RIL), sensors, GNSS, battery/charger, power hints -> HALs over vsock
#   cameras          : V4L2 capture devices (virtio-media) -> AOSP's external camera HAL
# Nothing here is hardware specific yet; HALs are added one by one (see README.md).
#
PRODUCT_VENDOR_PROPERTIES += \
    ro.mothership.vm=1 \
    ro.mothership.host=motorola_nio

# nio panel: 1080x2520, ~410 dpi (the host runs the guest display 1:1 on DSI-1)
PRODUCT_VENDOR_PROPERTIES += \
    ro.sf.lcd_density=420

# GPU: virtio-gpu DRM native context with Mesa freedreno/turnip (selected by the host: androidboot.hardware.egl=mesa,
# androidboot.hardware.vulkan=freedreno); gfxstream stays installed as the fallback
PRODUCT_PACKAGES += libEGL_mesa libGLESv2_mesa libGLESv1_CM_mesa libgallium_dri vulkan.freedreno
# minigbm gralloc0 module: Mesa reads buffer layouts (fds, strides, offsets, modifier) through its CrOS perform
# op (Mesa is built with the NDK: no libui, so no IMapper5 backend)
PRODUCT_PACKAGES += gralloc.minigbm

# Cameras (mship.camera): the host's cameras are V4L2 capture devices (virtio-media), used by AOSP's external camera
# HAL (patched for their BGRx frames: pixelOS_16_VM/patches/hardware/interfaces). Cuttlefish's emulated cameras are
# left out (TARGET_CF_NO_EMULATED_CAMERA in custom_nio_vm.mk).
PRODUCT_PACKAGES += android.hardware.camera.provider-V1-external-service
DEVICE_MANIFEST_FILE += device/mothership/nio_vm/camera/manifest.xml
PRODUCT_COPY_FILES += \
    device/mothership/nio_vm/camera/external_camera_config.xml:$(TARGET_COPY_OUT_VENDOR)/etc/external_camera_config.xml \
    frameworks/native/data/etc/android.hardware.camera.external.xml:$(TARGET_COPY_OUT_VENDOR)/etc/permissions/android.hardware.camera.external.xml
