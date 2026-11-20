# PixelOS VM for mothershipOS

Android 16 (PixelOS, with Google apps) as a virtual machine on a phone running [mothershipOS](https://github.com/MothershipOS/mothershipOS/blob/main/README.md):
a Cuttlefish arm64 product (`custom_nio_vm`) built from the PixelOS tree, booted by crosvm. It has no phone drivers:
the phone's hardware reaches it through mothershipOS's contracts as generic virtual devices.

- **Works**: GPU and display, touch and keys, Wi-Fi, telephony (no call audio), Bluetooth, battery, USB, audio,
  hardware video decode, cameras (preview, photo, video in the camera app), GNSS (satellites and NMEA), sensors; every handle can be lent or taken while PixelOS runs.
- **Needs work**: camera video at 1080p records at about 12 fps (software encoder), no camera controls; no GNSS position fix verified yet; sensors show
  "Goldfish ..." names and fixed ranges.
- **Broken**: Wi-Fi while the phone's QCA6390 firmware has crashed (host-side, until reboot).
- **Not implemented**: call audio; step counter, significant motion, tilt and gesture sensors.

Details: [docs/android-vm.md](docs/android-vm.md).

| | |
|---|---|
| GPU | virtio-gpu DRM native context: Android's Mesa freedreno/turnip drives the host Adreno; zero-copy scanout |
| Display, touch, keys | the phone's panel by DRM lease, its touchscreen and buttons while the VM is shown |
| Network | native Wi-Fi per band (`mship_wifi`), shared internet |
| Telephony | Cuttlefish's RIL against the phone's real modem (`mship.modem`) |
| Cameras | the external camera HAL, patched for mothershipOS's camera frames (`mship.camera`) |
| Location | the phone's GNSS receiver (`mship.gnss`), our HAL on `/dev/gnss0` |
| Sensors | the phone's sensors (`mship.sensors`) through Cuttlefish's goldfish sensors sub-HAL on hvc13 |
| Audio | speaker, earpiece, headset, mics (`mship.audio`) through virtio-snd |
| Also | Bluetooth, battery, USB, hardware video decode (virtio-media) |

## Contents
| Path | What |
|---|---|
| `device/mothership/nio_vm/` | the Android product (goes into the PixelOS tree as `device/mothership/nio_vm`) |
| `patches/` | patches to the AOSP tree, per project (`cargo xtask android-patches`) |
| `mesa/` | Mesa for Android (pins, cross files; `cargo xtask android-mesa`) |
| `vm.json` | the VM manifest mothershipOS runs it from |
| `Dockerfile.build` | the build container for its guest modules |
| `docs/android-vm.md` | status; how it is built, packaged and run |
| `docs/gki-modules.md` | building guest kernel modules for the stock GKI kernel |
| `crates/pixelos-install` | its install step on the phone: the disk from the package's images, or an update that keeps the user's data |
| `crates/android-image` | Android image formats (sparse, GPT, boot images, bootconfig, LZ4 ramdisks, cpio) |
| `xtask/` | its build (`cargo xtask`) |

## Install (on the phone)
PixelOS cannot be built on the phone (it needs the PixelOS tree on an x86 PC), so the phone installs the prebuilt
package from a [VM store](https://github.com/MothershipOS/vmstore) (its page shows the command) or a file:

```
mship-system vm-install <store>/pixelos/package.tar.xz
```

The first time it makes the VM's disk (32 GB of user data, `MSHIP_PIXELOS_USERDATA_GIB`); later it writes the new
system into the installed disk and keeps the user's data. How mothershipOS installs VMs:
[VM packages](https://github.com/MothershipOS/mothershipOS/blob/main/docs/vm-packages.md).

## Build (on a PC)
With the PixelOS tree (`device/mothership/nio_vm` from here, `cargo xtask android-patches <tree>`,
`cargo xtask android-mesa <tree>`, then the build; `cargo xtask release <pkg dir>` packs it, also as the VM's store
folder `target/release-assets/store/pixelos`: the package, `store/info.txt` and `store/icon.svg`), in mothershipOS's build image (Rust with the phone's target,
Docker), with mothershipOS checked out next to this repository (its guest drivers):

```
cargo xtask android-package <product out> <pkg dir>
cargo xtask gki-module <gki tree> <aosp tree> mship_wifi      # and virtio_media; docs/gki-modules.md
cargo xtask android-initrd <pkg dir>
cargo xtask release <pkg dir>                                  # target/release-assets/pixelOS_16_VM.tar.xz
```

Publish the archive as a release asset; details in [docs/android-vm.md](docs/android-vm.md).
