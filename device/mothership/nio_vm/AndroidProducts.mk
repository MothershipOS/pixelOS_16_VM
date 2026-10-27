# mothershipOS: Android guest for the Motorola nio host (KVM at EL2).
PRODUCT_MAKEFILES := \
    $(LOCAL_DIR)/custom_nio_vm.mk

COMMON_LUNCH_CHOICES := \
    custom_nio_vm-bp4a-userdebug
