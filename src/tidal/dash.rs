//! Minimal MPEG-DASH (MPD) parser for TIDAL's audio manifests: one period,
//! one audio representation, `SegmentTemplate` with a `SegmentTimeline`.

use anyhow::{anyhow, Result};

pub struct Mpd {
    /// Initialization segment first, then every media segment in order.
    pub urls: Vec<String>,
    /// Duration in seconds of each media segment (not the init segment).
    pub seg_secs: Vec<f64>,
    pub codec: String,
    pub bit_depth: Option<u32>,
    pub sample_rate: Option<u32>,
}

/// Upper bound on segments in one manifest. A 4-hour track in 4 s segments is
/// 3600; anything far above that is a malformed manifest, not a real track.
const MAX_SEGMENTS: u64 = 20_000;

pub fn parse(xml: &str) -> Result<Mpd> {
    let doc = roxmltree::Document::parse(xml)?;
    // Abyss only plays streams the account receives unencrypted. A protected
    // (DRM) representation is refused outright, never worked around.
    if doc.descendants().any(|n| n.has_tag_name("ContentProtection")) {
        return Err(anyhow!(crate::i18n::t("protected_drm_stream_is_unsupported")));
    }
    // Pick the representation with the highest bandwidth.
    let rep = doc
        .descendants()
        .filter(|n| n.has_tag_name("Representation"))
        .max_by_key(|n| n.attribute("bandwidth").and_then(|b| b.parse::<u64>().ok()).unwrap_or(0))
        .ok_or_else(|| anyhow!(crate::i18n::t("mpd_without_representation")))?;

    let template = rep
        .descendants()
        .find(|n| n.has_tag_name("SegmentTemplate"))
        .or_else(|| rep.parent()?.children().find(|n| n.has_tag_name("SegmentTemplate")))
        .ok_or_else(|| anyhow!(crate::i18n::t("mpd_without_segment_template")))?;

    let rep_id = rep.attribute("id").unwrap_or("");
    let bandwidth = rep.attribute("bandwidth").unwrap_or("");
    let fill = |t: &str, number: Option<u64>| -> String {
        let mut s = t.replace("$RepresentationID$", rep_id).replace("$Bandwidth$", bandwidth);
        if let Some(n) = number {
            s = s.replace("$Number$", &n.to_string());
        }
        s
    };

    let init = template.attribute("initialization").ok_or_else(|| anyhow!(crate::i18n::t("no_initialization_segment")))?;
    let media = template.attribute("media").ok_or_else(|| anyhow!(crate::i18n::t("no_media_template")))?;
    let start: u64 = template.attribute("startNumber").and_then(|s| s.parse().ok()).unwrap_or(1);

    let mut count: u64 = 0;
    let timescale: f64 = template.attribute("timescale").and_then(|t| t.parse().ok()).unwrap_or(1.0);
    let mut seg_secs: Vec<f64> = Vec::new();
    if let Some(tl) = template.children().find(|n| n.has_tag_name("SegmentTimeline")) {
        for s in tl.children().filter(|n| n.has_tag_name("S")) {
            let r: i64 = s.attribute("r").and_then(|r| r.parse().ok()).unwrap_or(0);
            let d: f64 = s.attribute("d").and_then(|d| d.parse().ok()).unwrap_or(0.0);
            count += 1 + r.max(0) as u64;
            if count > MAX_SEGMENTS {
                return Err(anyhow!(crate::i18n::text("mpd_has_too_many_segments", &[format!("{}", count)])));
            }
            for _ in 0..=r.max(0) {
                seg_secs.push(d / timescale);
            }
        }
    }
    if count == 0 {
        return Err(anyhow!(crate::i18n::t("mpd_without_segment_timeline")));
    }
    if timescale.is_nan() || timescale <= 0.0 {
        return Err(anyhow!(crate::i18n::t("mpd_with_invalid_timescale")));
    }
    // Audio is only fetched over TLS.
    if !init.starts_with("https://") || !media.starts_with("https://") {
        return Err(anyhow!(crate::i18n::t("mpd_has_non_https_segment_urls")));
    }

    let mut urls = Vec::with_capacity(count as usize + 1);
    urls.push(fill(init, None));
    for n in start..start + count {
        urls.push(fill(media, Some(n)));
    }

    let codec = rep.attribute("codecs").or_else(|| rep.parent()?.attribute("codecs")).unwrap_or("flac").to_string();
    let sample_rate = rep.attribute("audioSamplingRate").and_then(|s| s.parse().ok());
    // TIDAL ids look like "FLAC,96000,24".
    let bit_depth = rep_id.split(',').nth(2).and_then(|b| b.trim().parse().ok());

    Ok(Mpd { urls, seg_secs, codec: if codec.starts_with("mp4a") { "AAC".into() } else { codec.to_uppercase() }, bit_depth, sample_rate })
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_timeline() {
        let xml = r#"<?xml version='1.0'?><MPD xmlns="urn:mpeg:dash:schema:mpd:2011"><Period><AdaptationSet contentType="audio" mimeType="audio/mp4"><Representation id="FLAC,96000,24" codecs="flac" bandwidth="3000000" audioSamplingRate="96000"><SegmentTemplate timescale="96000" initialization="https://x/0.mp4" media="https://x/$Number$.mp4" startNumber="1"><SegmentTimeline><S d="384000" r="3"/><S d="1000"/></SegmentTimeline></SegmentTemplate></Representation></AdaptationSet></Period></MPD>"#;
        let m = super::parse(xml).unwrap();
        assert_eq!(m.urls.len(), 6);
        assert_eq!(m.seg_secs.len(), 5);
        assert_eq!(m.seg_secs[0], 4.0);
        assert_eq!(m.urls[1], "https://x/1.mp4");
        assert_eq!(m.urls[5], "https://x/5.mp4");
        assert_eq!(m.bit_depth, Some(24));
        assert_eq!(m.sample_rate, Some(96000));
    }

    fn mpd(extra: &str, s: &str, url: &str) -> String {
        format!(
            r#"<MPD><Period><AdaptationSet>{extra}<Representation id="FLAC,44100,16" bandwidth="1"><SegmentTemplate timescale="44100" initialization="{url}/0.mp4" media="{url}/$Number$.mp4"><SegmentTimeline>{s}</SegmentTimeline></SegmentTemplate></Representation></AdaptationSet></Period></MPD>"#
        )
    }

    #[test]
    fn refuses_drm() {
        let xml = mpd(r#"<ContentProtection schemeIdUri="urn:mpeg:dash:mp4protection:2011"/>"#, r#"<S d="1"/>"#, "https://x");
        assert!(super::parse(&xml).is_err());
    }

    #[test]
    fn refuses_huge_repeat() {
        // Would otherwise allocate trillions of entries.
        let xml = mpd("", r#"<S d="1" r="999999999999"/>"#, "https://x");
        assert!(super::parse(&xml).is_err());
    }

    #[test]
    fn refuses_plain_http() {
        let xml = mpd("", r#"<S d="1"/>"#, "http://x");
        assert!(super::parse(&xml).is_err());
    }

    #[test]
    fn rejects_garbage() {
        assert!(super::parse("not xml").is_err());
        assert!(super::parse("<MPD/>").is_err());
    }
}
