//! Claude quota reader — port of Services/ClaudeRateLimitReader.swift.
//!
//! Reads the local capture file written by our statusline wrapper (see
//! `statusline_hook`). No network, no OAuth, no keychain.
//!
//! Capture file shape:
//! ```json
//! {
//!   "five_hour":  { "used_percentage": 37.0, "resets_at": 1778950000 },
//!   "seven_day":  { "used_percentage": 64.0, "resets_at": 1779400000 },
//!   "model_id":   "claude-opus-4-7",
//!   "captured_at": 1778938491
//! }
//! ```
//! `resets_at` may also arrive as an ISO-8601 string on some Claude versions.

use super::{now_epoch, ProviderRateLimit, RateLimitProvider, RateLimitStatus, RateLimitWindow};
use serde_json::Value;
use std::path::Path;

const FIVE_HOUR_DURATION: f64 = 5.0 * 3600.0;
const SEVEN_DAY_DURATION: f64 = 7.0 * 86400.0;

pub fn read_from(capture_file: &Path, enabled: bool, now: f64) -> ProviderRateLimit {
    if !enabled {
        return ProviderRateLimit::empty(RateLimitProvider::ClaudeCode, RateLimitStatus::Disabled);
    }
    if !capture_file.exists() {
        // No capture yet: hook not installed, or Claude Code hasn't rendered a
        // statusline since install. `.disabled` keeps the enable affordance.
        return ProviderRateLimit::empty(RateLimitProvider::ClaudeCode, RateLimitStatus::Disabled);
    }

    let Ok(raw) = std::fs::read_to_string(capture_file) else {
        return ProviderRateLimit::empty(
            RateLimitProvider::ClaudeCode,
            RateLimitStatus::Error {
                message: "无法读取限额缓存".into(),
            },
        );
    };
    let Ok(obj) = serde_json::from_str::<Value>(&raw) else {
        return ProviderRateLimit::empty(
            RateLimitProvider::ClaudeCode,
            RateLimitStatus::Error {
                message: "限额缓存格式错误".into(),
            },
        );
    };

    let five_hour = parse_window(obj.get("five_hour"), FIVE_HOUR_DURATION, now);
    let seven_day = parse_window(obj.get("seven_day"), SEVEN_DAY_DURATION, now);

    if five_hour.is_none() && seven_day.is_none() {
        // File exists but no usable windows — e.g. API/Bedrock session with no
        // subscription limits. Collapse like Codex noData.
        return ProviderRateLimit::empty(RateLimitProvider::ClaudeCode, RateLimitStatus::NoData);
    }

    ProviderRateLimit {
        provider: RateLimitProvider::ClaudeCode,
        five_hour,
        seven_day,
        // Can't distinguish Pro vs Max from this payload — leave nil.
        plan_label: None,
        status: RateLimitStatus::Ok,
    }
}

pub fn read(capture_file: &Path, enabled: bool) -> ProviderRateLimit {
    read_from(capture_file, enabled, now_epoch())
}

/// Tolerant of both `used_percentage` and legacy `utilization`, and of
/// `resets_at` as epoch Number or ISO-8601 String.
///
/// Lenient staleness (unlike Codex): a past `resets_at` still shows the
/// utilization; we only drop `window_duration` so the time bar disappears
/// instead of pinning to a wrong 100%.
fn parse_window(raw: Option<&Value>, duration: f64, now: f64) -> Option<RateLimitWindow> {
    let dict = raw?.as_object()?;

    let utilization = dict
        .get("used_percentage")
        .and_then(Value::as_f64)
        .or_else(|| dict.get("utilization").and_then(Value::as_f64))?;

    let mut resets_at: Option<f64> = None;
    match dict.get("resets_at") {
        Some(Value::Number(n)) => {
            if let Some(secs) = n.as_f64() {
                if secs > 0.0 {
                    resets_at = Some(secs);
                }
            }
        }
        Some(Value::String(s)) if !s.is_empty() => {
            resets_at = parse_iso8601_epoch(s);
        }
        _ => {}
    }

    let reset_in_future = resets_at.map(|at| at - now > 0.0).unwrap_or(false);
    Some(RateLimitWindow {
        utilization,
        resets_at,
        window_duration: if reset_in_future { Some(duration) } else { None },
    })
}

