//! Codex quota reader — port of Services/CodexRateLimitReader.swift.
//!
//! Walks `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` newest-first, scans
//! each file end-to-start, returns the first `token_count` event carrying a
//! non-null `rate_limits` object. Windows are mapped by `window_minutes`
//! (300 → 5h, 10080 → 7d), never by slot name.

use super::{now_epoch, ProviderRateLimit, RateLimitProvider, RateLimitStatus, RateLimitWindow};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

pub fn default_sessions_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".codex")
        .join("sessions")
}

pub fn read() -> ProviderRateLimit {
    read_from(&default_sessions_dir(), now_epoch())
}

pub fn read_from(sessions_dir: &Path, now: f64) -> ProviderRateLimit {
    if !sessions_dir.exists() {
        return ProviderRateLimit::empty(RateLimitProvider::Codex, RateLimitStatus::NoData);
    }

    if let Some(snapshot) = scan_for_latest(sessions_dir, now) {
        // `parse_window` already discarded expired slots. If BOTH slots were
        // dropped there's no live data — collapse to noData rather than render
        // confidently-wrong percentages.
        if snapshot.five_hour.is_none() && snapshot.seven_day.is_none() {
            return ProviderRateLimit::empty(RateLimitProvider::Codex, RateLimitStatus::NoData);
        }
        return ProviderRateLimit {
            provider: RateLimitProvider::Codex,
            five_hour: snapshot.five_hour,
            seven_day: snapshot.seven_day,
            plan_label: snapshot.plan_label,
            status: RateLimitStatus::Ok,
        };
    }
    ProviderRateLimit::empty(RateLimitProvider::Codex, RateLimitStatus::NoData)
}

struct Snapshot {
    five_hour: Option<RateLimitWindow>,
    seven_day: Option<RateLimitWindow>,
    plan_label: Option<String>,
}

/// Year → month → day dirs newest-first; rollout files newest-first by name.
fn scan_for_latest(sessions_dir: &Path, now: f64) -> Option<Snapshot> {
    for year in sorted_subdirs_desc(sessions_dir) {
        for month in sorted_subdirs_desc(&year) {
            for day in sorted_subdirs_desc(&month) {
                let mut files: Vec<PathBuf> = fs::read_dir(&day)
                    .ok()?
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| {
                        p.extension().is_some_and(|e| e == "jsonl")
                            && p.file_name()
                                .and_then(|n| n.to_str())
                                .is_some_and(|n| n.starts_with("rollout-"))
                    })
                    .collect();
                files.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
                for file in files {
                    if let Some(snapshot) = scan_file(&file, now) {
                        return Some(snapshot);
                    }
                }
            }
        }
    }
    None
}

fn sorted_subdirs_desc(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    dirs
}

/// Latest `rate_limits` block in one rollout file (lines scanned in reverse).
fn scan_file(file: &Path, now: f64) -> Option<Snapshot> {
    let raw = fs::read_to_string(file).ok()?;
    for line in raw.lines().rev() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(obj) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let payload = obj.get("payload")?;
        if payload.get("type").and_then(Value::as_str) != Some("token_count") {
            continue;
        }
        let Some(rate_limits) = payload.get("rate_limits").filter(|v| !v.is_null()) else {
            continue;
        };

        // primary/secondary have no fixed semantics — window_minutes decides.
        let mut snapshot = Snapshot {
            five_hour: None,
            seven_day: None,
            plan_label: format_plan_label(rate_limits.get("plan_type").and_then(Value::as_str)),
        };
        for slot in ["primary", "secondary"] {
            let Some((window, minutes)) = parse_window(rate_limits.get(slot), now) else {
                continue;
            };
            match minutes {
                300 => snapshot.five_hour = Some(window),
                10080 => snapshot.seven_day = Some(window),
                _ => {}
            }
        }
        return Some(snapshot);
    }
    None
}

/// Codex emits plan_type lowercase ("free", "plus", "pro", "business") —
/// render the customer-facing capitalized form.
fn format_plan_label(raw: Option<&str>) -> Option<String> {
    let raw = raw?;
    if raw.is_empty() {
        return None;
    }
    let mut chars = raw.chars();
    let first = chars.next()?;
    Some(first.to_uppercase().collect::<String>() + chars.as_str())
}

