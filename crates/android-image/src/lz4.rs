//! LZ4 "legacy" frames (as the kernel's initramfs unpacker and `lz4 -l` use them): the magic, then blocks of a
//! u32 compressed size and data, each up to 8 MiB uncompressed. Streams may be concatenated.

use anyhow::{Result, bail};

pub const MAGIC: [u8; 4] = [0x02, 0x21, 0x4c, 0x18];
const BLOCK: usize = 8 << 20;

/// Decompress concatenated legacy streams.
pub fn decompress(mut data: &[u8]) -> Result<Vec<u8>> {
    let mut out = vec![];
    let mut buf = vec![0u8; BLOCK];
    while !data.is_empty() {
        if data.starts_with(&MAGIC) {
            data = &data[4..];
            continue;
        }
        if data.len() < 4 {
            break;
        }
        let n = u32::from_le_bytes(data[..4].try_into().unwrap()) as usize;
        // trailing padding (zeros) after the last stream
        if n == 0 {
            break;
        }
        if n > data.len() - 4 {
            bail!("lz4 block of {n} B beyond the data");
        }
        let got = lz4_flex::block::decompress_into(&data[4..4 + n], &mut buf).map_err(|e| anyhow::anyhow!("lz4: {e}"))?;
        out.extend(&buf[..got]);
        data = &data[4 + n..];
    }
    Ok(out)
}

/// One legacy stream.
pub fn compress(data: &[u8]) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    for chunk in data.chunks(BLOCK) {
        let c = lz4_flex::block::compress(chunk);
        out.extend((c.len() as u32).to_le_bytes());
        out.extend(c);
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn roundtrip_concatenated() {
        let a: Vec<u8> = (0..100_000u32).flat_map(|i| (i % 251).to_le_bytes()).collect();
        let mut s = super::compress(&a);
        s.extend(super::compress(b"tail"));
        let mut want = a.clone();
        want.extend(b"tail");
        assert_eq!(super::decompress(&s).unwrap(), want);
    }
}
