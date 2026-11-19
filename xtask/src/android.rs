//! The PixelOS VM from a PixelOS build (custom_nio_vm, Cuttlefish arm64) to its package:
//!
//! - `android-patches <aosp tree>`: patches/<project>/*.patch into the tree (applied ones skipped)
//! - `android-mesa <aosp tree>`: Mesa freedreno + turnip for the guest (virtio kernel interface only, the NDK),
//!   into the tree's device/mothership/nio_vm/mesa
//! - `android-package <product out> <pkg dir>`: the package for direct kernel boot: Image, initrd.img (vendor
//!   ramdisk + generic ramdisk + bootconfig), cmdline, the partition images, virtio_balloon.ko (not on Cuttlefish's
//!   load list; the VM pool needs it)
//! - `android-initrd <pkg dir>`: the crosvm initrds from it, one per GPU mode (initrd-crosvm-native.img: DRM
//!   native context; initrd-crosvm.img: gfxstream), with the guest modules (mship_wifi.ko, virtio-media.ko, virtio-gpu.ko,
//!   virtio_balloon.ko in the package) in the first-stage ramdisk, and cmdline.crosvm
//!
//! The VM's disk is made on the phone from the package's images (pixelos-install, see release.rs).

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};
use android_image::{bootconfig, bootimg, cpio, lz4};

use crate::util::*;

// ---------------------------------------------------------------- AOSP tree

pub fn patches_into(tree: &Path) -> Result<()> {
    let base = root().join("patches");
    let mut found = vec![];
    walk(&base, &mut found)?;
    found.retain(|p| p.extension().is_some_and(|x| x == "patch"));
    found.sort();
    for p in found {
        let project = p.parent().unwrap().strip_prefix(&base)?;
        let dir = tree.join(project);
        let applied = Command::new("git").args(["apply", "--reverse", "--check"]).arg(&p).current_dir(&dir).output()?.status.success();
        if applied {
            println!("[patches] {}: {} already applied", project.display(), p.file_name().unwrap().to_string_lossy());
            continue;
        }
        let st = Command::new("git").arg("apply").arg(&p).current_dir(&dir).status()?;
        ensure!(st.success(), "{}: does not apply to {}", p.display(), dir.display());
        println!("[patches] {}: {} applied", project.display(), p.file_name().unwrap().to_string_lossy());
    }
    Ok(())
}

/// Mesa for the Android guest: freedreno (GLES) + turnip (Vulkan), virtio kernel interface only (DRM native
/// context), built with the NDK for arm64 (lib64) and arm (lib, the 32-bit zygote and apps) in the android-mesa
/// image. android-stub: it links only the NDK and libcutils/libhardware/liblog/libsync/libnativewindow at run time,
/// so Mesa has no IMapper5 backend; it reads minigbm buffer layouts through gralloc.minigbm (CrOS API).
/// The pinned NDK (pixelOS_16_VM/mesa/source.json), unpacked into target/android-mesa/ndk
fn ndk_dir(src: &serde_json::Value) -> Result<PathBuf> {
    let ndk = &src["ndk"];
    let work = target_dir().join("android-mesa");
    let dir = work.join("ndk").join(ndk["dir"].as_str().context("ndk dir")?);
    if !dir.exists() {
        let zip = fetch(ndk["url"].as_str().context("ndk url")?, ndk["sha256"].as_str().context("ndk sha256")?, &work.join("ndk.zip"))?;
        std::fs::create_dir_all(work.join("ndk"))?;
        run_in(&work.join("ndk"), &["unzip", "-q", &zip.to_string_lossy()])?;
    }
    Ok(dir)
}

fn mesa_source() -> Result<serde_json::Value> {
    Ok(serde_json::from_str(&std::fs::read_to_string(root().join("mesa/source.json"))?)?)
}

