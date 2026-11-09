//! Read-only Claude Desktop quota observations. No credentials, prompts,
//! network requests, subprocesses, or writes to Claude.
use super::{ProviderRateLimit, RateLimitMeter, RateLimitProvider, RateLimitStatus};
use crate::quota_product::DiscoveryEnvironment;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::time::SystemTime;
use std::{fs::File, io::Read, path::PathBuf};
const MAX_BYTES: u64 = 2 * 1024 * 1024;
const MAX_AGE: f64 = 24.0 * 3600.0;
const RECORD: &str = "plan-usage-history.json";

/// One place a Claude Desktop installation keeps `plan-usage-history.json`.
struct Location {
    /// Which installation this is: the classic installer's `%APPDATA%\Claude`,
    /// or one MSIX package identified by its (lower-cased) publisher suffix.
    install: String,
    path: PathBuf,
    /// The MSIX package's current data directory, preferred over its older
    /// virtualized one when both records are equally new.
    current: bool,
}

/// The `<suffix>` of a `Claude_<suffix>` / `claude_<suffix>` directory name.
/// Case-insensitive: the package directory is `Claude_…` while Claude
/// Desktop's newer data directory is `claude_…`.
fn package_suffix(name: &str) -> Option<String> {
    let prefix = name.get(..7)?;
    let suffix = &name[7..];
    if !prefix.eq_ignore_ascii_case("claude_")
        || suffix.is_empty()
        || !suffix.bytes().all(|b| b.is_ascii_alphanumeric())
    {
        return None;
    }
    Some(suffix.to_ascii_lowercase())
}

/// Every existing Claude Desktop data directory:
///
/// - `%APPDATA%\Claude` — the classic (non-MSIX) installer;
/// - `%LOCALAPPDATA%\Packages\Claude_<suffix>\LocalCache\Roaming\Claude` —
///   the MSIX package's virtualized `%APPDATA%`, used by older releases;
/// - `%LOCALAPPDATA%\Claude-Data\claude_<suffix>` — where the MSIX package
///   writes after a Claude Desktop update in 2026-11 (vibe-usage-windows#27),
///   with the record directly in that directory.
fn locations(env: &DiscoveryEnvironment) -> Vec<Location> {
    if !env.windows {
        return vec![];
    }
    let mut found = vec![Location {
        install: "classic".into(),
        path: env
            .app_data
            .clone()
            .unwrap_or_else(|| env.home.join("AppData/Roaming"))
            .join("Claude"),
        current: false,
    }];
    let local = env
        .local_app_data
        .clone()
        .unwrap_or_else(|| env.home.join("AppData/Local"));
    for (parent, current) in [
        (local.join("Packages"), false),
        (local.join("Claude-Data"), true),
    ] {
        let Ok(entries) = std::fs::read_dir(&parent) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(suffix) = package_suffix(&name) else {
                continue;
            };
            if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let path = if current {
                entry.path()
            } else {
                entry.path().join("LocalCache/Roaming/Claude")
            };
            found.push(Location {
                install: format!("package:{suffix}"),
                path,
                current,
            });
        }
    }
    found.retain(|location| location.path.is_dir());
    found
}

pub fn data_dirs(env: &DiscoveryEnvironment) -> Vec<PathBuf> {
    let mut dirs: Vec<_> = locations(env).into_iter().map(|l| l.path).collect();
    dirs.sort();
    dirs.dedup();
    dirs
}

/// One record per installation. The same file reached through a directory
/// link (a user's workaround that points the old location at the new one)
/// counts once; an installation with records in both its old and its current
/// directory (left over from the migration) contributes the newer one.
fn record_files(env: &DiscoveryEnvironment) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut newest: BTreeMap<String, ((Option<SystemTime>, bool), PathBuf)> = BTreeMap::new();
    for location in locations(env) {
        let file = location.path.join(RECORD);
        if !file.is_file() {
            continue;
        }
        let canonical = std::fs::canonicalize(&file).unwrap_or_else(|_| file.clone());
        if !seen.insert(canonical.to_string_lossy().to_lowercase()) {
            continue;
        }
        // Newest modification time wins; an unknown time loses to a known
        // one, and a tie goes to the package's current directory.
        let rank = (
            std::fs::metadata(&file).and_then(|m| m.modified()).ok(),
            location.current,
        );
        match newest.get(&location.install) {
            Some((best, _)) if *best >= rank => {}
            _ => {
                newest.insert(location.install, (rank, file));
            }
        }
    }
    newest.into_values().map(|(_, file)| file).collect()
}

