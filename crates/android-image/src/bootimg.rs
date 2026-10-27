//! Android boot images, header version 3/4 (4 KiB pages): boot.img (kernel), init_boot.img (generic ramdisk) and
//! vendor_boot.img (vendor ramdisks, command line, bootconfig).

use anyhow::{Result, ensure};

use crate::u32le;

const PAGE: usize = 4096;

fn align(n: usize, to: usize) -> usize {
    n.div_ceil(to) * to
}

/// boot.img / init_boot.img: (kernel, ramdisk)
pub fn boot(img: &[u8]) -> Result<(&[u8], &[u8])> {
    ensure!(img.len() >= PAGE && &img[..8] == b"ANDROID!", "not an Android boot image");
    let (kernel_size, ramdisk_size, version) = (u32le(img, 8) as usize, u32le(img, 12) as usize, u32le(img, 40));
    ensure!(version >= 3, "boot image header v{version}: only v3/v4");
    let kernel = PAGE;
    let ramdisk = kernel + align(kernel_size, PAGE);
    ensure!(ramdisk + ramdisk_size <= img.len(), "boot image shorter than its header says");
    Ok((&img[kernel..kernel + kernel_size], &img[ramdisk..ramdisk + ramdisk_size]))
}

pub struct VendorBoot<'a> {
    pub cmdline: String,
    /// the vendor ramdisk table's fragments (unpack_bootimg's vendor_ramdisk00, 01, ...)
    pub ramdisks: Vec<&'a [u8]>,
    pub bootconfig: &'a [u8],
}

pub fn vendor_boot(img: &[u8]) -> Result<VendorBoot<'_>> {
    ensure!(img.len() >= 2128 && &img[..8] == b"VNDRBOOT", "not a vendor boot image");
    let version = u32le(img, 8);
    ensure!(version >= 4, "vendor boot header v{version}: only v4");
    let page = u32le(img, 12) as usize;
    let ramdisk_size = u32le(img, 24) as usize;
    let cmdline = String::from_utf8_lossy(&img[28..28 + 2048]).trim_end_matches('\0').to_string();
    let header_size = u32le(img, 2096) as usize;
    let dtb_size = u32le(img, 2100) as usize;
    let (table_size, entries, entry_size, bootconfig_size) =
        (u32le(img, 2112) as usize, u32le(img, 2116) as usize, u32le(img, 2120) as usize, u32le(img, 2124) as usize);
    let ramdisk_at = align(header_size, page);
    let dtb_at = ramdisk_at + align(ramdisk_size, page);
    let table_at = dtb_at + align(dtb_size, page);
    let bootconfig_at = table_at + align(table_size, page);
    ensure!(bootconfig_at + bootconfig_size <= img.len(), "vendor boot image shorter than its header says");
    let mut ramdisks = vec![];
    for i in 0..entries {
        let e = table_at + i * entry_size;
        let (size, offset) = (u32le(img, e) as usize, u32le(img, e + 4) as usize);
        ensure!(offset + size <= ramdisk_size, "vendor ramdisk {i} outside the ramdisk section");
        ramdisks.push(&img[ramdisk_at + offset..ramdisk_at + offset + size]);
    }
    Ok(VendorBoot { cmdline, ramdisks, bootconfig: &img[bootconfig_at..bootconfig_at + bootconfig_size] })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_v4() {
        let mut img = vec![0u8; PAGE * 3];
        img[..8].copy_from_slice(b"ANDROID!");
        img[8..12].copy_from_slice(&5u32.to_le_bytes());
        img[12..16].copy_from_slice(&3u32.to_le_bytes());
        img[40..44].copy_from_slice(&4u32.to_le_bytes());
        img[PAGE..PAGE + 5].copy_from_slice(b"KERNL");
        img[2 * PAGE..2 * PAGE + 3].copy_from_slice(b"RAM");
        assert_eq!(boot(&img).unwrap(), (&b"KERNL"[..], &b"RAM"[..]));
    }
}