pub fn mesa(tree: &Path) -> Result<()> {
    let src = mesa_source()?;
    let work = target_dir().join("android-mesa");
    std::fs::create_dir_all(&work)?;
    let mesa = work.join("mesa");
    git_checkout(src["mesa"]["git"].as_str().context("mesa git")?, src["mesa"]["rev"].as_str().context("mesa rev")?, &mesa)?;
    let ndk = &src["ndk"];
    // the cross files find it at /mb/ndk/<dir>
    ndk_dir(&src)?;
    for f in ["android-arm64.cross", "android-arm.cross"] {
        std::fs::copy(root().join("mesa").join(f), work.join(f))?;
    }
    const IMAGE: &str = "mothership-android-mesa:24.04";
    docker(&["build", "-q", "-t", IMAGE, &root().join("xtask/docker/android-mesa").to_string_lossy()])?;
    let user = format!("{}:{}", unsafe { libc::getuid() }, unsafe { libc::getgid() });
    let out = tree.join("device/mothership/nio_vm/mesa");
    for (arch, lib) in [("arm64", "lib64"), ("arm", "lib")] {
        let build = format!("build-android-{arch}");
        let _ = std::fs::remove_dir_all(mesa.join(&build));
        let vol = format!("{}:/mb", host_path(&work)?);
        let meson = [
            "meson", "setup", &build, "--cross-file", &format!("/mb/android-{arch}.cross"), "--prefix=/vendor", &format!("--libdir={lib}"),
            "-Dbuildtype=release", "-Dallow-fallback-for=libdrm", "-Dlibdrm:default_library=static", "-Degl-lib-suffix=_mesa",
            "-Dgles-lib-suffix=_mesa", "-Dplatforms=android", "-Dandroid-stub=true", "-Dandroid-libbacktrace=disabled",
            "-Dplatform-sdk-version=35", "-Dgallium-drivers=freedreno", "-Dvulkan-drivers=freedreno", "-Dfreedreno-kmds=virtio",
            "-Degl=enabled", "-Dgbm=disabled", "-Dglx=disabled", "-Dgles1=enabled", "-Dgles2=enabled", "-Dllvm=disabled",
            "-Dzstd=disabled", "-Dexpat=disabled", "-Dvalgrind=disabled", "-Dlibunwind=disabled", "-Dtools=", "-Dshared-glapi=enabled",
        ];
        let mut setup = vec!["run", "--rm", "-u", &user, "-v", &vol, "-w", "/mb/mesa", IMAGE];
        setup.extend(meson);
        docker(&setup)?;
        docker(&["run", "--rm", "-u", &user, "-v", &vol, "-w", "/mb/mesa", IMAGE, "ninja", "-C", &build])?;
        let b = mesa.join(&build);
        let dst = out.join(lib);
        std::fs::create_dir_all(&dst)?;
        for f in ["src/egl/libEGL_mesa.so", "src/mesa/glapi/es2api/libGLESv2_mesa.so", "src/mesa/glapi/es1api/libGLESv1_CM_mesa.so",
                  "src/gallium/targets/dri/libgallium_dri.so"] {
            std::fs::copy(b.join(f), dst.join(Path::new(f).file_name().unwrap()))?;
        }
        std::fs::copy(b.join("src/freedreno/vulkan/libvulkan_freedreno.so"), dst.join("vulkan.freedreno.so"))?;
    }
    std::fs::copy(root().join("mesa/Android.bp"), out.join("Android.bp"))?;
    std::fs::write(
        out.join("VERSION"),
        format!("Mesa {} (mothershipOS cargo xtask android-mesa): freedreno + turnip, freedreno-kmds=virtio, NDK {} API 35, arm64 + arm\n",
                src["mesa"]["rev"].as_str().unwrap_or("?"), ndk["dir"].as_str().unwrap_or("?")),
    )?;
    println!("[mesa] installed to {}", out.display());
    Ok(())
}

// ---------------------------------------------------------------- the package

