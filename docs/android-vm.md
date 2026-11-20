# Android VM (PixelOS on crosvm)

Guest: Cuttlefish arm64 product `custom_nio_vm` (PixelOS, Android 16 BP4A, GMS), user build.
Host: mothershipOS, user `mship`, sway session. VM dir on the phone: `/var/lib/mothership/vms/pixelos`
(`disk.img`, `pkg/` with kernel + initrd + cmdline, logs).

## Status
**Works**
- GPU + display: native context (see "GPU" below), zero-copy UBWC scanout with real present fences, 90 Hz, blur.
- Touch, volume keys, power key, double tap to wake.
- Wi-Fi: native per band (`mship.wifi`) or shared internet (item `internet`).
- Telephony (`mship.modem`): registration, mobile data, SMS, call signalling.
- Hardware video decode: H.264, HEVC, VP9, VP8 on the host's Venus, zero copy. Encode: H.264, VP8 on Venus
  (`patches/external/v4l2_codec2/0010`, `0011`): frames that are virtio-gpu buffers in Venus's layout (the camera's
  video buffers) go to Venus as they are (DMABUF); other frames (from dma-buf heaps, which virtio-media can't share)
  and the bitstream are copied through MMAP buffers. The launcher passes the encoder after the cameras so the camera
  ids stay 0-4.
- Bluetooth (`mship.bt`), battery (`mship.battery`), USB devices and USB disks (`mship.usb`).
- Audio (`mship.audio`): speaker, earpiece, wired headset, three mics, headset jack; Android routes natively.
- Cameras (`mship.camera`): five cameras (back, front, front 2, back 2, the mono depth camera); Aperture: back/front
  switching, the other cameras as extra lenses, full-resolution photos (16 MP on the back camera), preview, video
  recording, tap to focus, flash and torch; the cameras' sizes and rates are the host's modes (up to full size,
  4K, 1080p at 30 and 60 fps where the sensor has it). Frames go zero copy: the host renders each one into the HAL's
  own NV12 buffer, the guest GPU scales it into each stream; 4K video records at the camera's rate (24 fps with
  Aperture's 4:3 photo stream). Manual focus and resolution choice in apps that offer them
  (Open Camera with its Camera2 API).
- Hardware video encode (`mship.video`): H.264 and VP8 on the host's Venus (`c2.v4l2.avc.encoder`,
  `c2.v4l2.vp8.encoder`, up to 4K).
- GNSS (`mship.gnss`): satellites and NMEA through our HAL.
- Sensors (`mship.sensors`): accelerometer, magnetometer (compass), gyroscope, light, proximity; Android fuses
  rotation vector, gravity and linear acceleration.
- Hot plug: every handle (speaker, earpiece, headset, mics, modem, cameras, Bluetooth, USB, internet, location,
  sensors) can be lent or taken while PixelOS runs; console slots stay connected through device-service restarts.

