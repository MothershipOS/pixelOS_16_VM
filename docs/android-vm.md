# Android VM (PixelOS on crosvm)

Guest: Cuttlefish arm64 product `custom_nio_vm` (PixelOS, Android 16 BP4A, GMS), user build.
Host: mothershipOS, user `mship`, sway session. VM dir on the phone: `/var/lib/mothership/vms/pixelos`
(`disk.img`, `pkg/` with kernel + initrd + cmdline, logs).

## Status
- GPU + display: **native context** (see "GPU" below). Android's Mesa freedreno/turnip drives the Adreno; app and
  SurfaceFlinger buffers are UBWC msm dmabufs that the panel scans out zero-copy.
  The gfxstream fallback (~35-45 fps scrolling, ~12 ms/frame composition) stays as `MSHIP_VM_GPU=gfxstream`.
- Gap: if the VM dies while Android has the screen off, sway gets the panel back powered off
  (`swaymsg output DSI-1 power on`); the presenter should power it on before ending the lease.
- Touch, volume keys, power key: Android's own while lent.
- Wi-Fi: native, per band (`mship.wifi`, items Wi-Fi 2.4 GHz / 5 GHz), or shared internet (NAT, item
  `internet`). See "Wi-Fi" below.
- Telephony: Cuttlefish's stock RIL against the phone's modem (`mship.modem`, item `cellular`): registration,
  mobile data, SMS, call signalling; no call audio. See `contracts/mship.modem-1.md`.
- Cameras: the external camera HAL lists the phone's cameras (`mship.camera`); capture through an app is
  unverified. See `contracts/mship.camera-1.md`.
- Hardware video decode: H.264, HEVC, VP9, VP8 on the host's Venus (see below).
- Bluetooth: the phone's controller at the HCI level (`mship.bt`, hardware item `bluetooth`), see
  `contracts/mship.bt-1.md`.
- Battery: the phone's live battery and charger state, read-only, in every VM (`mship.battery`, AOSP health
  HAL instead of Cuttlefish's fixed one), see `contracts/mship.battery-1.md`.
- USB: devices plugged into the phone, passed to the guest's xHCI (`mship.usb`, hardware item `usb`); USB disks
  mount through vold (fstab patch). See `contracts/mship.usb-1.md`.
- Audio (`mship.audio`, items Speaker, Earpiece, Headset, Microphones in Settings › Hardware): the guest's
  virtio-snd card has one PCM per phone device (0 speaker, 1 earpiece, 2 wired headset, 3 main mic, 4 back mic,
  5 headset mic) and the headset jack (input switches). Android routes natively: the stock AIDL HAL and Cuttlefish's
  policy list these devices (`patches/hardware/interfaces/0002`, `patches/device/google/cuttlefish/0004`; the HAL
  picks the PCM by device type), WiredAccessoryManager connects the headset on the jack's switches (overlay
  `config_useDevInputEventForAudioJack`). No call audio.
- Sensors: still AOSP's example HAL (fake values) until the phone's sensors are bridged; its proximity sensor
  reports "far" (`patches/hardware/interfaces/0003`): the stock constant read as covered, so SystemUI's falsing
  classifier rejected lock screen swipe-ups and calls would blank the screen.
- Not yet: the phone's sensors, call audio, a `mship.power` adapter (the host builds PixelOS's power vote from
  what it sees).
- The product name `nio_vm` predates the decision to ship one generic image; rename it when the guest becomes
  phone-independent.

## Files

| On the phone | Source | Role |
|---|---|---|
| `/usr/local/bin/mship-launch` | `mothershipOS/crates/mship-launch` | launcher: crosvm from vm.json, drains the Cuttlefish vsock services the guest expects |
| `…/vms/pixelos/vm.json` | `pixelOS_16_VM/vm.json` | name, cid, platform, boot files, slots, `"display_mode": "lease"` |
| `/usr/local/bin/mship-presenter` | `mothershipOS/crates/mship-presenter` | presenter: Wayland server for crosvm + DRM lease client |
| `/opt/crosvm/bin/libGLESv2.so`, `libEGL.so` | `mothershipOS/crates/mship-gles-shim`, `mothershipOS/crates/mship-egl-shim` | zero-copy export; hides EGL_ANDROID_blob_cache |
| `/usr/local/bin/sway-mship` + `mship-sway.service.d/lease.conf` | `mothershipOS/host/sway/` (`cargo xtask sway`: sway 1.11 + patch) | `swaymsg lease_output <out> on/off` |
| `/usr/local/sbin/mothershipd`, `/usr/local/bin/mship`, `mship-inputd` | `mothershipOS/crates/mothershipd`, `mothershipOS/crates/mship`, `mothershipOS/crates/mship-inputd` | lend/take display, touch, keys and the other hardware items |
| `/usr/local/sbin/mship-system net` + `mship-vm-net.service` | `mothershipOS/crates/mship-system` | taps, DHCP, closed forwarding chains |
| `/opt/crosvm/` | Cuttlefish's prebuilt arm64 crosvm (`device/google/cuttlefish_vmm`) | VMM for the gfxstream fallback |
| `/opt/mship-gpu/{bin,lib}` | `mothershipOS/host/gpu/` (`cargo xtask gpu`) | crosvm + virglrenderer for native GPU |

