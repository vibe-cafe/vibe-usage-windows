//! Whether the user switched off Claude Code's "nonessential traffic"
//! (`CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC`).
//!
//! With that switch on, Claude Code answers `get_usage` with
//! `rate_limits: null`, so a probe cannot produce numbers; it would only add
//! one more signed-in launch of Claude Code. The probe used to unset the switch
//! for its own process (`--settings`), which turned back on traffic the user had
//! turned off. The network-privacy principle (rule 4: never change a user's
//! setting, including inside the processes we start) forbids that, so the app
//! now checks first and, when the switch is on, does not start Claude Code at
//! all; the card says why. Mirrors macOS `ClaudeNonessentialTraffic`.
//!
//! Claude Code counts any non-empty value as "on", `"0"` and `"false"`
//! included, so this does too. "On in any source" counts as on: Claude Code
//! resolves the levels by precedence, so a higher level that sets the variable
//! to an empty string would switch it back off there; treating that rare setup
//! as "on" errs towards not starting Claude Code. Server-managed settings from
//! the claude.ai console are fetched by Claude Code and are not visible here.
//!
//! Only the one key is read. Nothing else in these documents (which can hold
//! API keys in `env`) is kept, logged, or put in diagnostics.

use serde_json::Value;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

pub const VARIABLE: &str = "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC";

/// Where the probe's Claude Code would read the variable from. The probe runs
/// with the home directory as its working directory, so the home directory's
/// project files count too.
#[derive(Debug, Default, Clone)]
pub struct Sources {
    /// The app's own environment value.
    pub process_value: Option<String>,
    /// Settings files; missing or unreadable ones are skipped.
    pub settings_files: Vec<PathBuf>,
    /// Managed policy documents delivered as JSON text (the Windows registry's
    /// `SOFTWARE\Policies\ClaudeCode` `Settings` value, HKLM and HKCU).
    pub policy_documents: Vec<String>,
}

/// User settings (`$CLAUDE_CONFIG_DIR/settings.json`, else
/// `~/.claude/settings.json`), the home directory's project files
/// (`~/.claude/settings.json`, `~/.claude/settings.local.json`), then each
/// managed directory's `managed-settings.json` and its `managed-settings.d`
/// drop-ins (`*.json`, sorted, hidden files skipped).
pub fn settings_files(
    home: Option<&Path>,
    claude_config_dir: Option<&OsStr>,
    managed_dirs: &[PathBuf],
) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Some(dir) = claude_config_dir.filter(|dir| !dir.is_empty()) {
        files.push(PathBuf::from(dir).join("settings.json"));
    }
    if let Some(home) = home {
        let claude = home.join(".claude");
        files.push(claude.join("settings.json"));
        files.push(claude.join("settings.local.json"));
    }
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

pub fn is_disabled(sources: &Sources) -> bool {
    if sources
        .process_value
        .as_deref()
        .is_some_and(|value| !value.is_empty())
    {
        return true;
    }
    sources.settings_files.iter().any(|file| {
        std::fs::read_to_string(file)
            .ok()
            .is_some_and(|text| document_sets_it(&text))
    }) || sources
        .policy_documents
        .iter()
        .any(|text| document_sets_it(text))
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
/// valid in `env`; it counts as set too, erring towards not starting Claude
/// Code.
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

    fn disabled(home: &Path, config_dir: Option<&Path>, managed: &Path) -> bool {
        is_disabled(&Sources {
            process_value: None,
            settings_files: settings_files(
                Some(home),
                config_dir.map(Path::as_os_str),
                &[managed.to_path_buf()],
            ),
            policy_documents: Vec::new(),
        })
    }

    #[test]
    fn off_when_no_source_sets_it() {
        let root = tempfile::tempdir().unwrap();
        let (home, managed) = (root.path().join("home"), root.path().join("managed"));
        assert!(!disabled(&home, None, &managed));
        write(&home.join(".claude/settings.json"), r#"{"env":{"OTHER":"1"}}"#);
        assert!(!disabled(&home, None, &managed));
    }

    #[test]
    fn app_environment_counts_any_non_empty_value() {
        for (value, expected) in [("1", true), ("0", true), ("false", true), ("", false)] {
            let sources = Sources {
                process_value: Some(value.into()),
                ..Sources::default()
            };
            assert_eq!(is_disabled(&sources), expected, "{value:?}");
        }
    }

    #[test]
    fn user_project_local_and_config_dir_files_turn_it_on() {
        let root = tempfile::tempdir().unwrap();
        let (home, managed) = (root.path().join("home"), root.path().join("managed"));
        write(&home.join(".claude/settings.json"), &settings(r#""1""#));
        assert!(disabled(&home, None, &managed));

        let root = tempfile::tempdir().unwrap();
        let (home, managed) = (root.path().join("home"), root.path().join("managed"));
        write(&home.join(".claude/settings.local.json"), &settings(r#""true""#));
        assert!(disabled(&home, None, &managed));

        let root = tempfile::tempdir().unwrap();
        let (home, managed) = (root.path().join("home"), root.path().join("managed"));
        let profile = root.path().join("profile");
        write(&profile.join("settings.json"), &settings(r#""1""#));
        assert!(disabled(&home, Some(&profile), &managed));
        assert!(!disabled(&home, None, &managed));
    }

    #[test]
    fn managed_file_and_json_drop_ins_turn_it_on() {
        let root = tempfile::tempdir().unwrap();
        let (home, managed) = (root.path().join("home"), root.path().join("managed"));
        write(&managed.join("managed-settings.json"), &settings(r#""1""#));
        assert!(disabled(&home, None, &managed));

        let root = tempfile::tempdir().unwrap();
        let (home, managed) = (root.path().join("home"), root.path().join("managed"));
        write(&managed.join("managed-settings.d/10-base.json"), r#"{"env":{}}"#);
        write(&managed.join("managed-settings.d/notes.txt"), &settings(r#""1""#));
        assert!(!disabled(&home, None, &managed));
        write(&managed.join("managed-settings.d/20-privacy.JSON"), &settings(r#""1""#));
        assert!(disabled(&home, None, &managed));
    }

    #[test]
    fn registry_policy_documents_turn_it_on() {
        let on = Sources {
            policy_documents: vec![r#"{"env":{}}"#.into(), settings(r#""1""#)],
            ..Sources::default()
        };
        assert!(is_disabled(&on));
        let off = Sources {
            policy_documents: vec!["not json".into(), settings(r#""""#)],
            ..Sources::default()
        };
        assert!(!is_disabled(&off));
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
        assert!(document_sets_it(&settings("1")));
        assert!(document_sets_it(&settings("true")));
    }
}
