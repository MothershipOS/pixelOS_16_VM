//! The PixelOS VM's build tasks (`cargo xtask <task>`), on a PC with the PixelOS tree; mothershipOS installs the
//! result on the phone (`mship-system vm-install`). Run them where mothershipOS's own tasks run (its build image:
//! Rust with the phone's target, Docker); the guest drivers come from mothershipOS (`guest/drivers`, the checkout
//! next to this one or MSHIP_SOURCE).
//!
//! - `android-*`: from a PixelOS build to the VM's package (android.rs)
//! - `gki-*`: guest modules for PixelOS's GKI kernel (gki.rs)
//! - `release <pkg dir> [out dir]`: the package as one archive to publish (release.rs)

use anyhow::{Result, bail};

mod android;
mod gki;
mod release;
mod util;

fn main() -> Result<()> {
    let task = std::env::args().nth(1).unwrap_or_default();
    let args: Vec<String> = std::env::args().skip(2).collect();
    match task.as_str() {
        t if t.starts_with("gki-") => gki::dispatch(t, &args),
        t if t.starts_with("android-") => android::dispatch(t, &args),
        "release" => {
            let pkg = args.first().map(std::path::PathBuf::from).ok_or_else(|| anyhow::anyhow!("release <pkg dir> [out dir]"))?;
            release::release(&pkg, args.get(1).map(std::path::PathBuf::from)).map(|_| ())
        }
        "" | "help" => {
            println!("cargo xtask android-patches|android-mesa|android-package|android-initrd|gki-module|gki-symvers|gki-symvers-elf|release (see xtask/src/main.rs)");
            Ok(())
        }
        other => bail!("unknown task {other:?} (cargo xtask help)"),
    }
}
