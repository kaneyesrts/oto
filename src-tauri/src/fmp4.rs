//! Fragmented MP4 (HLS fMP4, AAC) → ADTS AAC, so the decoder gets a plain frame stream it
//! can read front to back. Symphonia's MP4 reader wants to seek around a file it cannot
//! see the end of yet; ADTS needs no seeking at all.

/// AAC parameters from the init segment's AudioSpecificConfig.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AacConfig {
    pub object_type: u8,
    pub freq_index: u8,
    pub channels: u8,
}

fn be32(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

/// Iterates boxes in `b[start..end]`, yielding (type, payload start, box end).
fn boxes(b: &[u8], start: usize, end: usize) -> impl Iterator<Item = ([u8; 4], usize, usize)> + '_ {
    let mut i = start;
    std::iter::from_fn(move || {
        if i + 8 > end {
            return None;
        }
        let mut size = be32(b, i)? as usize;
        let typ: [u8; 4] = b.get(i + 4..i + 8)?.try_into().ok()?;
        let mut header = 8;
        if size == 1 {
            size = u64::from_be_bytes(b.get(i + 8..i + 16)?.try_into().ok()?) as usize;
            header = 16;
        } else if size == 0 {
            size = end - i;
        }
        if size < header || i + size > end {
            return None;
        }
        let item = (typ, i + header, i + size);
        i += size;
        Some(item)
    })
}

fn find<'a>(b: &'a [u8], start: usize, end: usize, path: &[&[u8; 4]]) -> Option<(usize, usize)> {
    let (first, rest) = path.split_first()?;
    let (_, s, e) = boxes(b, start, end).find(|(t, _, _)| t == *first)?;
    if rest.is_empty() { Some((s, e)) } else { find(b, s, e, rest) }
}

/// Reads an MPEG-4 descriptor length (1–4 bytes, 7 bits each).
fn desc_len(b: &[u8], i: &mut usize) -> Option<usize> {
    let mut n = 0usize;
    for _ in 0..4 {
        let c = *b.get(*i)?;
        *i += 1;
        n = (n << 7) | (c & 0x7f) as usize;
        if c & 0x80 == 0 {
            break;
        }
    }
    Some(n)
}

/// Finds the AAC config in an init segment (moov/…/stsd/mp4a/esds).
pub fn parse_init(b: &[u8]) -> Option<AacConfig> {
    let (s, e) = find(b, 0, b.len(), &[b"moov", b"trak", b"mdia", b"minf", b"stbl", b"stsd"])?;
    // stsd: version/flags(4) + entry count(4), then sample entries.
    let (_, ms, me) = boxes(b, s + 8, e).find(|(t, _, _)| t == b"mp4a")?;
    // AudioSampleEntry has 28 bytes of fixed fields before its child boxes.
    let (es, ee) = boxes(b, ms + 28, me).find(|(t, _, _)| t == b"esds").map(|(_, s, e)| (s, e))?;
    let mut i = es + 4; // version/flags
    // ES_Descriptor (0x03) → DecoderConfigDescriptor (0x04) → DecoderSpecificInfo (0x05)
    while i < ee {
        let tag = b[i];
        i += 1;
        let len = desc_len(b, &mut i)?;
        match tag {
            0x03 => {
                let flags = *b.get(i + 2)?;
                i += 3;
                if flags & 0x80 != 0 { i += 2; }
                if flags & 0x40 != 0 { i += 1 + *b.get(i)? as usize; }
                if flags & 0x20 != 0 { i += 2; }
            }
            0x04 => i += 13,
            0x05 => {
                let asc = b.get(i..i + len.max(2))?;
                let object_type = asc[0] >> 3;
                let freq_index = ((asc[0] & 0x07) << 1) | (asc[1] >> 7);
                let channels = (asc[1] >> 3) & 0x0f;
                return Some(AacConfig { object_type, freq_index, channels });
            }
            _ => i += len,
        }
    }
    None
}

fn adts_header(c: AacConfig, payload_len: usize) -> [u8; 7] {
    let len = payload_len + 7;
    let profile = c.object_type.saturating_sub(1) & 0x3;
    [
        0xFF,
        0xF1, // MPEG-4, layer 0, no CRC
        (profile << 6) | ((c.freq_index & 0xf) << 2) | ((c.channels >> 2) & 0x1),
        ((c.channels & 0x3) << 6) | ((len >> 11) as u8 & 0x3),
        (len >> 3) as u8,
        (((len & 0x7) as u8) << 5) | 0x1F,
        0xFC,
    ]
}