**Needs work**
- Cameras: the external camera HAL captures one size covering all streams (4K video with Aperture's 4:3 photo
  stream: 3840x2880, at most 24 fps); JPEG reads the capture buffer on the CPU (NV12ToI420, then libjpeg).
  The front camera is grey above 1080p (the host's libcamera output, see `contracts/mship.camera-1.md`).
  Autofocus ignores the AF region (the host focuses on the centre); auto flash fires like always flash. Aperture can't be started for about a minute after boot ("Activity class does not
  exist" while the package is being scanned).
- GNSS: no position fix verified yet.
- Sensors: the HAL shows "Goldfish ..." names and fixed ranges; the first proximity value after boot is the HAL's
  placeholder for ~50 ms.
- If the VM dies while Android has the screen off, sway gets the panel back powered off
  (`swaymsg output DSI-1 power on`); the presenter should power it on before ending the lease.
- The product name `nio_vm` is phone-specific; rename it when the guest becomes phone-independent (one generic image).

**Broken**
- Wi-Fi, when the phone's QCA6390 firmware has crashed (host-side: no Wi-Fi for host or VM until reboot).

**Not implemented**
- Call audio.
- A `mship.power` adapter (the host builds PixelOS's power vote from what it sees).
- Sensors: step counter, significant motion, tilt, gestures.
- GNSS extensions (measurements, PSDS, batching, geofencing, AGNSS, ...).

## Devices
- GPU + display: Android's Mesa freedreno/turnip drives the Adreno; app and SurfaceFlinger buffers are UBWC msm
  dmabufs that the panel scans out zero-copy. The gfxstream fallback (~35-45 fps scrolling, ~12 ms/frame
  composition) is `MSHIP_VM_GPU=gfxstream`.
- Touch, volume keys, power key: Android's own while lent.
- Brightness: `patches/device/google/cuttlefish/0006`: Cuttlefish's lights HAL also has a backlight (without one
  Android's brightness does nothing), and `patches/frameworks/base/0001` sends it at 16 bits instead of 8 (8 bits
  made visible steps at low brightness); mothershipd applies it to the panel while PixelOS has the display
  (`contracts/mship.display-power-1.md`).
- Wi-Fi: native, per band (`mship.wifi`, items Wi-Fi 2.4 GHz / 5 GHz), or shared internet (NAT, item
  `internet`). See "Wi-Fi" below.
- Telephony: Cuttlefish's stock RIL against the phone's modem (`mship.modem`, item `cellular`). See
  `contracts/mship.modem-1.md`.
- Cameras: `mship.camera`, see `contracts/mship.camera-1.md`.
  - AOSP's external camera HAL (`android.hardware.camera.provider-V1-external-service`) on the virtio-media V4L2
    devices; Cuttlefish's emulated cameras are left out (`TARGET_CF_NO_EMULATED_CAMERA`).
  - `patches/hardware/interfaces/0001`: XBGR32 frames (converted with libyuv; used when the GPU path is off). `0004`: facing and sensor
    orientation from the device's `V4L2_CID_CAMERA_ORIENTATION` / `V4L2_CID_CAMERA_SENSOR_ROTATION` (the host sets
    them per camera; without them every camera is "external", shown as front). `0005`: a fixed AE target fps range
    [fps, fps] besides the webcam range [fps/2, fps]; recording apps that only offer fixed rates need it.
  - `camera/external_camera_config.xml`: `CameraIdOffset` -1, so the cameras after the video decoder (`/dev/video0`)
    are ids 0 (back), 1 (front), 2, 3 as on a phone; camera apps take only 0 and 1 as main cameras.
  - `camera/external_camera_config.xml` `FpsList`: up to 1088 lines at 60 fps, larger sizes at 30 (sizes taller
    than the last limit would be dropped); `MaxJpegBufferSize` 16 MiB for 16 MP stills.
  - `camera/media_profiles.xml` → `odm/etc/media_profiles_V1_0.xml` (searched before Cuttlefish's vendor copy,
    which only knows ids 0-6 of its emulated cameras): 720p, 1080p and 2160p H.264/AAC profiles for ids 0-8. Without a profile
    CameraX has no video quality and camera apps abort video mode.
  - `patches/hardware/interfaces/0006`: focus and light through the device's standard V4L2 controls (FOCUS_AUTO,
    FOCUS_ABSOLUTE, AUTO_FOCUS_START/STATUS, FLASH_LED_MODE, active only on cameras that have them): AF modes off
    (manual distance, 0-10 diopters spread over the lens range), auto (trigger), continuous with the host's AF
    state, one AF region (apps offer tap to focus only with one), flash for captures, `setTorchMode` (flashlight).
  - `patches/hardware/interfaces/0008`: NV12 capture into the HAL's own gralloc buffers (DMABUF: the host renders
    into them; the device takes their layout through its `bytesperline` and `nv12 scanlines` control) and the
    per-stream work on the GPU (`ExternalCameraGpu`: EGL/GLES3 on the virtio-gpu native context, the phone's Adreno
    650): the capture buffer's planes sampled as R8 / RG88 dma-buf images, cropped and scaled into each NV12/NV21
    output buffer (Android's EGL loader doesn't list `EGL_EXT_image_dma_buf_import` but passes the images to Mesa).
    JPEG, planar and Y16 outputs, or a failed import, use the CPU path (NV12ToI420). Properties:
    `ro.vendor.camera.external.dmabuf=false` captures through MMAP buffers; `ro.vendor.camera.external.gpu=false`
    takes XBGR32 frames converted on the CPU. sepolicy: `hal_camera_default` may open the GPU
    (`sepolicy/hal_camera_default.te`).
  - `patches/hardware/interfaces/0007`: the provider reports the torch's changes (`torchModeStatusChange`);
    without it the camera service never learns the torch is on and the flashlight tile only turns it on.
  - Features declared: `android.hardware.camera.flash-autofocus`, `android.hardware.camera.front`. The flashlight
    tile ("Torch") is offered in the quick-settings editor; an existing tile list doesn't get it by itself.
  - Overlay `packages/apps/Aperture`: `config_enableAuxCameras` true (the ultrawides).
  - The guest `virtio-media.ko` must hand back the device's offset for dequeued MMAP buffers (the HAL re-queues
    frames with offset 0; see mothershipOS `guest/drivers/virtio_media`).
- Bluetooth: the phone's controller at the HCI level (`mship.bt`, hardware item `bluetooth`), see
  `contracts/mship.bt-1.md`.
