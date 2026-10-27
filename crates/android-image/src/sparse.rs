//! Android sparse images: a header, then chunks (raw data, a fill pattern, "don't care" holes, a CRC).

use std::io::{Read, Seek, SeekFrom, Write};

use anyhow::{Result, bail, ensure};

use crate::u32le;

const MAGIC: u32 = 0xed26_ff3a;
const RAW: u16 = 0xcac1;
const FILL: u16 = 0xcac2;
const DONT_CARE: u16 = 0xcac3;
const CRC32: u16 = 0xcac4;

/// Is this an Android sparse image (its first four bytes)?
pub fn is_sparse(head: &[u8]) -> bool {
    head.len() >= 4 && u32le(head, 0) == MAGIC
}

/// Expand a sparse image into `out` at byte `at` (holes and zero fills are skipped: `out` stays sparse); its
/// expanded size.
pub fn expand<R: Read, W: Write + Seek>(src: &mut R, out: &mut W, at: u64) -> Result<u64> {
    let mut h = [0u8; 28];
    src.read_exact(&mut h)?;
    ensure!(u32le(&h, 0) == MAGIC, "not an Android sparse image");
    let hdr_sz = u16::from_le_bytes([h[8], h[9]]) as usize;
    let chunk_hdr_sz = u16::from_le_bytes([h[10], h[11]]) as usize;
    let (blk_sz, total_blks, total_chunks) = (u32le(&h, 12) as u64, u32le(&h, 16) as u64, u32le(&h, 20));
    ensure!(hdr_sz >= 28 && chunk_hdr_sz >= 12, "sparse header sizes {hdr_sz}/{chunk_hdr_sz}");
    std::io::copy(&mut src.by_ref().take((hdr_sz - 28) as u64), &mut std::io::sink())?;
    let mut pos = at;
    let mut buf = vec![0u8; 1 << 20];
    for _ in 0..total_chunks {
        let mut c = vec![0u8; chunk_hdr_sz];
        src.read_exact(&mut c)?;
        let (kind, blocks, total) = (u16::from_le_bytes([c[0], c[1]]), u32le(&c, 4) as u64, u32le(&c, 8) as u64);
        let data = total - chunk_hdr_sz as u64;
        let len = blocks * blk_sz;
        match kind {
            RAW => {
                ensure!(data == len, "raw chunk of {len} B carries {data} B");
                out.seek(SeekFrom::Start(pos))?;
                let mut left = len;
                while left > 0 {
                    let n = left.min(buf.len() as u64) as usize;
                    src.read_exact(&mut buf[..n])?;
                    out.write_all(&buf[..n])?;
                    left -= n as u64;
                }
            }
            FILL => {
                let mut p = [0u8; 4];
                src.read_exact(&mut p)?;
                if p != [0; 4] {
                    out.seek(SeekFrom::Start(pos))?;
                    let pattern: Vec<u8> = p.iter().copied().cycle().take(buf.len()).collect();
                    let mut left = len;
                    while left > 0 {
                        let n = left.min(pattern.len() as u64) as usize;
                        out.write_all(&pattern[..n])?;
                        left -= n as u64;
                    }
                }
            }
            DONT_CARE => {}
            CRC32 => {
                std::io::copy(&mut src.by_ref().take(data), &mut std::io::sink())?;
                continue;
            }
            other => bail!("sparse chunk type {other:#x}"),
        }
        pos += len;
    }
    let size = total_blks * blk_sz;
    ensure!(pos - at == size, "sparse image expands to {} B, header says {size}", pos - at);
    Ok(size)
}

/// The expanded size of a sparse image (from its header).
pub fn expanded_size(head: &[u8]) -> Option<u64> {
    is_sparse(head).then(|| u32le(head, 12) as u64 * u32le(head, 16) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(kind: u16, blocks: u32, payload: &[u8]) -> Vec<u8> {
        let mut c = kind.to_le_bytes().to_vec();
        c.extend([0, 0]);
        c.extend(blocks.to_le_bytes());
        c.extend((12 + payload.len() as u32).to_le_bytes());
        c.extend(payload);
        c
    }

    #[test]
    fn expands_raw_fill_and_holes() {
        let blk = 8u32;
        let mut img = MAGIC.to_le_bytes().to_vec();
        img.extend([1, 0, 0, 0, 28, 0, 12, 0]);
        img.extend(blk.to_le_bytes());
        img.extend(4u32.to_le_bytes()); // blocks
        img.extend(3u32.to_le_bytes()); // chunks
        img.extend(0u32.to_le_bytes());
        img.extend(chunk(RAW, 1, b"ABCDEFGH"));
        img.extend(chunk(DONT_CARE, 2, &[]));
        img.extend(chunk(FILL, 1, &[1, 2, 3, 4]));
        let mut out = std::io::Cursor::new(vec![0u8; 32]);
        assert_eq!(expand(&mut img.as_slice(), &mut out, 0).unwrap(), 32);
        let o = out.into_inner();
        assert_eq!(&o[..8], b"ABCDEFGH");
        assert_eq!(&o[8..24], &[0; 16]);
        assert_eq!(&o[24..], &[1, 2, 3, 4, 1, 2, 3, 4]);
        assert_eq!(expanded_size(&img), Some(32));
    }
}