/// Converts one media segment (moof+mdat pairs) to ADTS frames.
pub fn segment_to_adts(b: &[u8], c: AacConfig) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len() + b.len() / 50);
    let mut pending: Option<(usize, Vec<u32>)> = None; // (absolute data offset, sample sizes)
    for (typ, s, e) in boxes(b, 0, b.len()) {
        if &typ == b"moof" {
            let moof_start = s - 8;
            let Some((ts, te)) = find(b, s, e, &[b"traf"]) else { continue };
            // tfhd may carry a default sample size.
            let mut default_size = 0u32;
            if let Some((hs, _)) = find(b, ts, te, &[b"tfhd"]) {
                let flags = be32(b, hs).unwrap_or(0) & 0xFFFFFF;
                let mut i = hs + 8; // version/flags + track id
                if flags & 0x01 != 0 { i += 8; }
                if flags & 0x02 != 0 { i += 4; }
                if flags & 0x08 != 0 { i += 4; }
                if flags & 0x10 != 0 {
                    default_size = be32(b, i).unwrap_or(0);
                }
            }
            let Some((rs, _)) = find(b, ts, te, &[b"trun"]) else { continue };
            let flags = be32(b, rs).unwrap_or(0) & 0xFFFFFF;
            let count = be32(b, rs + 4).unwrap_or(0) as usize;
            let mut i = rs + 8;
            let mut data_off = None;
            if flags & 0x001 != 0 {
                data_off = Some(be32(b, i).unwrap_or(0) as i32);
                i += 4;
            }
            if flags & 0x004 != 0 { i += 4; }
            let mut sizes = Vec::with_capacity(count);
            for _ in 0..count {
                if flags & 0x100 != 0 { i += 4; }
                let size = if flags & 0x200 != 0 {
                    let v = be32(b, i).unwrap_or(0);
                    i += 4;
                    v
                } else {
                    default_size
                };
                if flags & 0x400 != 0 { i += 4; }
                if flags & 0x800 != 0 { i += 4; }
                sizes.push(size);
            }
            let abs = data_off.map(|o| (moof_start as i64 + o as i64) as usize);
            pending = Some((abs.unwrap_or(usize::MAX), sizes));
        } else if &typ == b"mdat" {
            if let Some((abs, sizes)) = pending.take() {
                let mut p = if abs == usize::MAX { s } else { abs };
                for sz in sizes {
                    let sz = sz as usize;
                    if sz == 0 || p + sz > e {
                        break;
                    }
                    out.extend_from_slice(&adts_header(c, sz));
                    out.extend_from_slice(&b[p..p + sz]);
                    p += sz;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(typ: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        v.extend_from_slice(typ);
        v.extend_from_slice(payload);
        v
    }

    #[test]
    fn adts_header_fields() {
        let c = AacConfig { object_type: 2, freq_index: 4, channels: 2 }; // AAC-LC 44.1k stereo
        let h = adts_header(c, 100);
        assert_eq!(h[0], 0xFF);
        assert_eq!(h[1] & 0xF0, 0xF0);
        assert_eq!(h[2] >> 6, 1); // profile LC = 1
        assert_eq!((h[2] >> 2) & 0xf, 4);
        let len = (((h[3] & 0x3) as usize) << 11) | ((h[4] as usize) << 3) | (h[5] >> 5) as usize;
        assert_eq!(len, 107);
    }

    #[test]
    fn converts_a_minimal_fragment() {
        let c = AacConfig { object_type: 2, freq_index: 3, channels: 2 };
        // trun: flags 0x201 (data offset + sample sizes), 2 samples of 3 and 2 bytes.
        let mut trun = vec![0, 0, 0x02, 0x01];
        trun.extend_from_slice(&2u32.to_be_bytes());
        trun.extend_from_slice(&0u32.to_be_bytes()); // data offset, patched below
        trun.extend_from_slice(&3u32.to_be_bytes());
        trun.extend_from_slice(&2u32.to_be_bytes());
        let tfhd = vec![0, 0, 0, 0, 0, 0, 0, 1];
        let traf = [mk(b"tfhd", &tfhd), mk(b"trun", &trun)].concat();
        let mut moof = mk(b"moof", &mk(b"traf", &traf));
        let off = (moof.len() + 8) as u32; // samples start right after mdat's header
        let pos = moof.len() - trun.len() + 8;
        moof[pos..pos + 4].copy_from_slice(&off.to_be_bytes());
        let seg = [moof, mk(b"mdat", &[1, 2, 3, 4, 5])].concat();
        let out = segment_to_adts(&seg, c);
        assert_eq!(out.len(), 7 + 3 + 7 + 2);
        assert_eq!(&out[7..10], &[1, 2, 3]);
        assert_eq!(&out[17..19], &[4, 5]);
    }
}
