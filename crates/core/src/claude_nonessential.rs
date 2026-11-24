//! How the Claude probe treats Claude Code's "nonessential traffic" switch
//! (`CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC`) — network-privacy principle,
//! rules 4 and 6 (keep what the user switched off off, without losing the
//! quota card). Mirrors macOS `ClaudeNonessentialTraffic`.
//!
//! What the switch does, read from Claude Code 2.1.330 and measured with a
//! host-logging proxy (2026-11-24): it blocks every request Claude Code calls
//! nonessential, the usage read (`/api/oauth/usage`) included — with it on,
//! `get_usage` only answers from Claude Code's own snapshot, which nothing
//! refreshes. The documented equivalents (telemetry, error reporting, the
//! auto-updater, the bug command) and every other such feature that has a
//! switch of its own can be turned off one by one.
//!
//! So the probe always runs with the master switch cleared and all those
//! separate switches on (`QUIET_ENVIRONMENT`, passed both in the environment
//! and in an in-memory `--settings`, which outranks the user's files): what
//! the user switched off stays off, and the probe adds only the usage read.
//! Measured, the probe then reaches `api.anthropic.com` only, three requests:
//! the usage read plus two Claude Code makes at every start that no separate
//! switch covers (its start-up configuration and the organization's fast-mode
//! status) — same host and same account as the usage read.
//!
//! Cleared only for the probe's own process and never written anywhere. An
//! organization's managed settings outrank `--settings`; when those force the
//! switch on (`is_forced_by_policy`), Claude Code cannot read the quota and
//! the app does not start it.
//!
//! Only that one key is read from policy documents. Each is parsed in memory
//! and everything else in it is dropped right away — never kept, logged, or
//! put in diagnostics.

use serde_json::Value;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

pub const VARIABLE: &str = "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC";

/// The probe's Claude Code environment: the master switch cleared (empty
/// string — Claude Code counts any non-empty value as on, `"0"` included) and
/// each separate off switch on. `CLAUDE_CODE_MODEL_CATALOG` is off with `"0"`.
pub const QUIET_ENVIRONMENT: [(&str, &str); 15] = [
    (VARIABLE, ""),
    ("DISABLE_TELEMETRY", "1"),
    ("DISABLE_ERROR_REPORTING", "1"),
    ("DISABLE_AUTOUPDATER", "1"),
    ("DISABLE_UPDATES", "1"),
    ("DISABLE_BUG_COMMAND", "1"),
    ("DISABLE_FEEDBACK_COMMAND", "1"),
    ("CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY", "1"),
    ("DISABLE_GROWTHBOOK", "1"),
    ("CLAUDE_CODE_DISABLE_FEATURE_FLAGS", "1"),
    ("CLAUDE_CODE_DISABLE_VITALS_EMITTER", "1"),
    ("CLAUDE_CODE_MODEL_CATALOG", "0"),
    ("DISABLE_INSTALLATION_CHECKS", "1"),
    ("CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL", "1"),
    ("DO_NOT_TRACK", "1"),
];

/// `--settings` for the probe: `{"env": QUIET_ENVIRONMENT}`. Claude Code
/// re-applies `env` from the user's settings files after start, so the
/// process environment alone is not enough; flag settings outrank those files
/// (not managed settings) and are never written to disk.
pub fn probe_settings() -> String {
    let env: serde_json::Map<String, Value> = QUIET_ENVIRONMENT
        .iter()
        .map(|(key, value)| ((*key).to_string(), Value::String((*value).to_string())))
        .collect();
    serde_json::json!({ "env": env }).to_string()
}

/// Each managed directory's `managed-settings.json` and its
/// `managed-settings.d` drop-ins (`*.json`, sorted, hidden files skipped).
pub fn managed_settings_files(managed_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for managed in managed_dirs {
        files.push(managed.join("managed-settings.json"));
        let mut drop_ins: Vec<PathBuf> = std::fs::read_dir(managed.join("managed-settings.d"))
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .filter(|path| {
                        path.extension()
                            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
                    })
                    .filter(|path| {
                        !path
                            .file_name()
                            .and_then(OsStr::to_str)
                            .is_some_and(|name| name.starts_with('.'))
                    })
                    .collect()
            })
            .unwrap_or_default();
        drop_ins.sort();
        files.extend(drop_ins);
    }
    files
}

/// Whether the organization's managed settings force the switch on: a
/// managed settings file (see `managed_settings_files`) or a policy document
/// (the registry's `SOFTWARE\Policies\ClaudeCode` `Settings` value). Those
/// outrank the probe's `--settings`, so Claude Code could not read the quota.
/// The user's own files and environment do not count here: the probe handles
/// those. Server-managed settings from the claude.ai console are fetched by
/// Claude Code and are not visible here; with those, the probe runs and Claude
/// Code answers without windows.
pub fn is_forced_by_policy(files: &[PathBuf], policy_documents: &[String]) -> bool {
    files.iter().any(|file| file_sets_it(file))
        || policy_documents.iter().any(|text| document_sets_it(text))
}