## Build (on a build machine: the buildfarm)
- Tree `~/aosp/PixelOS` (includes Cuttlefish). Product `device/mothership/nio_vm` from this repository (working
  copy in the tree). Build natively: `source build/envsetup.sh && lunch custom_nio_vm-bp4a-userdebug && m`, or in
  the `mothership-android-build:24.04` container (`Dockerfile.build`).
- Product facts: Cuttlefish vsoc_arm64 + PixelOS `common_full_phone` with GMS, `full_base_telephony.mk`,
  telephony with Cuttlefish's stock rild/RIL (`mship.modem`: AT over vsock 9600 to the phone's modem; bootconfig
  `modem_simulator_ports`), `PRODUCT_ADB_KEYS` (host key baked in, adb over
  `vsock:3:5555`). GMS APKs need `git lfs pull`. Soong needs 32 GB extra swap.
- Image: `cargo xtask android-package <product out> <pkg>` (kernel, initrd with bootconfig, images),
  `cargo xtask android-initrd <pkg>` (below), `cargo xtask release <pkg>`: the package as one archive
  (`target/release-assets/pixelOS_16_VM.tar.xz`), published as a release asset. On the phone
  `mship-system vm-install <archive URL or file>` installs it; its install step (`pixelos-install`) makes the GPT
  disk the first time (12 partitions incl. frp, userdata grown to 32 GB) and later writes the new images (`pkg/`:
  super, the four vbmeta, boot images) into the existing disk, keeping userdata (the VM stopped). It always
  updates vbmeta together with super (super changes all four vbmeta images: `vbmeta_a`, `vbmeta_system_a`,
  `vbmeta_system_dlkm_a`, `vbmeta_vendor_dlkm_a`; a stale one fails dm-verity at boot).
  `cargo xtask android-initrd <pkg>` derives `initrd-crosvm-native.img`, `initrd-crosvm.img` (gfxstream) and
  `cmdline.crosvm` (bootconfig: `boot_devices=10000.pci`, `hwcomposer.mode=client`, console on hvc0 at loglevel 5).
- Before building: `cargo xtask android-patches ~/aosp/PixelOS`; after a Mesa update:
  `cargo xtask android-mesa ~/aosp/PixelOS`. Images: `m vendorimage superimage vbmetaimage
  vbmetasystemimage` (~11 min after a Soong change).