/// Minimal ISO-8601/RFC-3339 → epoch seconds ("2026-07-03T10:00:00Z",
/// fractional seconds and ±hh:mm offsets supported). Avoids a chrono dep.
pub(crate) fn parse_iso8601_epoch(s: &str) -> Option<f64> {
    let s = s.trim();
    let bytes = s.as_bytes();
    if bytes.len() < 19 || bytes[4] != b'-' || bytes[7] != b'-' || (bytes[10] != b'T' && bytes[10] != b't' && bytes[10] != b' ') {
        return None;
    }
    let year: i64 = s.get(0..4)?.parse().ok()?;
    let month: i64 = s.get(5..7)?.parse().ok()?;
    let day: i64 = s.get(8..10)?.parse().ok()?;
    let hour: i64 = s.get(11..13)?.parse().ok()?;
    let minute: i64 = s.get(14..16)?.parse().ok()?;
    let second: i64 = s.get(17..19)?.parse().ok()?;

    let mut rest = &s[19..];
    let mut frac = 0.0;
    if rest.starts_with('.') {
        let end = rest[1..]
            .find(|c: char| !c.is_ascii_digit())
            .map(|i| i + 1)
            .unwrap_or(rest.len());
        frac = format!("0{}", &rest[..end]).parse().unwrap_or(0.0);
        rest = &rest[end..];
    }
    let offset_secs: i64 = if rest.is_empty() || rest == "Z" || rest == "z" {
        0
    } else {
        let sign = match rest.as_bytes()[0] {
            b'+' => 1,
            b'-' => -1,
            _ => return None,
        };
        let oh: i64 = rest.get(1..3)?.parse().ok()?;
        let om: i64 = if rest.len() >= 6 {
            rest.get(4..6)?.parse().ok()?
        } else {
            0
        };
        sign * (oh * 3600 + om * 60)
    };

    // days since epoch (civil algorithm, Howard Hinnant)
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;

    Some((days * 86400 + hour * 3600 + minute * 60 + second - offset_secs) as f64 + frac)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const NOW: f64 = 1_800_000_000.0;

    fn write_capture(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempdir().unwrap();
        let file = dir.path().join("claude-rate-limits.json");
        std::fs::write(&file, content).unwrap();
        (dir, file)
    }

    #[test]
    fn disabled_when_not_enabled() {
        let (_d, f) = write_capture("{}");
        assert_eq!(read_from(&f, false, NOW).status, RateLimitStatus::Disabled);
    }

    #[test]
    fn disabled_when_file_missing() {
        let dir = tempdir().unwrap();
        let r = read_from(&dir.path().join("nope.json"), true, NOW);
        assert_eq!(r.status, RateLimitStatus::Disabled);
    }

    #[test]
    fn parses_both_windows() {
        let (_d, f) = write_capture(&format!(
            r#"{{"five_hour":{{"used_percentage":37.0,"resets_at":{}}},"seven_day":{{"used_percentage":64.0,"resets_at":{}}},"captured_at":{NOW}}}"#,
            NOW + 3000.0,
            NOW + 400000.0
        ));
        let r = read_from(&f, true, NOW);
        assert_eq!(r.status, RateLimitStatus::Ok);
        let five = r.five_hour.unwrap();
        assert_eq!(five.utilization, 37.0);
        assert_eq!(five.window_duration, Some(5.0 * 3600.0));
        assert_eq!(r.seven_day.unwrap().utilization, 64.0);
        assert!(r.plan_label.is_none());
    }

    #[test]
    fn stale_reset_drops_time_bar_but_keeps_utilization() {
        let (_d, f) = write_capture(&format!(
            r#"{{"five_hour":{{"used_percentage":88.0,"resets_at":{}}}}}"#,
            NOW - 60.0
        ));
        let r = read_from(&f, true, NOW);
        assert_eq!(r.status, RateLimitStatus::Ok);
        let five = r.five_hour.unwrap();
        assert_eq!(five.utilization, 88.0);
        assert!(five.window_duration.is_none(), "no time bar for stale reset");
    }

    #[test]
    fn legacy_utilization_key_accepted() {
        let (_d, f) = write_capture(r#"{"seven_day":{"utilization":12.5}}"#);
        let r = read_from(&f, true, NOW);
        assert_eq!(r.seven_day.unwrap().utilization, 12.5);
    }

    #[test]
    fn iso_string_resets_at_accepted() {
        let (_d, f) = write_capture(
            r#"{"five_hour":{"used_percentage":10.0,"resets_at":"2100-01-01T00:00:00Z"}}"#,
        );
        let r = read_from(&f, true, NOW);
        let five = r.five_hour.unwrap();
        assert_eq!(five.resets_at, Some(4102444800.0));
        assert!(five.window_duration.is_some());
    }

    #[test]
    fn no_windows_is_no_data() {
        let (_d, f) = write_capture(r#"{"model_id":"claude-x"}"#);
        assert_eq!(read_from(&f, true, NOW).status, RateLimitStatus::NoData);
    }

    #[test]
    fn garbage_is_error() {
        let (_d, f) = write_capture("not json{{{");
        assert!(matches!(
            read_from(&f, true, NOW).status,
            RateLimitStatus::Error { .. }
        ));
    }

    #[test]
    fn iso8601_parser_variants() {
        assert_eq!(parse_iso8601_epoch("1970-01-01T00:00:00Z"), Some(0.0));
        // Reference values from GNU `date -u -d ... +%s`.
        assert_eq!(parse_iso8601_epoch("2026-07-03T04:00:00Z"), Some(1783051200.0));
        assert_eq!(
            parse_iso8601_epoch("2026-07-03T12:00:00+08:00"),
            Some(1783051200.0)
        );
        let frac = parse_iso8601_epoch("2026-07-03T04:00:00.500Z").unwrap();
        assert!((frac - 1783051200.5).abs() < 1e-6);
        assert_eq!(parse_iso8601_epoch("garbage"), None);
    }
}
