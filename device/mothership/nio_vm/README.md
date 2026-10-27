# device/mothership/nio_vm

Android guest product for **mothershipOS** on the Motorola nio (SM8250). PixelOS (LineageOS + Pixel GMS) on the
Cuttlefish `vsoc_arm64` virtual device, run under KVM by the mothershipOS host.

    lunch custom_nio_vm-bp4a-userdebug && m      # build inside the mothership-android-build container

## Design
- The image is meant to be generic (one image for every mothershipOS phone); the name `nio_vm` predates that and
  should change. Real hardware reaches the guest through mothershipOS hardware handles at the lowest generic
  Linux level (`mothershipOS/docs/hardware-handles.md`): virtio devices where they exist, small generic guest adapters
  (e.g. a cfg80211 proxy for Wi-Fi) where they don't. No phone-specific drivers or firmware in the guest.

## Status (2026-10-12)
- [x] product skeleton (GMS, Cuttlefish arm64 virtual hardware), builds on buildfarm
- [x] boots under crosvm on the nio: gfxstream GPU, panel via DRM lease + zero copy (perf needs work)
- [x] touch, volume keys, power key (Android sleeps/wakes and drives the panel)
- [x] shared-internet Wi-Fi (virt_wifi over NAT)
- [x] Bluetooth (the host's controller over hvc5, mship.bt)
- [x] battery from the host (goldfish battery fed by mship-battery, AOSP health HAL)
- [~] cameras: the host's cameras as V4L2 devices via the external camera HAL (mship.camera): four cameras listed, capture not verified yet
- [~] modem: rild talks AT to the phone's modem (mship.modem); SIM, registration and data need a SIM in
- [ ] native Wi-Fi, audio, sensors, GNSS
- [ ] generic product name, guest perf tweaks (blur, region sampling)

Details: `pixelOS_16_VM/docs/android-vm.md`.