## GPU: DRM native context (default)
The guest runs its own Mesa (freedreno GLES 3.2, turnip Vulkan) and drives the host Adreno directly: GPU commands go
to the host kernel almost unchanged (virtio-gpu context type `drm`, virglrenderer's msm renderer), no GL translation.
Linux guest glmark2 1264 (offscreen) vs. 1068 native on the host.

```
guest app/SurfaceFlinger: Mesa freedreno/turnip (virtio KMD) --virtio-gpu ctx "drm"--> virglrenderer msm --> /dev/dri/renderD128
guest gralloc (minigbm cross_domain) --ctx "cross-domain"--> crosvm rutabaga gralloc = minigbm msm --> msm GEM dmabuf
   (exact stride/offset/modifier back to the guest; Mesa imports via gralloc.minigbm's CrOS API, rutabaga hands the
    same dmabuf to the msm context with import_blob; buffers are UBWC where minigbm chooses it)
```
| Piece | Source | Where |
|---|---|---|
| virglrenderer (msm) + patch | `cargo xtask gpu`, `mothershipOS/host/gpu/patches/virglrenderer` | `/opt/mship-gpu/lib` |
| crosvm with minigbm(msm) gralloc | `cargo xtask gpu` (upstream crosvm, static minigbm), `mothershipOS/host/gpu/patches/crosvm` | `/opt/mship-gpu/bin/crosvm` |
| Mesa for Android (arm64 + arm) | `cargo xtask android-mesa` (NDK, buildfarm docker) | product `nio_vm/mesa` → `/vendor/lib*/{egl,hw}` |
| gralloc.minigbm, sepolicy labels | `device/mothership/nio_vm` | `/vendor/lib*/hw` |
| AOSP tree patches (minigbm) | `pixelOS_16_VM/patches/` (`cargo xtask android-patches`) | vendor APEX `com.google.cf.gralloc` |
| bootconfig egl=mesa, vulkan=freedreno, GLES 3.2 | `cargo xtask android-initrd` | `pkg/initrd-crosvm-native.img` |

- The launcher uses native mode when `initrd-crosvm-native.img` and `/opt/mship-gpu/bin/crosvm` exist;
  `MSHIP_VM_GPU=gfxstream` selects the old path (Cuttlefish's `/opt/crosvm`, `initrd-crosvm.img`).
- SM8250 has no stage-2 FWB: KVM can't map cached host memory into a guest (`kvm_supports_cacheable_pfnmap`), so
  virglrenderer advertises no cached-coherent BOs (patch 0001) and minigbm allocates write-combined only.
- Why not virgl for gralloc: minigbm learns a virgl buffer's host layout only through the ChromeOS-only
  `RESOURCE_INFO_CROS` ioctl; GKI lacks it, so strides were guessed (4320 vs. the real 4352) and imports failed.
- Why our own crosvm: Cuttlefish's crosvm has only the memfd gralloc (not importable by the GPU). minigbm is
  linked statically and hidden (`--exclude-libs`) because virglrenderer in the same process uses Mesa's libgbm.
- Guest minigbm needs `patches/external/minigbm/0001` (`cargo xtask android-patches <tree>`): its private
  virtgpu header calls getparam 8 CREATE_GUEST_HANDLE (ChromeOS kernels); GKI uses 8 for EXPLICIT_DEBUG_NAME and
  answers yes, so every hardware buffer got the unknown blob flag 0x8 → EINVAL, "no client target", black screen.
- crosvm's rutabaga minigbm gralloc stashes the buffer allocated for GET_IMAGE_REQUIREMENTS for the following
  CREATE_BLOB; upstream fails all later queries (`AlreadyInUse`) once one query has no CREATE_BLOB.
  `mothershipOS/host/gpu/patches/rutabaga_gfx/0001-minigbm-stash.patch` replaces stale stashes (a cargo `[patch]` override).
- SET_SCANOUT_BLOB carries no modifier and crosvm imported scanouts as linear: UBWC frames showed as colour noise.
  `mothershipOS/host/gpu/patches/crosvm/0001-scanout-modifier.patch` takes the modifier rutabaga recorded at allocation. Check:
  `display.log` "frames: dmabuf ... modifier 0x500000000000001", debugfs `dri/1/state` plane 41 the same.
- Panel at 90 Hz; the guest is a user build (`ro.debuggable=0`): adb works as `shell`, no `adb root`/strace on
  services. Small test programs: NDK on the buildfarm, `adb push` to `/data/local/tmp`.
- Mesa is built with `android-stub` (NDK): no IMapper5 backend (needs libui, a platform C++ ABI), hence
  gralloc.minigbm for buffer info. Mesa debug: `setprop debug.mesa.fd.mesa.debug msgs,layout`
  (`FOO_BAR` → `debug.mesa.foo.bar`).

## Display

- Lending the display (mothershipd) → `swaymsg lease_output DSI-1 on`: sway offers DSI-1 via wp_drm_lease_device_v1 and the
  presenter takes it. Taking it back → `… off`: sway revokes it and gets the panel back as a new output.
- Zero copy with real flip completion (default): the panel scans out the guest's buffer and the
  guest's page flip completes only once that frame is on screen. Stock virtio-gpu completes flips at once and
  ranchu has two framebuffers, so the guest drew into the frame being scanned out (flicker, tearing). The chain:
  - guest `virtio-gpu.ko` (`mothershipOS/guest/drivers/virtio_gpu`, built against the stock GKI with matching
    CRCs, loaded from the first-stage ramdisk) fences every primary-plane flush; `virtio_gpu.flush_fence=0` on
    the kernel cmdline (pkg/cmdline.crosvm) restores stock behaviour.
  - crosvm (`host/gpu/patches/crosvm/0009-display-fences.patch`) holds a RESOURCE_FLUSH's fence until the
    presenter's Wayland frame callback for that commit (100 ms timeout).
  - the presenter sends the frame callback when the frame's KMS flip completes, at once for frames it won't show.
  - ranchu HWC (`patches/device/generic/goldfish/0001`): the blocking commit returns when the frame is on screen,
    so its out fence (the present fence) carries the real present time. The HWC reports present fences as
    reliable and phase-locks its software vsync to them; SurfaceFlinger runs with the sync framework and uses
    them (`patches/device/google/cuttlefish/0005`). Check: `dumpsys SurfaceFlinger | grep SYNC_FRAMEWORK`.
  `MSHIP_VM_PRESENT=copy`: the presenter copies each frame instead (~1.5 ms GPU per frame), stock pacing is fine.
- Blur works (SurfaceFlinger Kawase, SystemUI cross-window blur); the shade doesn't blur on the lock screen, by
  design (NotificationShadeDepthController).
- The guest runs at 90 Hz (`MSHIP_VM_REFRESH`), the panel's only mode. At 60 Hz frames alternated between
  1 and 2 vblanks (judder). The framework's default refresh rate is 90 too (product overlay
  `config_defaultRefreshRate`/`config_defaultPeakRefreshRate`): the stock 60 made SurfaceFlinger render at
  45 fps on the 90 Hz mode.
- `hwcomposer.mode=client` is required. Otherwise ranchu picks GuestFrameComposer (because vulkan=pastel):
  CPU composition plus gralloc readbacks, ~40 ms/frame. The host composer (vulkan unset) gave no gain.
- Perf tools: `input swipe` + `dumpsys gfxinfo <pkg> framestats`; perfetto works on the user build
  (`perfetto --txt -c -`), analyze with the `perfetto` python package; presenter `MSHIP_VM_DISPLAY_STATS=1`.

## Display path (gfxstream fallback)

```
guest SurfaceFlinger (client composition) --gfxstream--> host GL texture (freedreno, UBWC)
  crosvm display flush → transfer_read ==libGLESv2 shim==> dmabuf (once per buffer) → <vm>/display.sock-zc
  mship-presenter: AddFB2(UBWC) on the leased CRTC/plane, atomic page flip at 90 Hz
```
- Cuttlefish's prebuilt crosvm with its bundled libs runs from `/opt/crosvm` (`LD_LIBRARY_PATH=/opt/crosvm/bin`,
  `EGL_PLATFORM=surfaceless`, `MESA_SHADER_CACHE_DISABLE=true`; the EGL shim hides the blob cache, whose
  render-thread GetBlob spun).