/// What launch_cvd/u-boot would pass the guest (Cuttlefish's bootconfig_args.cpp): boot device, slot _a,
/// unlocked/orange AVB, graphics, guest-only keymint/gatekeeper (no host secure_env). The crosvm initrds change some.
const BOOTCONFIG: &[(&str, &str)] = &[
    ("androidboot.slot_suffix", "_a"),
    ("androidboot.force_normal_boot", "1"),
    ("androidboot.verifiedbootstate", "orange"),
    ("androidboot.vbmeta.device_state", "unlocked"),
    ("androidboot.boot_devices", "4010000000.pcie"),
    ("androidboot.serialno", "MSHIPNIO0001"),
    ("androidboot.ddr_size", "4096MB"),
    ("androidboot.lcd_density", "420"),
    ("androidboot.setupwizard_mode", "OPTIONAL"),
    ("androidboot.enable_bootanimation", "1"),
    ("androidboot.fstab_suffix", "cf.f2fs.hctr2"),
    ("androidboot.wifi_mac_prefix", "5554"),
    ("androidboot.wifi_impl", "virt_wifi"),
    // mship.modem: the vsock port of the modem slot (Cuttlefish's rild finds its modem simulator there)
    ("androidboot.modem_simulator_ports", "9600"),
    ("androidboot.hw_timeout_multiplier", "3"),
    ("androidboot.hypervisor.version", "cf-qemu_cli"),
    ("androidboot.hypervisor.vm.supported", "0"),
    ("androidboot.hypervisor.protected_vm.supported", "0"),
    ("androidboot.console", "ttyAMA0"),
    ("androidboot.serialconsole", "1"),
    ("androidboot.selinux", "permissive"),
    ("androidboot.enable_confirmationui", "0"),
    ("androidboot.vendor.apex.com.android.hardware.keymint", "com.android.hardware.keymint.rust_nonsecure"),
    ("androidboot.vendor.apex.com.android.hardware.gatekeeper", "com.android.hardware.gatekeeper.nonsecure"),
    ("androidboot.vendor.apex.com.android.hardware.secure_element", "com.android.hardware.secure_element"),
    ("androidboot.vendor.apex.com.android.hardware.strongbox", "none"),
    ("androidboot.vendor.apex.com.android.hardware.graphics.composer", "com.android.hardware.graphics.composer.ranchu"),
    ("androidboot.cpuvulkan.version", "0"),
    ("androidboot.hardware.gralloc", "minigbm"),
    ("androidboot.hardware.hwcomposer", "ranchu"),
    ("androidboot.hardware.hwcomposer.mode", "client"),
    ("androidboot.hardware.hwcomposer.display_finder_mode", "drm"),
    ("androidboot.hardware.hwcomposer.display_framebuffer_format", "rgba"),
    ("androidboot.hardware.egl", "mesa"),
    ("androidboot.opengles.version", "196608"),
];

/// The partition images the disk is made of
const IMAGES: &[&str] = &["boot", "init_boot", "vendor_boot", "vbmeta", "vbmeta_system", "vbmeta_vendor_dlkm", "vbmeta_system_dlkm", "super", "userdata"];

pub fn package(product: &Path, out: &Path) -> Result<()> {
    std::fs::create_dir_all(out)?;
    let read = |n: &str| std::fs::read(product.join(format!("{n}.img"))).with_context(|| format!("{}/{n}.img", product.display()));
    let boot = read("boot")?;
    let init_boot = read("init_boot")?;
    let vendor_boot = read("vendor_boot")?;
    let (kernel, _) = bootimg::boot(&boot)?;
    let (_, generic_ramdisk) = bootimg::boot(&init_boot)?;
    let vb = bootimg::vendor_boot(&vendor_boot)?;
    let vendor_ramdisk = vb.ramdisks.first().context("vendor_boot: no vendor ramdisk")?;
    let text = String::from_utf8_lossy(vb.bootconfig);
    let mut kv: Vec<(String, String)> =
        text.trim_matches(['\0', '\n']).lines().filter_map(|l| l.split_once('=').map(|(k, v)| (k.to_string(), v.to_string()))).collect();
    for (k, v) in BOOTCONFIG {
        bootconfig::set(&mut kv, k, v);
    }
    let mut initrd = vendor_ramdisk.to_vec();
    initrd.extend(generic_ramdisk);
    initrd.extend(bootconfig::encode(&kv));
    std::fs::write(out.join("initrd.img"), initrd)?;
    std::fs::write(out.join("Image"), kernel)?;
    std::fs::write(out.join("cmdline"), format!("{} console=ttyAMA0\n", vb.cmdline))?;
    std::fs::write(out.join("bootconfig.txt"), kv.iter().map(|(k, v)| format!("{k}={v}\n")).collect::<String>())?;
    for n in IMAGES {
        std::fs::copy(product.join(format!("{n}.img")), out.join(format!("{n}.img")))?;
    }
    // the balloon driver (the VM pool lends RAM through it): built, but not on Cuttlefish's load list
    let balloon = product.join("system_dlkm/lib/modules/virtio_balloon.ko");
    if balloon.exists() {
        std::fs::copy(&balloon, out.join("virtio_balloon.ko"))?;
    }
    println!("package ready: {}", out.display());
    Ok(())
}