fn parse_window(raw: Option<&Value>, now: f64) -> Option<(RateLimitWindow, i64)> {
    let dict = raw?.as_object()?;
    let used_percent = dict.get("used_percent")?.as_f64()?;
    let window_minutes = dict.get("window_minutes")?.as_i64()?;

    let mut resets_at: Option<f64> = None;
    if let Some(epoch) = dict.get("resets_at").and_then(Value::as_f64) {
        if epoch > 0.0 {
            resets_at = Some(epoch);
        }
    }
    if resets_at.is_none() {
        if let Some(secs) = dict.get("resets_in_seconds").and_then(Value::as_f64) {
            if secs >= 0.0 {
                resets_at = Some(now + secs);
            }
        }
    }

    // Reject a window whose resets_at is already past: the window has rolled
    // and used_percent belongs to the previous window (strict staleness).
    // Without resets_at at all we keep the window (utilization still
    // meaningful; just no time bar) — matches the Claude reader's tolerance.
    if let Some(at) = resets_at {
        if at - now <= 0.0 {
            return None;
        }
    }

    Some((
        RateLimitWindow {
            utilization: used_percent,
            resets_at,
            window_duration: Some((window_minutes * 60) as f64),
        },
        window_minutes,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    const NOW: f64 = 1_800_000_000.0;

    fn write_rollout(dir: &Path, name: &str, lines: &[&str]) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(name), lines.join("\n")).unwrap();
    }

    fn token_count_line(rate_limits: &str) -> String {
        format!(r#"{{"timestamp":"t","payload":{{"type":"token_count","info":{{}},"rate_limits":{rate_limits}}}}}"#)
    }

    #[test]
    fn no_sessions_dir_reports_no_data() {
        let dir = tempdir().unwrap();
        let r = read_from(&dir.path().join("missing"), NOW);
        assert_eq!(r.status, RateLimitStatus::NoData);
    }

    #[test]
    fn parses_both_windows_and_plan() {
        let dir = tempdir().unwrap();
        let day = dir.path().join("2026").join("07").join("03");
        let rl = format!(
            r#"{{"plan_type":"plus","primary":{{"used_percent":14.0,"window_minutes":300,"resets_at":{}}},"secondary":{{"used_percent":8.0,"window_minutes":10080,"resets_at":{}}}}}"#,
            NOW + 3600.0,
            NOW + 86400.0
        );
        write_rollout(
            &day,
            "rollout-2026-07-03T10-00-00.jsonl",
            &[r#"{"payload":{"type":"session_meta"}}"#, &token_count_line(&rl)],
        );

        let r = read_from(dir.path(), NOW);
        assert_eq!(r.status, RateLimitStatus::Ok);
        assert_eq!(r.plan_label.as_deref(), Some("Plus"));
        let five = r.five_hour.unwrap();
        assert_eq!(five.utilization, 14.0);
        assert_eq!(five.window_duration, Some(300.0 * 60.0));
        let seven = r.seven_day.unwrap();
        assert_eq!(seven.utilization, 8.0);
        assert_eq!(seven.resets_at, Some(NOW + 86400.0));
    }

    #[test]
    fn drops_expired_window_keeps_live_one() {
        let dir = tempdir().unwrap();
        let day = dir.path().join("2026").join("07").join("03");
        let rl = format!(
            r#"{{"plan_type":"pro","primary":{{"used_percent":80.0,"window_minutes":300,"resets_at":{}}},"secondary":{{"used_percent":5.0,"window_minutes":10080,"resets_at":{}}}}}"#,
            NOW - 100.0, // expired 5h
            NOW + 86400.0
        );
        write_rollout(&day, "rollout-a.jsonl", &[&token_count_line(&rl)]);

        let r = read_from(dir.path(), NOW);
        assert_eq!(r.status, RateLimitStatus::Ok);
        assert!(r.five_hour.is_none(), "expired 5h slot must be dropped");
        assert!(r.seven_day.is_some());
    }

    #[test]
    fn fully_expired_snapshot_collapses_to_no_data() {
        let dir = tempdir().unwrap();
        let day = dir.path().join("2026").join("01").join("01");
        let rl = format!(
            r#"{{"primary":{{"used_percent":8.0,"window_minutes":10080,"resets_at":{}}}}}"#,
            NOW - 12.0 * 86400.0
        );
        write_rollout(&day, "rollout-a.jsonl", &[&token_count_line(&rl)]);
        let r = read_from(dir.path(), NOW);
        assert_eq!(r.status, RateLimitStatus::NoData);
    }

    #[test]
    fn resets_in_seconds_fallback() {
        let dir = tempdir().unwrap();
        let day = dir.path().join("2026").join("07").join("03");
        let rl = r#"{"primary":{"used_percent":37.0,"window_minutes":300,"resets_in_seconds":1200}}"#;
        write_rollout(&day, "rollout-a.jsonl", &[&token_count_line(rl)]);
        let r = read_from(dir.path(), NOW);
        let five = r.five_hour.unwrap();
        assert_eq!(five.resets_at, Some(NOW + 1200.0));
    }

    #[test]
    fn picks_newest_day_and_newest_file_and_last_line() {
        let dir = tempdir().unwrap();
        let old_day = dir.path().join("2026").join("07").join("02");
        let new_day = dir.path().join("2026").join("07").join("03");
        let rl_old = format!(
            r#"{{"primary":{{"used_percent":1.0,"window_minutes":10080,"resets_at":{}}}}}"#,
            NOW + 999.0
        );
        let rl_mid = format!(
            r#"{{"primary":{{"used_percent":2.0,"window_minutes":10080,"resets_at":{}}}}}"#,
            NOW + 999.0
        );
        let rl_new = format!(
            r#"{{"primary":{{"used_percent":3.0,"window_minutes":10080,"resets_at":{}}}}}"#,
            NOW + 999.0
        );
        write_rollout(&old_day, "rollout-a.jsonl", &[&token_count_line(&rl_old)]);
        write_rollout(
            &new_day,
            "rollout-b.jsonl",
            &[&token_count_line(&rl_mid), &token_count_line(&rl_new)],
        );
        let r = read_from(dir.path(), NOW);
        assert_eq!(r.seven_day.unwrap().utilization, 3.0);
    }

    #[test]
    fn skips_files_without_rate_limits() {
        let dir = tempdir().unwrap();
        let day = dir.path().join("2026").join("07").join("03");
        write_rollout(
            &day,
            "rollout-z-no-limits.jsonl",
            &[r#"{"payload":{"type":"token_count","rate_limits":null}}"#],
        );
        let rl = format!(
            r#"{{"primary":{{"used_percent":42.0,"window_minutes":10080,"resets_at":{}}}}}"#,
            NOW + 999.0
        );
        write_rollout(&day, "rollout-a-has-limits.jsonl", &[&token_count_line(&rl)]);
        let r = read_from(dir.path(), NOW);
        assert_eq!(r.seven_day.unwrap().utilization, 42.0);
    }
}