- The zero-copy shim (`MSHIP_VM_ZERO_COPY`, default on) is LD_PRELOADed into crosvm. For crosvm's scanout
  readbacks (`stream_renderer_transfer_read_iov` with a host iovec) it exports the read FBO's texture with
  `EGL_MESA_image_dma_buf_export` instead of `glReadPixels`. crosvm's own shm frames then only pace the guest.
  Check: presenter log `frames: zero copy`, `/sys/kernel/debug/dri/1/state` plane 41 `modifier=0x500000000000001`.
  crosvm can't export non-blob gfxstream resources itself (rutabaga: handle None), hence the shim.
- Host Vulkan for gfxstream (`MSHIP_VM_HOST_VULKAN=true`, turnip) boots, but GL can't import its ColorBuffer
  memory (error 1286): black/stale frames, flicker. Keep it off.

## Hardware video decode (mship.video)
The host's Venus decoder (`--v4l2-proxy`, virtio-media) is a V4L2 stateful decoder in the guest. AOSP's
`external/v4l2_codec2` serves it as Codec2 components over the AIDL HAL (`device/mothership/nio_vm/media`: our
service, VINTF, rc, extended seccomp policy, `media_codecs_c2.xml`). Rank 0x80, so apps get it ahead of the
software decoders: H.264, HEVC (Main), VP9 (profile 0), VP8.
```
app MediaCodec --AIDL Codec2--> c2.v4l2.* (vendor.media.c2-v4l2, mediacodec domain)
  bitstream: copied into Venus' MMAP OUTPUT buffers (mapped into the guest by virtio-media)
  frames:    DMABUF CAPTURE = the app's gralloc buffers (virtio-gpu blobs) -> virtio object UUIDs ->
             crosvm resource bridge -> host DMABUF -> Venus writes them directly (zero copy)
```
- Guest driver: `mothershipOS/guest/drivers/virtio_media` (DMABUF virtio objects, vb2-like poll); host:
  `host/gpu/patches/virtio-media/0003`, `host/gpu/patches/crosvm/0010`.