- GNSS: the phone's receiver (`mship.gnss`, hardware item `location`): our HAL reads the host's NMEA epochs on
  `/dev/gnss0` (see below and `contracts/mship.gnss-1.md`). Cuttlefish's mock GNSS HAL is not installed.
- Sensors (`mship.sensors`, every VM at once): Cuttlefish's goldfish sensors sub-HAL
  (`android.hardware.sensors@2.1-impl.cuttlefish`) under AOSP's multi-HAL on hvc13, selected with
  `LOCAL_SENSOR_PRODUCT_PACKAGE` in `custom_nio_vm.mk` (Cuttlefish's default in this tree is AOSP's example HAL
  with fake sensors). `patches/device/generic/goldfish/0002-sensors-host-follows-activation.patch`: the host
  enables only the sensors Android has active. Declared features: accelerometer, compass, gyroscope, light,
  proximity. See `contracts/mship.sensors-1.md`.
- `patches/hardware/interfaces/0003`: AOSP's example sensors HAL reports proximity "far" (the stock constant reads
  as covered: SystemUI's falsing classifier rejects lock screen swipe-ups and calls blank the screen). Only
  matters when the example HAL is selected.
- Battery: the phone's live battery and charger state, read-only, in every VM (`mship.battery`, AOSP health
  HAL instead of Cuttlefish's fixed one), see `contracts/mship.battery-1.md`.
- USB: devices plugged into the phone, passed to the guest's xHCI (`mship.usb`, hardware item `usb`); USB disks
  mount through vold (fstab patch). See `contracts/mship.usb-1.md`.
- Audio (`mship.audio`, items Speaker, Earpiece, Headset, Microphones in a VM's page › Hardware): the guest's
  virtio-snd card has one PCM per phone device (0 speaker, 1 earpiece, 2 wired headset, 3 main mic, 4 back mic,
  5 headset mic) and the headset jack (input switches). The stock AIDL HAL and Cuttlefish's policy list these
  devices (`patches/hardware/interfaces/0002`, `patches/device/google/cuttlefish/0004`; the HAL picks the PCM by
  device type), WiredAccessoryManager connects the headset on the jack's switches (overlay
  `config_useDevInputEventForAudioJack`).

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
  `MSHIP_VM_GPU=gfxstream` selects the gfxstream path (Cuttlefish's `/opt/crosvm`, `initrd-crosvm.img`).
- SM8250 has no stage-2 FWB: KVM can't map cached host memory into a guest (`kvm_supports_cacheable_pfnmap`), so
  virglrenderer advertises no cached-coherent BOs (patch 0001) and minigbm allocates write-combined only.
- gralloc can't use virgl: minigbm learns a virgl buffer's host layout only through the ChromeOS-only
  `RESOURCE_INFO_CROS` ioctl, which GKI lacks (strides get guessed wrong, imports fail).
- Our own crosvm: Cuttlefish's crosvm has only the memfd gralloc (not importable by the GPU). minigbm is
  linked statically and hidden (`--exclude-libs`) because virglrenderer in the same process uses Mesa's libgbm.
- Guest minigbm needs `patches/external/minigbm/0001` (`cargo xtask android-patches <tree>`): its private
  virtgpu header calls getparam 8 CREATE_GUEST_HANDLE (ChromeOS kernels); GKI uses 8 for EXPLICIT_DEBUG_NAME and
  answers yes, so without the patch every hardware buffer gets the unknown blob flag 0x8 → EINVAL, "no client
  target", black screen.
- crosvm's rutabaga minigbm gralloc stashes the buffer allocated for GET_IMAGE_REQUIREMENTS for the following
  CREATE_BLOB; upstream fails all later queries (`AlreadyInUse`) once one query has no CREATE_BLOB.
  `mothershipOS/host/gpu/patches/rutabaga_gfx/0001-minigbm-stash.patch` replaces stale stashes (a cargo `[patch]` override).
- SET_SCANOUT_BLOB carries no modifier; upstream crosvm imports scanouts as linear (UBWC frames show as colour
  noise). `mothershipOS/host/gpu/patches/crosvm/0001-scanout-modifier.patch` takes the modifier rutabaga recorded
  at allocation. Check: `display.log` "frames: dmabuf ... modifier 0x500000000000001", debugfs `dri/1/state`
  plane 41 the same.
- Panel at 90 Hz; the guest is a user build (`ro.debuggable=0`): adb works as `shell`, no `adb root`/strace on
  services. Small test programs: NDK on the buildfarm, `adb push` to `/data/local/tmp`.
- Mesa is built with `android-stub` (NDK): no IMapper5 backend (needs libui, a platform C++ ABI), hence
  gralloc.minigbm for buffer info. Mesa debug: `setprop debug.mesa.fd.mesa.debug msgs,layout`
  (`FOO_BAR` → `debug.mesa.foo.bar`).

## Display

- Lending the display (mothershipd) → `swaymsg lease_output DSI-1 on`: sway offers DSI-1 via wp_drm_lease_device_v1 and the
  presenter takes it. Taking it back → `… off`: sway revokes it and gets the panel back as a new output.
- Zero copy with real flip completion (default): the panel scans out the guest's buffer and the
  guest's page flip completes only once that frame is on screen. Stock virtio-gpu completes flips at once; with
  ranchu's two framebuffers the guest then draws into the frame being scanned out (flicker, tearing). The chain:
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
- The guest runs at 90 Hz (`MSHIP_VM_REFRESH`), the panel's only mode; at 60 Hz frames alternate between
  1 and 2 vblanks (judder). The framework's default refresh rate is 90 too (product overlay
  `config_defaultRefreshRate`/`config_defaultPeakRefreshRate`): with the stock 60, SurfaceFlinger renders at
  45 fps on the 90 Hz mode.
