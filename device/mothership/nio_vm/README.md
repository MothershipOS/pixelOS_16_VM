# device/mothership/nio_vm

Android guest product for **mothershipOS** on the Motorola nio (SM8250). PixelOS (LineageOS + Pixel GMS) on the
Cuttlefish `vsoc_arm64` virtual device, run under KVM by the mothershipOS host.

    lunch custom_nio_vm-bp4a-userdebug && m      # build inside the mothership-android-build container

## Status
**Works**
- Product (GMS, Cuttlefish arm64 virtual hardware), builds on the buildfarm, boots under crosvm on the nio.
- GPU: virtio-gpu DRM native context (guest Mesa freedreno/turnip); panel via DRM lease, zero copy.
- Touch, volume keys, power key (Android sleeps/wakes and drives the panel).
- Wi-Fi: native per band (mship.wifi) and shared internet (virt_wifi over NAT).
- Bluetooth: the host's controller over hvc5 (mship.bt).
- Battery from the host (goldfish battery fed by mship-battery, AOSP health HAL).
- Modem: rild talks AT to the phone's modem (mship.modem): registration, mobile data, SMS, call signalling.
- Audio: the phone's speaker, earpiece, headset and mics (mship.audio) through the stock virtio-snd driver and
  audio HAL.
- GNSS: the phone's receiver (mship.gnss), NMEA on /dev/gnss0 (hvc6), our HAL `android.hardware.gnss-service.mship`
  (`gnss/`); satellites and NMEA reach Android.
- Sensors: the phone's accelerometer, gyroscope, magnetometer, proximity, light (mship.sensors) through
  Cuttlefish's goldfish sensors sub-HAL under AOSP's multi-HAL on hvc13.
- Hot plug: handles can be lent or taken while PixelOS runs.
- Cameras: the host's cameras (mship.camera) through the external camera HAL: ids 0-3 (`camera/`), facing and
  orientation and modes (sizes, rates) from the host, 720p/1080p/2160p recording profiles (`camera/media_profiles.xml` to odm), Aperture with the
  ultrawides (overlay); preview, photos and video work.

**Needs work**
- Cameras: 1080p recording at about 12 fps (software encoder), no camera controls.
- GNSS: no position fix verified yet.
- Sensors: "Goldfish ..." names and fixed ranges; the first proximity value after boot is a placeholder for ~50 ms.
- Generic product name; guest perf tweaks (region sampling).

**Broken**
- Wi-Fi while the phone's QCA6390 firmware has crashed (host-side, until reboot).

**Not implemented**
- Call audio; sensors: step counter, significant motion, tilt, gestures.

## Design
- The image is meant to be generic (one image for every mothershipOS phone); the name `nio_vm` is phone-specific
  and should change. Real hardware reaches the guest through mothershipOS hardware handles at the lowest generic
  Linux level (`mothershipOS/docs/hardware-handles.md`): virtio devices where they exist, small generic guest adapters
  (e.g. a cfg80211 proxy for Wi-Fi) where they don't. No phone-specific drivers or firmware in the guest.
- `custom_nio_vm.mk` selects the GNSS HAL (`LOCAL_GNSS_PRODUCT_PACKAGE := android.hardware.gnss-service.mship`,
  instead of Cuttlefish's mock HAL) and the sensors HAL (`LOCAL_SENSOR_PRODUCT_PACKAGE`: multi-HAL +
  `android.hardware.sensors@2.1-impl.cuttlefish`, instead of AOSP's example HAL with fake sensors); declared sensor
  features: accelerometer, compass, gyroscope, light, proximity.

Details: `pixelOS_16_VM/docs/android-vm.md`.
