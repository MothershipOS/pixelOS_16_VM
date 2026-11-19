//! `release <pkg dir> [out dir]`: the VM's package for mothershipOS (`vm-install`), as one archive:
//! target/release-assets/pixelOS_16_VM.tar.xz, to publish as a release asset of this repository; and this VM's
//! folder for a VM store (vmstore), target/release-assets/store/pixelos: the archive as package.tar.xz, info.txt and
//! the icon (store/).
//!
//! It holds vm.json, mothership-package.json (id `pixelos`; `disk.img` kept on updates; the install step), the
//! install step `pixelos-install` (built for the phone) and `pkg/`: the partition images and userdata (the disk is
//! made from them on the phone, or an installed VM's partitions are updated from them) and what the launcher boots
//! (Image, the crosvm initrds, cmdline.crosvm). Make `<pkg dir>` with android-package, then gki-module for the guest
//! modules and android-initrd.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};

use crate::util::*;

/// pkg/ files the VM needs on the phone
const PKG: &[&str] = &[
    "Image", "initrd-crosvm-native.img", "initrd-crosvm.img", "cmdline.crosvm", "super.img", "vbmeta.img", "vbmeta_system.img",
    "vbmeta_vendor_dlkm.img", "vbmeta_system_dlkm.img", "boot.img", "init_boot.img", "vendor_boot.img", "userdata.img",
];
const PHONE_TARGET: &str = "aarch64-unknown-linux-gnu";

pub fn release(pkg: &Path, out: Option<PathBuf>) -> Result<PathBuf> {
    for f in PKG {
        ensure!(pkg.join(f).exists(), "{}: no {f} (android-package, then android-initrd)", pkg.display());
    }
    cargo(&["build", "--release", "--target", PHONE_TARGET, "-p", "pixelos-install"])?;
    let install = target_dir().join(PHONE_TARGET).join("release/pixelos-install");
    let out = out.unwrap_or_else(|| target_dir().join("release-assets"));
    let stage = out.join("stage");
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir_all(stage.join("pkg"))?;
    std::fs::copy(root().join("vm.json"), stage.join("vm.json"))?;
    std::fs::copy(root().join("mothership-package.json"), stage.join("mothership-package.json"))?;
    std::fs::copy(&install, stage.join("pixelos-install")).with_context(|| install.display().to_string())?;
    for f in PKG {
        std::fs::copy(pkg.join(f), stage.join("pkg").join(f))?;
    }
    let archive = out.join("pixelOS_16_VM.tar.xz");
    let st = std::process::Command::new("tar").env("XZ_OPT", "-T0 -6").arg("-cJf").arg(&archive).arg("-C").arg(&stage).arg(".").status()?;
    ensure!(st.success(), "tar {}", archive.display());
    std::fs::remove_dir_all(&stage)?;
    let store = out.join("store/pixelos");
    let _ = std::fs::remove_dir_all(&store);
    std::fs::create_dir_all(&store)?;
    for f in std::fs::read_dir(root().join("store"))? {
        let f = f?;
        std::fs::copy(f.path(), store.join(f.file_name()))?;
    }
    store_info(&store.join("info.txt"))?;
    // a hard link, not a second copy of the archive
    std::fs::hard_link(&archive, store.join("package.tar.xz")).or_else(|_| std::fs::copy(&archive, store.join("package.tar.xz")).map(|_| ()))?;
    println!("{} ({} MB): publish it as a release asset, or copy {} into the VM store; on the phone: \
              mship-system vm-install <its URL>", archive.display(), std::fs::metadata(&archive)?.len() >> 20, store.display());
    Ok(archive)
}
