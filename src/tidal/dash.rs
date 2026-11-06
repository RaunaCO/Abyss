//! Minimal MPEG-DASH (MPD) parser for TIDAL's audio manifests: one period,
//! one audio representation, `SegmentTemplate` with a `SegmentTimeline`.

use anyhow::{anyhow, Result};

pub struct Mpd {
    /// Initialization segment first, then every media segment in order.
    pub urls: Vec<String>,
    pub codec: String,
    pub bit_depth: Option<u32>,
    pub sample_rate: Option<u32>,
}

pub fn parse(xml: &str) -> Result<Mpd> {
    let doc = roxmltree::Document::parse(xml)?;
    // Pick the representation with the highest bandwidth.
    let rep = doc
        .descendants()
        .filter(|n| n.has_tag_name("Representation"))
        .max_by_key(|n| n.attribute("bandwidth").and_then(|b| b.parse::<u64>().ok()).unwrap_or(0))
        .ok_or_else(|| anyhow!("MPD without Representation"))?;

    let template = rep
        .descendants()
        .find(|n| n.has_tag_name("SegmentTemplate"))
        .or_else(|| {
            rep.parent()?.children().find(|n| n.has_tag_name("SegmentTemplate"))
        })
        .ok_or_else(|| anyhow!("MPD without SegmentTemplate"))?;

    let rep_id = rep.attribute("id").unwrap_or("");
    let bandwidth = rep.attribute("bandwidth").unwrap_or("");
    let fill = |t: &str, number: Option<u64>| -> String {
        let mut s = t.replace("$RepresentationID$", rep_id).replace("$Bandwidth$", bandwidth);
        if let Some(n) = number {
            s = s.replace("$Number$", &n.to_string());
        }
        s
    };

    let init = template.attribute("initialization").ok_or_else(|| anyhow!("no init segment"))?;
    let media = template.attribute("media").ok_or_else(|| anyhow!("no media template"))?;
    let start: u64 = template.attribute("startNumber").and_then(|s| s.parse().ok()).unwrap_or(1);

    let mut count: u64 = 0;
    if let Some(tl) = template.children().find(|n| n.has_tag_name("SegmentTimeline")) {
        for s in tl.children().filter(|n| n.has_tag_name("S")) {
            let r: i64 = s.attribute("r").and_then(|r| r.parse().ok()).unwrap_or(0);
            count += 1 + r.max(0) as u64;
        }
    }
    if count == 0 {
        return Err(anyhow!("MPD without segment timeline"));
    }

    let mut urls = Vec::with_capacity(count as usize + 1);
    urls.push(fill(init, None));
    for n in start..start + count {
        urls.push(fill(media, Some(n)));
    }

    let codec = rep
        .attribute("codecs")
        .or_else(|| rep.parent()?.attribute("codecs"))
        .unwrap_or("flac")
        .to_string();
    let sample_rate = rep.attribute("audioSamplingRate").and_then(|s| s.parse().ok());
    // TIDAL ids look like "FLAC,96000,24".
    let bit_depth = rep_id.split(',').nth(2).and_then(|b| b.trim().parse().ok());

    Ok(Mpd {
        urls,
        codec: if codec.starts_with("mp4a") { "AAC".into() } else { codec.to_uppercase() },
        bit_depth,
        sample_rate,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_timeline() {
        let xml = r#"<?xml version='1.0'?><MPD xmlns="urn:mpeg:dash:schema:mpd:2011"><Period><AdaptationSet contentType="audio" mimeType="audio/mp4"><Representation id="FLAC,96000,24" codecs="flac" bandwidth="3000000" audioSamplingRate="96000"><SegmentTemplate timescale="96000" initialization="https://x/0.mp4" media="https://x/$Number$.mp4" startNumber="1"><SegmentTimeline><S d="384000" r="3"/><S d="1000"/></SegmentTimeline></SegmentTemplate></Representation></AdaptationSet></Period></MPD>"#;
        let m = super::parse(xml).unwrap();
        assert_eq!(m.urls.len(), 6);
        assert_eq!(m.urls[1], "https://x/1.mp4");
        assert_eq!(m.urls[5], "https://x/5.mp4");
        assert_eq!(m.bit_depth, Some(24));
        assert_eq!(m.sample_rate, Some(96000));
    }
}
