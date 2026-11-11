//! Technical facts about a track's stream, read from the file itself (for
//! Song Info): the first bytes of the FLAC file or the MP4 init segment hold
//! the channel count, sample rate, bit depth and length; the HTTP headers hold
//! the file size of single-file streams. Independent of the audio engine.

use anyhow::Result;

use super::models::{StreamInfo, StreamSource};
use super::Tidal;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TechInfo {
    pub codec: String,
    pub channels: Option<u32>,
    pub sample_rate: Option<u32>,
    pub bits: Option<u32>,
    /// Total samples per channel (FLAC), when the header states it.
    pub total_samples: Option<u64>,
    /// Size of the file (single-file streams only).
    pub file_size: Option<u64>,
    /// DASH: the audio comes in segments (no single file size).
    pub segmented: bool,
}

/// FLAC STREAMINFO block (34 bytes): (sample rate, channels, bits, total samples).
fn streaminfo(b: &[u8]) -> Option<(u32, u32, u32, u64)> {
    if b.len() < 18 {
        return None;
    }
    let rate = ((b[10] as u32) << 12) | ((b[11] as u32) << 4) | ((b[12] as u32) >> 4);
    let channels = ((b[12] >> 1) & 0x07) as u32 + 1;
    let bits = ((((b[12] & 1) as u32) << 4) | ((b[13] as u32) >> 4)) + 1;
    let total = (((b[13] & 0x0f) as u64) << 32) | ((b[14] as u64) << 24) | ((b[15] as u64) << 16) | ((b[16] as u64) << 8) | b[17] as u64;
    (rate > 0).then_some((rate, channels, bits, total))
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Read what the start of a stream says: native FLAC, FLAC in MP4 (`dfLa`)
/// or AAC in MP4 (`mp4a`).
pub fn parse_header(b: &[u8], info: &mut TechInfo) {
    // Native FLAC: "fLaC", 4-byte block header, STREAMINFO.
    if b.starts_with(b"fLaC") {
        if let Some((r, c, bits, total)) = streaminfo(&b[8..]) {
            info.codec = "FLAC".into();
            (info.sample_rate, info.channels, info.bits, info.total_samples) = (Some(r), Some(c), Some(bits), (total > 0).then_some(total));
        }
        return;
    }
    // FLAC in MP4: 'dfLa', version/flags (4), block header (4), STREAMINFO.
    if let Some(i) = find(b, b"dfLa") {
        if let Some((r, c, bits, total)) = b.get(i + 12..).and_then(streaminfo) {
            info.codec = "FLAC".into();
            (info.sample_rate, info.channels, info.bits, info.total_samples) = (Some(r), Some(c), Some(bits), (total > 0).then_some(total));
        }
        return;
    }
    // AAC in MP4: sample entry 'mp4a' (8 reserved + 8 version, then channel count,
    // sample size, 4 more, sample rate as 16.16).
    if let Some(i) = find(b, b"mp4a") {
        let at = i + 4;
        if let Some(e) = b.get(at..at + 28) {
            let ch = u16::from_be_bytes([e[16], e[17]]) as u32;
            let rate = u16::from_be_bytes([e[24], e[25]]) as u32;
            info.codec = "AAC".into();
            if ch > 0 {
                info.channels = Some(ch);
            }
            if rate > 0 {
                info.sample_rate = Some(rate);
            }
        }
    }
}

impl Tidal {
    /// Read the stream's header (first 64 KiB, or the DASH init segment) and,
    /// for single files, the size from the server.
    pub fn probe_stream(&self, s: &StreamInfo) -> Result<TechInfo> {
        let mut info = TechInfo { codec: s.codec.to_uppercase(), sample_rate: s.sample_rate, bits: s.bit_depth, ..Default::default() };
        let http = self.http();
        match &s.source {
            StreamSource::Single(url) => {
                let r = http.get(url).header("Range", "bytes=0-65535").send()?;
                // "bytes 0-65535/12345678": the total is the file size.
                info.file_size = r
                    .headers()
                    .get("content-range")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.rsplit('/').next())
                    .and_then(|n| n.trim().parse().ok())
                    .or_else(|| r.content_length().filter(|n| *n > 65_536));
                let bytes = r.bytes()?;
                parse_header(&bytes, &mut info);
            }
            StreamSource::Segments(urls) => {
                info.segmented = true;
                if let Some(init) = urls.first() {
                    let bytes = http.get(init).send()?.bytes()?;
                    parse_header(&bytes, &mut info);
                }
            }
        }
        Ok(info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// STREAMINFO for 44.1 kHz, 2 channels, 16 bits, 1 000 000 samples.
    fn si() -> Vec<u8> {
        let mut b = vec![0u8; 34];
        let rate: u32 = 44_100;
        let (ch, bits, total): (u32, u32, u64) = (2, 16, 1_000_000);
        b[10] = (rate >> 12) as u8;
        b[11] = (rate >> 4) as u8;
        b[12] = (((rate & 0xf) << 4) as u8) | (((ch - 1) << 1) as u8) | (((bits - 1) >> 4) as u8);
        b[13] = ((((bits - 1) & 0xf) << 4) as u8) | ((total >> 32) as u8 & 0xf);
        b[14..18].copy_from_slice(&(total as u32).to_be_bytes());
        b
    }

    #[test]
    fn native_flac() {
        let mut b = b"fLaC".to_vec();
        b.extend([0x80, 0, 0, 34]);
        b.extend(si());
        let mut i = TechInfo::default();
        parse_header(&b, &mut i);
        assert_eq!((i.sample_rate, i.channels, i.bits, i.total_samples), (Some(44_100), Some(2), Some(16), Some(1_000_000)));
    }

    #[test]
    fn flac_in_mp4() {
        let mut b = vec![0u8; 20];
        b.extend(b"dfLa");
        b.extend([0, 0, 0, 0, 0x80, 0, 0, 34]);
        b.extend(si());
        let mut i = TechInfo::default();
        parse_header(&b, &mut i);
        assert_eq!((i.codec.as_str(), i.channels, i.sample_rate), ("FLAC", Some(2), Some(44_100)));
    }

    #[test]
    fn aac_in_mp4() {
        let mut b = vec![0u8; 10];
        b.extend(b"mp4a");
        let mut e = vec![0u8; 28];
        e[16..18].copy_from_slice(&2u16.to_be_bytes());
        e[24..26].copy_from_slice(&48_000u16.to_be_bytes());
        b.extend(e);
        let mut i = TechInfo::default();
        parse_header(&b, &mut i);
        assert_eq!((i.codec.as_str(), i.channels, i.sample_rate), ("AAC", Some(2), Some(48_000)));
    }
}