// ---------------------------------------------------------------- the crosvm initrds

const OVERLAY_TAG: &str = "mship-overlay-v1";

/// The ramdisk without a previous module overlay of ours (the last LZ4 stream holding the tag).
fn base_ramdisk(rd: &[u8]) -> &[u8] {
    let mut end = rd.len();
    while let Some(i) = rd[..end].windows(4).rposition(|w| w == lz4::MAGIC) {
        if i == 0 {
            break;
        }
        if lz4::decompress(&rd[i..]).is_ok_and(|d| d.windows(OVERLAY_TAG.len()).any(|w| w == OVERLAY_TAG.as_bytes())) {
            return &rd[..i];
        }
        end = i;
    }
    rd
}

/// Add kernel modules ((path, dependencies)) to the first-stage ramdisk: an LZ4 cpio appended to it (later entries
/// win) with the modules and extended modules.load/modules.dep; first-stage init loads them last.
fn add_modules(initrd: &[u8], mods: &[(PathBuf, Vec<&str>)]) -> Result<Vec<u8>> {
    let (rd, kv) = bootconfig::split(initrd)?;
    let rd = base_ramdisk(rd);
    let entries = cpio::read(&lz4::decompress(rd)?)?;
    let file = |name: &str| -> String {
        entries.iter().rev().find(|e| e.name == name).map(|e| String::from_utf8_lossy(&e.data).into_owned()).unwrap_or_default()
    };
    let mut load: Vec<String> = file("lib/modules/modules.load").lines().map(String::from).collect();
    let mut dep: Vec<String> = file("lib/modules/modules.dep").lines().map(String::from).collect();
    let mut overlay = vec![cpio::Entry { name: "lib".into(), mode: 0o040755, data: vec![] }, cpio::Entry { name: "lib/modules".into(), mode: 0o040755, data: vec![] }];
    for (path, deps) in mods {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        overlay.push(cpio::Entry { name: format!("lib/modules/{name}"), mode: 0o100644, data: std::fs::read(path)? });
        load.retain(|l| *l != name);
        load.push(name.clone());
        dep.retain(|l| !l.starts_with(&format!("/lib/modules/{name}:")));
        dep.push(format!("/lib/modules/{name}:{}", deps.iter().map(|d| format!(" /lib/modules/{d}")).collect::<String>()));
    }
    overlay.push(cpio::Entry { name: "lib/modules/modules.load".into(), mode: 0o100644, data: (load.join("\n") + "\n").into_bytes() });
    overlay.push(cpio::Entry { name: "lib/modules/modules.dep".into(), mode: 0o100644, data: (dep.join("\n") + "\n").into_bytes() });
    overlay.push(cpio::Entry { name: format!("lib/modules/{OVERLAY_TAG}"), mode: 0o100644, data: b"modules added by cargo xtask android-initrd\n".to_vec() });
    let mut out = rd.to_vec();
    out.extend(lz4::compress(&cpio::write(&overlay)));
    out.extend(bootconfig::encode(&kv));
    Ok(out)
}

