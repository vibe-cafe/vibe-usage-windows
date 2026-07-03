//! Semver comparison for the update checker.

pub fn parse(version: &str) -> (u64, u64, u64) {
    let clean = version.trim().trim_start_matches('v');
    let core = clean.split(['-', '+']).next().unwrap_or("");
    let mut parts = core.split('.');
    let p = |s: Option<&str>| s.and_then(|x| x.parse().ok()).unwrap_or(0u64);
    (p(parts.next()), p(parts.next()), p(parts.next()))
}

/// True when `candidate` is strictly newer than `current`.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    parse(candidate) > parse(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("0.5.2", "0.5.1"));
        assert!(is_newer("0.6.0", "0.5.9"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.5.1", "0.5.1"));
        assert!(!is_newer("0.5.0", "0.5.1"));
        assert!(is_newer("v0.5.2", "0.5.1"));
        assert!(is_newer("0.5.2-beta", "0.5.1"));
    }
}