- `patches/external/v4l2_codec2` 0001-0009: MMAP input, no ARCVM initial output (Venus only has a CAPTURE format
  after the stream header: `ro.vendor.v4l2_codec2.initial_output=false`), the AIDL HAL's IGBA and non-bufferpool
  GRALLOC pools, a real initial OUTPUT size (empty -> Venus takes 8192x8192, STREAMON -ENOMEM), the visible
  rectangle from the bitstream (the firmware reports no crop: 1080p came out 1088 rows), 8-bit 4:2:0 profiles only,
  HEVC headers in front of the first picture (Venus drops a buffer with only VPS/SPS/PPS: no source change, hang).
- Measured (1080p, surface): VP9 ~390 fps, HEVC ~400 fps, H.264 ~290 fps; 4K VP9/H.264/HEVC ~110-120 fps; ~1.3 ms codec-process CPU per
  frame (software VP9: ~14.5 ms). Output pixels identical to the software decoders (Y checksums).
- Testing on the host: `v4l2-ctl --stream-from` only splits H.264 into frames; an HEVC/VP9 file goes in as 2 MB
  slices, so everything after the first frame of a slice comes back "P-frame, error" (no references). Not a decoder
  bug; feed whole frames (e.g. through the guest).
- Test without an app: `VDec` (`MediaCodec` + `ImageReader`) via `app_process`; `ro.vendor.v4l2_codec2.input_mmap`.

## Power key and screen state
- `mship-inputd` grabs `pm8941_pwrkey` while the VM has the display and forwards KEY_POWER. Lending the
  display sends KEY_WAKEUP. Android sleeps, wakes and times out on its own.
- Double tap to wake: with Android's screen off the panel sleeps powered and the touch controller is in gesture
  mode (host kernel, motorola_nio/docs/touch.md); a double tap reports KEY_WAKEUP on "Novatek NT36672C Wake
  Gesture", which inputd forwards like the power key. crosvm's virtio keyboard declares KEY_WAKEUP (patch 0011:
  before it, the guest dropped every KEY_WAKEUP, the lend-time wake included).
- ranchu writes `VIRTUAL_DEVICE_DISPLAY_POWER_MODE_CHANGED display=0 mode=OFF|ON` to /dev/kmsg. The kernel
  console goes to hvc0 (`$D/kernel.log`). The launcher follows it and signals the presenter (SIGUSR1 off /
  SIGUSR2 on), which switches the leased CRTC off/on (panel + WLED).

## Wi-Fi (native, mship.wifi)
- `mship_wifi.ko` in `pkg/` makes `cargo xtask android-initrd` add it to the first-stage ramdisk and set
  `androidboot.wifi_impl=mship_wifi` (no virt_wifi). It creates `wlan0` on the 4th NIC (`pxwltap0`,
  MAC 02:6d:73:00:00:03). Android's own Wi-Fi stack drives the host's station interface of the lent band
  (`mshw2g` or `mshw5g`) through `mship-wifi-bridge` (vsock 7710); one band per VM at a time. Details and gaps: `contracts/mship.wifi-1.md`.
- Android prefers a validated Ethernet over Wi-Fi: don't lend shared internet to a VM that uses native Wi-Fi.
- Bridge control: `attach 3 2g` / `detach 3 user` / `status` on `/run/mothership/wifi-bridge.sock`.

## Wi-Fi (shared internet)
- `mship-vm-net.service` (boot, `mship-system net`): taps pxmtap0/pxetap0/pxwtap0/pxwltap0, dnsmasq DHCP,
  iptables-legacy chains `MSHIP_VM` closed (DROP). Without native Wi-Fi the guest's eth2 becomes virt_wifi
  `wlan0` ("VirtWifi").
- Item `internet`: mothershipd NATs pxwtap0 out of the host's uplink while the VM runs; taking it back or stopping
  the VM closes it again. Screen switches don't move it.
