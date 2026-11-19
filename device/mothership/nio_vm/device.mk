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

# Audio (mship.audio): the guest's virtio-snd card has one PCM per phone audio device and the headset jack; the
# stock AIDL HAL and Cuttlefish's policy know these devices (pixelOS_16_VM/patches: hardware/interfaces 0002,
# device/google/cuttlefish 0004), the framework connects wired devices on the jack's input switches (overlay)
DEVICE_PACKAGE_OVERLAYS += device/mothership/nio_vm/overlay

# Cameras (mship.camera): the host's cameras are V4L2 capture devices (virtio-media), used by AOSP's external camera
# HAL (patched for their BGRx frames: pixelOS_16_VM/patches/hardware/interfaces). Cuttlefish's emulated cameras are
# left out (TARGET_CF_NO_EMULATED_CAMERA in custom_nio_vm.mk).
PRODUCT_PACKAGES += android.hardware.camera.provider-V1-external-service
DEVICE_MANIFEST_FILE += device/mothership/nio_vm/camera/manifest.xml
PRODUCT_COPY_FILES += \
    device/mothership/nio_vm/camera/external_camera_config.xml:$(TARGET_COPY_OUT_VENDOR)/etc/external_camera_config.xml \
    frameworks/native/data/etc/android.hardware.camera.external.xml:$(TARGET_COPY_OUT_VENDOR)/etc/permissions/android.hardware.camera.external.xml

# Hardware video decode (mship.video): the host's Venus decoder is a V4L2 stateful decoder here (virtio-media);
# external/v4l2_codec2 (patched: MMAP bitstream buffers, decoded frames straight into gralloc buffers as virtio
# objects) serves it as c2.v4l2.* Codec2 components over the AIDL HAL (media/)
PRODUCT_SOONG_NAMESPACES += external/v4l2_codec2 device/mothership/nio_vm/media
PRODUCT_PACKAGES += android.hardware.media.c2-service.mship-v4l2
PRODUCT_COPY_FILES += \
    device/mothership/nio_vm/media/media_codecs_c2.xml:$(TARGET_COPY_OUT_VENDOR)/etc/media_codecs_c2.xml \
    device/mothership/nio_vm/media/android.hardware.media.c2-extended-seccomp_policy:$(TARGET_COPY_OUT_VENDOR)/etc/seccomp_policy/android.hardware.media.c2-extended-seccomp_policy
PRODUCT_VENDOR_PROPERTIES += \
    ro.vendor.v4l2_codec2.decode_concurrent_instances=8 \
    ro.vendor.v4l2_codec2.initial_output=false