pub fn initrd(pkg: &Path) -> Result<()> {
    let base = std::fs::read(pkg.join("initrd.img"))?;
    // native Wi-Fi (mship.wifi): with mship_wifi.ko in the package the guest gets the driver, and Cuttlefish's
    // virt_wifi wrapper stays off (wlan0 comes from mship_wifi on the 02:6d:73 NIC)
    let wifi = pkg.join("mship_wifi.ko");
    let media = pkg.join("virtio-media.ko");
    let mut mods = vec![];
    if wifi.exists() {
        mods.push((wifi.clone(), vec!["cfg80211.ko", "rfkill.ko"]));
    }
    // the virtio-media V4L2 bridge (mship.video: the host's Venus decoder); DMABUF buffers as virtio objects need
    // virtio_dma_buf.ko (Cuttlefish's, in the ramdisk)
    if media.exists() {
        mods.push((media, vec!["virtio_dma_buf.ko"]));
    }
    // virtio-gpu, patched (guest/drivers/virtio_gpu: page flips complete when the frame is on screen), in place of
    // Cuttlefish's module of the same name
    let gpu = pkg.join("virtio-gpu.ko");
    if gpu.exists() {
        mods.push((gpu, vec!["virtio_dma_buf.ko"]));
    }
    // virtio-balloon (the VM pool: RAM lent and taken back, page reporting), from the package
    let balloon = pkg.join("virtio_balloon.ko");
    if balloon.exists() {
        mods.push((balloon, vec![]));
    }
    let common: &[(&str, &str)] = &[
        ("androidboot.boot_devices", "10000.pci"),
        ("androidboot.console", "hvc1"),
        ("androidboot.hypervisor.version", "cf-crosvm"),
        // SurfaceFlinger composes on the GPU, ranchu only posts (else GuestFrameComposer: CPU, ~40 ms a frame)
        ("androidboot.hardware.hwcomposer.mode", "client"),
        ("androidboot.openthread_node_id", "1"),
        ("androidboot.vsock_lights_port", "6900"),
        ("androidboot.vsock_lights_cid", "2"),
        ("androidboot.wifi_impl", if wifi.exists() { "mship_wifi" } else { "virt_wifi" }),
    ];
    let modes: &[(&str, &[(&str, &str)])] = &[
        // DRM native context: the guest's Mesa freedreno/turnip drives the host Adreno through virtio-gpu
        ("initrd-crosvm-native.img", &[("androidboot.hardware.egl", "mesa"), ("androidboot.hardware.vulkan", "freedreno"),
            ("androidboot.opengles.version", "196610"), ("androidboot.cpuvulkan.version", ""), ("androidboot.hardware.gltransport", "")]),
        // gfxstream (host GLES translation): the fallback
        ("initrd-crosvm.img", &[("androidboot.hardware.egl", "emulation"), ("androidboot.hardware.vulkan", "pastel"),
            ("androidboot.cpuvulkan.version", "4202496"), ("androidboot.hardware.gltransport", "virtio-gpu-asg"),
            ("androidboot.opengles.version", "196609")]),
    ];
    for (name, gpu) in modes {
        let (rd, mut kv) = bootconfig::split(&base)?;
        for (k, v) in common.iter().chain(gpu.iter()) {
            bootconfig::set(&mut kv, k, v);
        }
        let mut img = rd.to_vec();
        img.extend(bootconfig::encode(&kv));
        if !mods.is_empty() {
            img = add_modules(&img, &mods)?;
        }
        std::fs::write(pkg.join(name), img)?;
        println!("[android-initrd] {}", pkg.join(name).display());
    }
    // the kernel console on hvc0 (virtio, the launcher's kernel.log) at loglevel 5: fast, and it carries ranchu's
    // display power lines the launcher turns into panel off/on; the emulated 8250 stays earlycon only.
    // kpti=0: the GKI turns KPTI on for KASLR alone (the vCPUs report no E0PD); the phone's cores (Cortex-A77/A55)
    // are not Meltdown-affected, and without it guest syscalls cost what they cost on the host (219 -> 173 ns)
    let cmdline = std::fs::read_to_string(pkg.join("cmdline"))?;
    let words: Vec<String> = cmdline
        .split_whitespace()
        .filter(|w| !w.starts_with("loglevel=") && !w.starts_with("kpti="))
        .map(|w| if w == "console=ttyAMA0" || w == "console=ttyS0" { "console=hvc0".to_string() } else { w.to_string() })
        .chain(["loglevel=5".to_string(), "kpti=0".to_string()])
        .collect();
    std::fs::write(pkg.join("cmdline.crosvm"), words.join(" ") + "\n")?;
    Ok(())
}

pub fn dispatch(task: &str, args: &[String]) -> Result<()> {
    let p = |i: usize| args.get(i).map(PathBuf::from).with_context(|| format!("{task}: missing argument {}", i + 1));
    match task {
        "android-patches" => patches_into(&p(0)?),
        "android-mesa" => mesa(&p(0)?),
        "android-package" => package(&p(0)?, &p(1)?),
        "android-initrd" => initrd(&p(0)?),
        _ => bail!("unknown task {task}"),
    }
}