- `hwcomposer.mode=client` is required. Otherwise ranchu picks GuestFrameComposer (because vulkan=pastel):
  CPU composition plus gralloc readbacks, ~40 ms/frame. The host composer (vulkan unset) is no faster.
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
  render-thread GetBlob spins).
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
  rectangle from the bitstream (the firmware reports no crop: 1080p would come out 1088 rows), 8-bit 4:2:0
  profiles only, HEVC headers in front of the first picture (Venus drops a buffer with only VPS/SPS/PPS: no source
  change, hang).
- Performance (1080p, surface): VP9 ~390 fps, HEVC ~400 fps, H.264 ~290 fps; 4K VP9/H.264/HEVC ~110-120 fps; ~1.3 ms
  codec-process CPU per frame (software VP9: ~14.5 ms). Output pixels identical to the software decoders (Y checksums).
- Testing on the host: `v4l2-ctl --stream-from` only splits H.264 into frames; an HEVC/VP9 file goes in as 2 MB
  slices, so everything after the first frame of a slice comes back "P-frame, error" (no references). Not a decoder
  bug; feed whole frames (e.g. through the guest).
- Test without an app: `VDec` (`MediaCodec` + `ImageReader`) via `app_process`; `ro.vendor.v4l2_codec2.input_mmap`.

## GNSS (mship.gnss)
- `device/mothership/nio_vm/gnss`: `android.hardware.gnss-service.mship` (IGnss V6, `hal_gnss_default` domain),
  selected with `LOCAL_GNSS_PRODUCT_PACKAGE` in `custom_nio_vm.mk` instead of Cuttlefish's APEX.
- It opens `/dev/gnss0` (hvc6; property `vendor.mship.gnss.port` overrides) raw, says `$PMSHG,HELLO,1.0`, and
  sends `$PMSHG,START,<interval>` / `$PMSHG,STOP` when the framework starts and stops GNSS: the host runs the
  receiver only meanwhile. Each epoch (NMEA, ended by `RMC`) becomes `gnssSvStatusCb` (from GSV/GSA),
  `gnssNmeaCb` (the sentences) and `gnssLocationCb` (RMC + GGA altitude and separation = ellipsoid height, GST
  accuracies).
- Capabilities: scheduling only. Every extension (measurements, PSDS, batching, geofencing, AGNSS, ...) answers
  `EX_UNSUPPORTED_OPERATION`, which the framework treats as absent. The host does time and XTRA itself, so
  `injectTime`/`injectLocation` are accepted and dropped.
- Test without an app: `GpsTest` (`motorola_nio/archive/prototypes/modem/GpsTest.java`, app_process) prints the
  framework's GNSS status, NMEA and locations.
- Check: `dumpsys location` (`GNSS Hardware Model Name: mothershipOS mship.gnss`), on the host
  `echo status | socat - UNIX-SENDTO:/run/mothership/gnss.sock,bind=/tmp/q.sock`.

## Power key and screen state
- `mship-inputd` grabs `pm8941_pwrkey` while the VM has the display and forwards KEY_POWER. Lending the
  display sends KEY_WAKEUP. Android sleeps, wakes and times out on its own.
- Double tap to wake: with Android's screen off the panel sleeps powered and the touch controller is in gesture
  mode (host kernel, motorola_nio/docs/touch.md); a double tap reports KEY_WAKEUP on "Novatek NT36672C Wake
  Gesture", which inputd forwards like the power key. crosvm's virtio keyboard declares KEY_WAKEUP (patch 0011;
  without it the guest drops every KEY_WAKEUP, the lend-time wake included).
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
