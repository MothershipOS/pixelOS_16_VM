//! Android image formats, as the PixelOS VM needs them (no AOSP host tools):
//!
//! - [`sparse`]: Android sparse images (simg2img)
//! - [`gpt`]: GUID partition tables (create a disk, find a partition by name)
//! - [`bootimg`]: boot / init_boot / vendor_boot images, header v4 (unpack_bootimg)
//! - [`bootconfig`]: the bootconfig trailer of an Android initrd
//! - [`lz4`], [`cpio`]: the legacy-LZ4 newc cpio archives Android ramdisks are made of

pub mod bootconfig;
pub mod bootimg;
pub mod cpio;
pub mod gpt;
pub mod lz4;
pub mod sparse;

fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

fn u64le(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}