fn file_sets_it(file: &Path) -> bool {
    std::fs::read_to_string(file)
        .ok()
        .is_some_and(|text| document_sets_it(&text))
}

/// True when a settings document's top-level `env` sets the variable to a
/// non-empty value. A leading UTF-8 byte-order mark (Notepad writes one) is
/// ignored; anything that is not a JSON object counts as not set.
pub fn document_sets_it(text: &str) -> bool {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Ok(root) = serde_json::from_str::<Value>(text) else {
        return false;
    };
    is_on(
        root.get("env")
            .and_then(Value::as_object)
            .and_then(|env| env.get(VARIABLE)),
    )
}

/// Claude Code treats any non-empty value as set. A non-string value is not
/// valid in `env`; it counts as set too.
pub fn is_on(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::String(text)) => !text.is_empty(),
        Some(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn settings(value: &str) -> String {
        format!(r#"{{"model":"x","env":{{"ANTHROPIC_API_KEY":"secret","{VARIABLE}":{value}}}}}"#)
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn forced(managed: &Path) -> bool {
        is_forced_by_policy(&managed_settings_files(&[managed.to_path_buf()]), &[])
    }

    #[test]
    fn quiet_environment_clears_only_the_master_switch() {
        let empty: Vec<_> = QUIET_ENVIRONMENT.iter().filter(|(_, v)| v.is_empty()).collect();
        assert_eq!(empty, vec![&(VARIABLE, "")]);
        for key in [
            "DISABLE_TELEMETRY",
            "DISABLE_ERROR_REPORTING",
            "DISABLE_AUTOUPDATER",
            "DISABLE_BUG_COMMAND",
        ] {
            assert!(QUIET_ENVIRONMENT.contains(&(key, "1")), "{key}");
        }
        assert!(QUIET_ENVIRONMENT.contains(&("CLAUDE_CODE_MODEL_CATALOG", "0")));
    }

    #[test]
    fn probe_settings_carry_exactly_the_quiet_environment() {
        let settings: Value = serde_json::from_str(&probe_settings()).unwrap();
        let object = settings.as_object().unwrap();
        assert_eq!(object.len(), 1);
        let env = object["env"].as_object().unwrap();
        assert_eq!(env.len(), QUIET_ENVIRONMENT.len());
        for (key, value) in QUIET_ENVIRONMENT {
            assert_eq!(env[key], Value::String(value.into()), "{key}");
        }
    }

    #[test]
    fn no_policy_by_default() {
        let root = tempfile::tempdir().unwrap();
        let managed = root.path().join("managed");
        assert!(!forced(&managed));
        write(&managed.join("managed-settings.json"), r#"{"env":{"OTHER":"1"}}"#);
        assert!(!forced(&managed));
    }

    #[test]
    fn managed_file_and_json_drop_ins_force_it() {
        let root = tempfile::tempdir().unwrap();
        let managed = root.path().join("managed");
        write(&managed.join("managed-settings.json"), &settings(r#""1""#));
        assert!(forced(&managed));

        let root = tempfile::tempdir().unwrap();
        let managed = root.path().join("managed");
        write(&managed.join("managed-settings.d/10-base.json"), r#"{"env":{}}"#);
        write(&managed.join("managed-settings.d/notes.txt"), &settings(r#""1""#));
        assert!(!forced(&managed));
        write(&managed.join("managed-settings.d/20-privacy.JSON"), &settings(r#""1""#));
        assert!(forced(&managed));
    }

    #[test]
    fn registry_policy_documents_force_it() {
        assert!(is_forced_by_policy(&[], &[r#"{"env":{}}"#.into(), settings(r#""1""#)]));
        assert!(!is_forced_by_policy(&[], &["not json".into(), settings(r#""""#)]));
    }

    #[test]
    fn empty_missing_or_unreadable_values_do_not_count() {
        assert!(!document_sets_it(&settings(r#""""#)));
        assert!(!document_sets_it(&settings("null")));
        assert!(!document_sets_it("{ not json"));
        assert!(!document_sets_it(r#"{"env":"not an object"}"#));
        assert!(!document_sets_it("[]"));
    }

    #[test]
    fn byte_order_mark_and_non_string_values() {
        assert!(document_sets_it(&format!("\u{feff}{}", settings(r#""1""#))));
        assert!(document_sets_it(&settings("0")));
        assert!(document_sets_it(&settings("1")));
        assert!(document_sets_it(&settings("true")));
    }
}
