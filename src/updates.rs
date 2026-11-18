//! "Check for updates" in Settings > About: asks GitHub for the latest published
//! release, only when the user clicks. Listed in docs/PRIVACY.md.

/// The version of the latest published (non-draft) release on GitHub.
pub fn latest_release() -> anyhow::Result<String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("Abyss/", env!("CARGO_PKG_VERSION"), " (https://github.com/RaunaCO/Abyss)"))
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let body = client
        .get("https://api.github.com/repos/RaunaCO/Abyss/releases/latest")
        .header("Accept", "application/vnd.github+json")
        .send()?
        .error_for_status()?
        .text()?;
    let v: serde_json::Value = serde_json::from_str(&body)?;
    let tag = v.get("tag_name").and_then(serde_json::Value::as_str).ok_or_else(|| anyhow::anyhow!("no tag_name"))?;
    Ok(tag.trim().trim_start_matches(['v', 'V']).to_string())
}

/// `remote` is a higher x.y.z than `local` (missing parts count as 0).
pub fn is_newer(remote: &str, local: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> { v.split(['.', '-', '+']).take(3).map(|p| p.parse().unwrap_or(0)).collect() };
    let (mut r, mut l) = (parse(remote), parse(local));
    r.resize(3, 0);
    l.resize(3, 0);
    r > l
}

#[cfg(test)]
mod update_tests {
    #[test]
    fn version_comparison() {
        assert!(super::is_newer("1.5.0", "1.4.1"));
        assert!(super::is_newer("1.10.0", "1.9.9"));
        assert!(!super::is_newer("1.2.0", "1.4.1"));
        assert!(!super::is_newer("1.4.1", "1.4.1"));
        assert!(super::is_newer("2", "1.9"));
    }
}
