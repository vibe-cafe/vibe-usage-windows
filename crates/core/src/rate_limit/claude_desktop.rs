//! Read-only Claude Desktop quota observations. No credentials, prompts,
//! network requests, subprocesses, or writes to Claude.
use super::{ProviderRateLimit, RateLimitMeter, RateLimitProvider, RateLimitStatus};
use crate::quota_product::DiscoveryEnvironment;
use serde_json::Value;
use std::{fs::File, io::Read, path::PathBuf};
const MAX_BYTES: u64 = 2 * 1024 * 1024;
const MAX_AGE: f64 = 24.0 * 3600.0;

pub fn data_dirs(env: &DiscoveryEnvironment) -> Vec<PathBuf> {
    if !env.windows {
        return vec![];
    }
    let mut dirs = vec![env
        .app_data
        .clone()
        .unwrap_or_else(|| env.home.join("AppData/Roaming"))
        .join("Claude")];
    let local = env
        .local_app_data
        .clone()
        .unwrap_or_else(|| env.home.join("AppData/Local"));
    if let Ok(entries) = std::fs::read_dir(local.join("Packages")) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(suffix) = name.strip_prefix("Claude_") else {
                continue;
            };
            if suffix.is_empty() || !suffix.bytes().all(|b| b.is_ascii_alphanumeric()) {
                continue;
            }
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                dirs.push(entry.path().join("LocalCache/Roaming/Claude"));
            }
        }
    }
    dirs.retain(|p| p.is_dir());
    dirs.sort();
    dirs.dedup();
    dirs
}

/// Some(noData) means Desktop history exists but is stale or ambiguous.
/// Do not silently replace this with a different Claude Code account.
pub fn read(env: &DiscoveryEnvironment, now: f64) -> Option<ProviderRateLimit> {
    let paths: Vec<_> = data_dirs(env)
        .into_iter()
        .map(|p| p.join("plan-usage-history.json"))
        .filter(|p| p.is_file())
        .collect();
    if paths.is_empty() {
        return None;
    }
    let mut empty =
        ProviderRateLimit::empty(RateLimitProvider::ClaudeCode, RateLimitStatus::NoData);
    empty.source_label = Some("Claude 桌面额度记录 · 非本机独占用量".into());
    // Multiple installations may have different accounts. No guessing.
    if paths.len() != 1 {
        return Some(empty);
    }
    let result = (|| {
        let file = File::open(&paths[0]).ok()?;
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1).read_to_end(&mut bytes).ok()?;
        if bytes.len() as u64 > MAX_BYTES {
            return None;
        }
        parse(&bytes, now)
    })();
    Some(result.unwrap_or(empty))
}