/// Some(noData) means Desktop history exists but is stale or ambiguous.
/// Do not silently replace this with a different Claude Code account.
pub fn read(env: &DiscoveryEnvironment, now: f64) -> Option<ProviderRateLimit> {
    let paths = record_files(env);
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
        // A window the account does not have may be written as `null` (verified
        // Ultra accounts have no 5h window since 2026-10-30). That is absence,
        // not a malformed record: skip the meter instead of discarding the
        // other windows. Non-null non-numbers are still rejected below.
        let Some(value) = usage.get(key).filter(|v| !v.is_null()) else {
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
    fn null_window_is_skipped_without_discarding_the_others() {
        // Verified Ultra: no 5h window.
        let s = decode(vec![
            json!({"t":NOW*1000.0,"org":"one","u":{"fh":null,"sd":47}}),
        ])
        .unwrap();
        assert_eq!(s.status, RateLimitStatus::Ok);
        assert_eq!(s.meters.len(), 1);
        assert_eq!(s.meters[0].label, "7d");
        assert_eq!(s.meters[0].utilization, 47.0);

        // Optional model windows may be null too; zero stays a real reading.
        let s = decode(vec![json!({
            "t":NOW*1000.0,"org":"one","u":{"fh":0,"sd":13,"so":null,"sn":null,"cw":null}
        })])
        .unwrap();
        let labels: Vec<_> = s.meters.iter().map(|m| m.label.as_str()).collect();
        assert_eq!(labels, ["5h", "7d"]);
        assert_eq!(s.meters[0].utilization, 0.0);
    }
    #[test]
    fn all_null_or_non_numeric_windows_still_yield_no_snapshot() {
        assert!(decode(vec![
            json!({"t":NOW*1000.0,"org":"one","u":{"fh":null,"sd":null}})
        ])
        .is_none());
        assert!(decode(vec![
            json!({"t":NOW*1000.0,"org":"one","u":{"fh":"12","sd":13}})
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

    mod locations {
        use super::*;
        use std::path::Path;
        use std::time::{Duration, SystemTime};

        const SUFFIX: &str = "pzs8sxrjxfjjc";

        fn env(root: &Path) -> DiscoveryEnvironment {
            DiscoveryEnvironment {
                home: root.into(),
                path_directories: vec![],
                path_extensions: vec![],
                app_data: Some(root.join("Roaming")),
                local_app_data: Some(root.join("Local")),
                program_files: vec![],
                windows: true,
            }
        }

        /// `%LOCALAPPDATA%\Packages\Claude_<suffix>\LocalCache\Roaming\Claude`
        fn packaged(root: &Path, suffix: &str) -> PathBuf {
            root.join("Local/Packages")
                .join(format!("Claude_{suffix}"))
                .join("LocalCache/Roaming/Claude")
        }

        /// `%LOCALAPPDATA%\Claude-Data\claude_<suffix>` (record directly inside).
        fn current(root: &Path, suffix: &str) -> PathBuf {
            root.join("Local/Claude-Data")
                .join(format!("claude_{suffix}"))
        }

        fn write_record(dir: &Path, five_hour: f64, modified: SystemTime) {
            std::fs::create_dir_all(dir).unwrap();
            let file = dir.join(RECORD);
            let bytes =
                serde_json::to_vec(&json!({"version":2,"samples":[sample(NOW, "one", five_hour)]}))
                    .unwrap();
            std::fs::write(&file, bytes).unwrap();
            File::options()
                .write(true)
                .open(&file)
                .unwrap()
                .set_modified(modified)
                .unwrap();
        }

        fn five_hour(snapshot: &ProviderRateLimit) -> f64 {
            assert_eq!(snapshot.status, RateLimitStatus::Ok);
            snapshot.meters[0].utilization
        }

        #[test]
        fn record_in_the_new_data_directory_is_read() {
            let dir = tempfile::tempdir().unwrap();
            let env = env(dir.path());
            write_record(&current(dir.path(), SUFFIX), 28.0, SystemTime::now());
            assert_eq!(five_hour(&read(&env, NOW).unwrap()), 28.0);
            assert_eq!(data_dirs(&env), vec![current(dir.path(), SUFFIX)]);
            assert!(crate::quota_product::discover_with(&env)
                .iter()
                .any(|p| p.provider == RateLimitProvider::ClaudeCode && p.is_detected));
        }

        #[test]
        fn old_and_new_directory_of_one_package_are_one_installation() {
            let dir = tempfile::tempdir().unwrap();
            let env = env(dir.path());
            let now = SystemTime::now();
            let hour_ago = now - Duration::from_secs(3600);
            // Left over from before the migration, then the current record.
            write_record(&packaged(dir.path(), SUFFIX), 10.0, hour_ago);
            write_record(&current(dir.path(), SUFFIX), 28.0, now);
            assert_eq!(five_hour(&read(&env, NOW).unwrap()), 28.0);

            // "Newer" is the file's own modification time, not its location.
            write_record(&packaged(dir.path(), SUFFIX), 10.0, now);
            write_record(&current(dir.path(), SUFFIX), 28.0, hour_ago);
            assert_eq!(five_hour(&read(&env, NOW).unwrap()), 10.0);

            // Equally new: the package's current directory wins.
            write_record(&packaged(dir.path(), SUFFIX), 10.0, now);
            write_record(&current(dir.path(), SUFFIX), 28.0, now);
            assert_eq!(five_hour(&read(&env, NOW).unwrap()), 28.0);
        }

        #[test]
        fn different_packages_or_installers_are_still_ambiguous() {
            let now = SystemTime::now();
            let dir = tempfile::tempdir().unwrap();
            write_record(&packaged(dir.path(), "aaa111"), 10.0, now);
            write_record(&current(dir.path(), "bbb222"), 28.0, now);
            let snapshot = read(&env(dir.path()), NOW).unwrap();
            assert_eq!(snapshot.status, RateLimitStatus::NoData);
            assert!(snapshot.meters.is_empty());

            let dir = tempfile::tempdir().unwrap();
            write_record(&dir.path().join("Roaming/Claude"), 10.0, now);
            write_record(&current(dir.path(), SUFFIX), 28.0, now);
            assert_eq!(
                read(&env(dir.path()), NOW).unwrap().status,
                RateLimitStatus::NoData
            );
        }

        #[test]
        fn old_location_linked_to_the_new_one_counts_once() {
            let dir = tempfile::tempdir().unwrap();
            let env = env(dir.path());
            let target = current(dir.path(), SUFFIX);
            write_record(&target, 28.0, SystemTime::now());
            let link = packaged(dir.path(), SUFFIX);
            std::fs::create_dir_all(link.parent().unwrap()).unwrap();
            #[cfg(unix)]
            let linked = std::os::unix::fs::symlink(&target, &link);
            #[cfg(windows)]
            let linked = std::os::windows::fs::symlink_dir(&target, &link);
            if linked.is_err() {
                // Creating a link can need a privilege the runner lacks.
                eprintln!("skipped: could not create a directory link");
                return;
            }
            assert_eq!(data_dirs(&env).len(), 2);
            assert_eq!(five_hour(&read(&env, NOW).unwrap()), 28.0);
        }

        #[test]
        fn package_directories_match_case_insensitively_with_a_plain_suffix() {
            assert_eq!(
                package_suffix("claude_pzs8sxrjxfjjc").as_deref(),
                Some(SUFFIX)
            );
            assert_eq!(
                package_suffix("Claude_pzs8sxrjxfjjc").as_deref(),
                Some(SUFFIX)
            );
            assert_eq!(
                package_suffix("CLAUDE_PZS8SXRJXFJJC").as_deref(),
                Some(SUFFIX)
            );
            for name in [
                "claude_",
                "claude_abc-def",
                "claude_abc.def",
                "claudeabc",
                "Claude",
                "爱claude_abc",
            ] {
                assert_eq!(package_suffix(name), None, "{name}");
            }

            let dir = tempfile::tempdir().unwrap();
            let env = env(dir.path());
            let data = dir.path().join("Local/Claude-Data");
            // Not a package directory, or not a directory at all.
            std::fs::create_dir_all(data.join("claude_abc-def")).unwrap();
            std::fs::write(data.join("claude_zzz"), b"").unwrap();
            assert!(data_dirs(&env).is_empty());
            assert!(read(&env, NOW).is_none());
        }
    }
}