pub fn parse(bytes: &[u8], now: f64) -> Option<ProviderRateLimit> {
    let root: Value = serde_json::from_slice(bytes).ok()?;
    if root.get("version")?.as_u64()? != 2 {
        return None;
    }
    let samples = root.get("samples")?.as_array()?;
    let mut org: Option<&str> = None;
    for sample in samples {
        let candidate = sample.get("org")?.as_str().filter(|s| !s.is_empty())?;
        if org.is_some_and(|previous| previous != candidate) {
            return None;
        }
        org = Some(candidate);
    }
    let latest = samples
        .iter()
        .filter_map(|s| Some((s.get("t")?.as_f64()?, s)))
        .max_by(|a, b| a.0.total_cmp(&b.0))?;
    let as_of = latest.0 / 1000.0;
    if !now.is_finite() || !(0.0..=MAX_AGE).contains(&(now - as_of)) {
        return None;
    }
    let usage = latest.1.get("u")?.as_object()?;
    let mut meters = vec![];
    for (key, label) in [
        ("fh", "5h"),
        ("sd", "7d"),
        ("so", "Opus"),
        ("sn", "Sonnet"),
        ("cw", "Cowork"),
        ("oa", "OAuth apps"),
    ] {
        let Some(value) = usage.get(key) else {
            continue;
        };
        let utilization = value.as_f64()?;
        if !(0.0..=100.0).contains(&utilization) {
            return None;
        }
        meters.push(RateLimitMeter {
            id: format!("desktop-{key}"),
            label: label.into(),
            utilization,
            resets_at: None,
            window_duration: None,
        });
    }
    if meters.is_empty() {
        return None;
    }
    let mut snapshot = ProviderRateLimit::empty(RateLimitProvider::ClaudeCode, RateLimitStatus::Ok);
    snapshot.meters = meters;
    snapshot.data_as_of = Some(as_of);
    snapshot.fetched_at = Some(now);
    snapshot.source_label = Some(
        if now - as_of > 900.0 {
            "Claude 桌面历史额度 · 非实时 · 非本机独占用量"
        } else {
            "Claude 桌面额度记录 · 非本机独占用量"
        }
        .into(),
    );
    Some(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const NOW: f64 = 1_800_000_000.0;
    fn sample(at: f64, org: &str, n: f64) -> Value {
        json!({"t":at*1000.0,"org":org,"u":{"fh":n,"sd":13}})
    }
    fn decode(samples: Vec<Value>) -> Option<ProviderRateLimit> {
        parse(
            &serde_json::to_vec(&json!({"version":2,"samples":samples})).unwrap(),
            NOW,
        )
    }
    #[test]
    fn newest_observation_is_not_a_sum_and_has_no_invented_reset() {
        let s = decode(vec![
            sample(NOW, "one", 28.0),
            sample(NOW - 300.0, "one", 24.0),
        ])
        .unwrap();
        assert_eq!(s.meters[0].utilization, 28.0);
        assert_eq!(s.data_as_of, Some(NOW));
        assert!(s
            .meters
            .iter()
            .all(|m| m.resets_at.is_none() && m.window_duration.is_none()));
    }
    #[test]
    fn old_observation_is_explicitly_historical() {
        let s = decode(vec![sample(NOW - 3600.0, "one", 28.0)]).unwrap();
        assert_eq!(s.data_as_of, Some(NOW - 3600.0));
        assert!(s.source_label.unwrap().contains("非实时"));
    }
    #[test]
    fn stale_future_invalid_and_cross_account_records_are_rejected() {
        for values in [
            vec![sample(NOW - MAX_AGE - 1.0, "one", 28.0)],
            vec![sample(NOW + 1.0, "one", 28.0)],
            vec![sample(NOW, "one", -1.0)],
            vec![sample(NOW, "one", 101.0)],
            vec![sample(NOW, "one", 28.0), sample(NOW - 1.0, "two", 70.0)],
            vec![],
        ] {
            assert!(decode(values).is_none());
        }
        assert!(parse(b"broken", NOW).is_none());
        assert!(parse(br#"{"version":1,"samples":[]}"#, NOW).is_none());
        assert!(decode(vec![
            sample(NOW - 10.0, "one", 28.0),
            json!({"t":NOW*1000.0,"org":"one","u":{}})
        ])
        .is_none());
    }
    #[test]
    fn packaged_directory_is_detected_and_source_file_is_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let local = dir.path().join("Local");
        let desktop = local.join("Packages/Claude_abc123/LocalCache/Roaming/Claude");
        std::fs::create_dir_all(&desktop).unwrap();
        let file = desktop.join("plan-usage-history.json");
        let bytes =
            serde_json::to_vec(&json!({"version":2,"samples":[sample(NOW,"one",28.0)]})).unwrap();
        std::fs::write(&file, &bytes).unwrap();
        let env = DiscoveryEnvironment {
            home: dir.path().into(),
            path_directories: vec![],
            path_extensions: vec![],
            app_data: None,
            local_app_data: Some(local),
            program_files: vec![],
            windows: true,
        };
        assert_eq!(data_dirs(&env), vec![desktop]);
        assert_eq!(read(&env, NOW).unwrap().meters[0].utilization, 28.0);
        assert_eq!(std::fs::read(&file).unwrap(), bytes);
        assert_eq!(
            read(&env, NOW + MAX_AGE + 1.0).unwrap().status,
            RateLimitStatus::NoData
        );
        assert!(crate::quota_product::discover_with(&env)
            .iter()
            .any(|p| p.provider == RateLimitProvider::ClaudeCode && p.is_detected));
    }
}
