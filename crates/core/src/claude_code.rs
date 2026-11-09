//! Where the Claude quota probe finds Claude Code on Windows. Pure file-system
//! inspection: nothing here starts a process or reads credentials.
//!
//! Detection (`quota_product`) counts any `claude.*` that PATHEXT resolves on
//! PATH as installed, but the probe used to look for `claude.exe` only. An npm
//! install puts `claude.cmd` (plus a POSIX `claude` and `claude.ps1`) on PATH
//! and no `claude.exe`, so the card said "detected" while the reader could
//! never find a program. macOS `ClaudeUsageProbe.discoverBinaries` walks PATH
//! for the same reason; this restores that documented parity.
//!
//! A `.cmd` shim is never run through `cmd.exe`: the probe arguments carry JSON
//! (quotes and braces) that cmd's own parsing can rewrite. The shim is read as
//! text instead and resolved to the program it launches — npm's generated
//! shim for Claude Code 2.1.x execs `node_modules\@anthropic-ai\claude-code\
//! bin\claude.exe` directly; older releases ran `cli.js` with Node.

use crate::quota_product::{path_executables, DiscoveryEnvironment};
use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Generated shims are a few hundred bytes; anything larger is not one.
const MAX_SHIM_BYTES: u64 = 64 * 1024;
/// Yarn's global shim points at another `.cmd`; deeper chains are not shims.
const MAX_SHIM_DEPTH: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeLaunch {
    /// The executable to start.
    pub program: PathBuf,
    /// JavaScript entry handed to `program` (Node) ahead of the probe
    /// arguments; `None` when `program` is Claude Code itself.
    pub script: Option<PathBuf>,
}

/// Candidate launches in preference order: the explicit override, the native
/// installer locations, then every `claude.*` on PATH in shell order.
/// Duplicates (the native installer's directory is usually on PATH too) are
/// dropped; entries that cannot be launched without a shell are skipped.
pub fn discover(env: &DiscoveryEnvironment, override_bin: Option<PathBuf>) -> Vec<ClaudeLaunch> {
    discover_with_fallback_node(env, override_bin, None)
}

/// [`discover`], with a Node to run a JavaScript shim entry when neither the
/// shim's directory nor PATH has one. The app passes its bundled runtime: a
/// Node manager such as fnm puts `node` on PATH only inside the shells it
/// hooks, so an app started at login would otherwise drop the candidate.
pub fn discover_with_fallback_node(
    env: &DiscoveryEnvironment,
    override_bin: Option<PathBuf>,
    fallback_node: Option<&Path>,
) -> Vec<ClaudeLaunch> {
    let mut paths = Vec::new();
    paths.extend(override_bin);
    paths.push(env.home.join(".local").join("bin").join("claude.exe"));
    paths.push(env.home.join(".claude").join("local").join("claude.exe"));
    if let Some(local) = &env.local_app_data {
        paths.push(
            local
                .join("Programs")
                .join("Claude Code")
                .join("claude.exe"),
        );
    }
    paths.extend(path_executables("claude", env));

    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|path| path.is_file())
        .filter_map(|path| launch_for(&path, env, fallback_node, 0))
        .filter(|launch| seen.insert(identity(launch)))
        .collect()
}

fn identity(launch: &ClaudeLaunch) -> (String, Option<String>) {
    let lower = |path: &Path| path.to_string_lossy().to_ascii_lowercase();
    (lower(&launch.program), launch.script.as_deref().map(lower))
}

fn extension(path: &Path) -> String {
    path.extension()
        .map(|value| value.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

fn launch_for(
    path: &Path,
    env: &DiscoveryEnvironment,
    fallback_node: Option<&Path>,
    depth: usize,
) -> Option<ClaudeLaunch> {
    match extension(path).as_str() {
        "exe" => Some(ClaudeLaunch {
            program: path.to_path_buf(),
            script: None,
        }),
        "cmd" | "bat" if depth < MAX_SHIM_DEPTH => resolve_shim(path, env, fallback_node, depth),
        // The extensionless POSIX shim, `.ps1`, and anything else need a shell.
        _ => None,
    }
}

/// Follow one npm / pnpm / Yarn generated shim to what it starts.
fn resolve_shim(
    shim: &Path,
    env: &DiscoveryEnvironment,
    fallback_node: Option<&Path>,
    depth: usize,
) -> Option<ClaudeLaunch> {
    let mut bytes = Vec::new();
    File::open(shim)
        .ok()?
        .take(MAX_SHIM_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_SHIM_BYTES {
        return None;
    }
    let directory = shim.parent()?;
    let relative = shim_target(&String::from_utf8_lossy(&bytes))?;
    let mut target = directory.to_path_buf();
    for component in relative.split(['\\', '/']).filter(|part| !part.is_empty()) {
        target.push(component);
    }
    if !target.is_file() {
        return None;
    }
    match extension(&target).as_str() {
        "js" | "cjs" | "mjs" => Some(ClaudeLaunch {
            program: node_for_shim(directory, env, fallback_node)?,
            script: Some(target),
        }),
        _ => launch_for(&target, env, fallback_node, depth + 1),
    }
}

/// The shim's target: its last quoted path that starts at the shim's own
/// directory (`%dp0%` in npm's template, `%~dp0` in pnpm's and Yarn's). The
/// `node.exe` probe in `IF EXIST` comes earlier, so the last one is the entry.
fn shim_target(text: &str) -> Option<String> {
    let mut found = None;
    for line in text.lines() {
        for quoted in line.split('"').skip(1).step_by(2) {
            let lower = quoted.to_ascii_lowercase();
            let rest = ["%dp0%", "%~dp0"]
                .iter()
                .find_map(|prefix| lower.starts_with(prefix).then(|| &quoted[prefix.len()..]));
            if let Some(rest) = rest {
                let rest = rest.trim_start_matches(['\\', '/']);
                if !rest.is_empty() {
                    found = Some(rest.to_string());
                }
            }
        }
    }
    found
}

/// The Node the shim itself would use — a `node.exe` beside the shim, else the
/// first `node.exe` on PATH — so the probe runs Claude Code the way the user's
/// own `claude` command does. Only when neither exists does `fallback_node`
/// (the app's bundled runtime) run the entry instead of dropping it.
fn node_for_shim(
    directory: &Path,
    env: &DiscoveryEnvironment,
    fallback_node: Option<&Path>,
) -> Option<PathBuf> {
    let local = directory.join("node.exe");
    if local.is_file() {
        return Some(local);
    }
    path_executables("node", env)
        .into_iter()
        .find(|path| extension(path) == "exe")
        .or_else(|| {
            fallback_node
                .filter(|node| node.is_file())
                .map(Path::to_path_buf)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// npm's cmd-shim output for a target without a shebang (Claude Code
    /// 2.1.x ships `bin/claude.exe`, a native binary placed by postinstall).
    const NPM_NATIVE_SHIM: &str = "@ECHO off\r\nGOTO start\r\n:find_dp0\r\nSET dp0=%~dp0\r\nEXIT /b\r\n:start\r\nSETLOCAL\r\nCALL :find_dp0\r\n\"%dp0%\\node_modules\\@anthropic-ai\\claude-code\\bin\\claude.exe\"   %*\r\n";

    /// npm's cmd-shim output for a `#!/usr/bin/env node` entry (older
    /// releases shipped `cli.js`).
    const NPM_NODE_SHIM: &str = "@ECHO off\r\nGOTO start\r\n:find_dp0\r\nSET dp0=%~dp0\r\nEXIT /b\r\n:start\r\nSETLOCAL\r\nCALL :find_dp0\r\n\r\nIF EXIST \"%dp0%\\node.exe\" (\r\n  SET \"_prog=%dp0%\\node.exe\"\r\n) ELSE (\r\n  SET \"_prog=node\"\r\n  SET PATHEXT=%PATHEXT:;.JS;=;%\r\n)\r\n\r\nendLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & \"%_prog%\"  \"%dp0%\\node_modules\\@anthropic-ai\\claude-code\\cli.js\" %*\r\n";

    /// pnpm's (`@zkochan/cmd-shim`) global shim.
    const PNPM_SHIM: &str = "@SETLOCAL\r\n@IF NOT DEFINED NODE_PATH (\r\n  @SET \"NODE_PATH=C:\\pnpm\\global\\5\\node_modules\"\r\n)\r\n@IF EXIST \"%~dp0\\node.exe\" (\r\n  \"%~dp0\\node.exe\"  \"%~dp0\\global\\5\\node_modules\\@anthropic-ai\\claude-code\\cli.js\" %*\r\n) ELSE (\r\n  @SET PATHEXT=%PATHEXT:;.JS;=;%\r\n  node  \"%~dp0\\global\\5\\node_modules\\@anthropic-ai\\claude-code\\cli.js\" %*\r\n)\r\n";

    fn environment(root: &Path, path_directories: Vec<PathBuf>) -> DiscoveryEnvironment {
        DiscoveryEnvironment {
            home: root.join("home"),
            path_directories,
            path_extensions: vec![".COM".into(), ".EXE".into(), ".BAT".into(), ".CMD".into()],
            app_data: Some(root.join("Roaming")),
            local_app_data: Some(root.join("Local")),
            program_files: vec![],
            windows: true,
        }
    }

    /// Windows (and macOS test volumes) match file names case-insensitively,
    /// and PATHEXT spells extensions in upper case.
    fn identities(launches: Vec<ClaudeLaunch>) -> Vec<(String, Option<String>)> {
        launches.iter().map(identity).collect()
    }

    fn write(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    /// The layout `npm install -g @anthropic-ai/claude-code` leaves behind.
    fn npm_prefix(root: &Path, shim: &str, entry: &str) -> PathBuf {
        let prefix = root.join("Roaming").join("npm");
        write(&prefix.join("claude.cmd"), shim);
        write(&prefix.join("claude"), "#!/bin/sh\n");
        write(&prefix.join("claude.ps1"), "#!/usr/bin/env pwsh\n");
        write(
            &prefix
                .join("node_modules")
                .join("@anthropic-ai")
                .join("claude-code")
                .join(entry),
            "",
        );
        prefix
    }

    fn claude_package(prefix: &Path) -> PathBuf {
        prefix
            .join("node_modules")
            .join("@anthropic-ai")
            .join("claude-code")
    }

    #[test]
    fn npm_shim_resolves_to_the_native_binary_it_execs() {
        let root = tempfile::tempdir().unwrap();
        let prefix = npm_prefix(root.path(), NPM_NATIVE_SHIM, "bin/claude.exe");
        let env = environment(root.path(), vec![prefix.clone()]);
        assert_eq!(
            discover(&env, None),
            vec![ClaudeLaunch {
                program: claude_package(&prefix).join("bin").join("claude.exe"),
                script: None,
            }]
        );
    }

    #[test]
    fn detection_and_the_reader_now_agree_on_an_npm_install() {
        let root = tempfile::tempdir().unwrap();
        let prefix = npm_prefix(root.path(), NPM_NATIVE_SHIM, "bin/claude.exe");
        let env = environment(root.path(), vec![prefix]);
        let detected = crate::quota_product::discover_with(&env)
            .into_iter()
            .any(|product| {
                product.provider == crate::RateLimitProvider::ClaudeCode && product.is_detected
            });
        assert!(detected);
        assert_eq!(discover(&env, None).len(), 1);
    }

    #[test]
    fn legacy_node_shim_runs_the_script_with_the_node_the_shim_would_pick() {
        let root = tempfile::tempdir().unwrap();
        let prefix = npm_prefix(root.path(), NPM_NODE_SHIM, "cli.js");
        let nodejs = root.path().join("nodejs");
        write(&nodejs.join("node.exe"), "");
        let env = environment(root.path(), vec![prefix.clone(), nodejs.clone()]);
        let script = claude_package(&prefix).join("cli.js");
        assert_eq!(
            identities(discover(&env, None)),
            identities(vec![ClaudeLaunch {
                program: nodejs.join("node.exe"),
                script: Some(script.clone()),
            }])
        );

        // A node.exe beside the shim wins, exactly like the shim's IF EXIST.
        write(&prefix.join("node.exe"), "");
        assert_eq!(
            identities(discover(&env, None)),
            identities(vec![ClaudeLaunch {
                program: prefix.join("node.exe"),
                script: Some(script),
            }])
        );
    }

    #[test]
    fn script_shim_without_any_node_is_not_a_candidate() {
        let root = tempfile::tempdir().unwrap();
        let prefix = npm_prefix(root.path(), NPM_NODE_SHIM, "cli.js");
        let env = environment(root.path(), vec![prefix]);
        assert!(discover(&env, None).is_empty());
        // A fallback that is not actually there does not help either.
        let missing = root.path().join("bundled").join("node.exe");
        assert!(discover_with_fallback_node(&env, None, Some(&missing)).is_empty());
    }

    #[test]
    fn bundled_node_runs_a_script_shim_only_when_no_other_node_exists() {
        let root = tempfile::tempdir().unwrap();
        let prefix = npm_prefix(root.path(), NPM_NODE_SHIM, "cli.js");
        let bundled = root.path().join("app").join("node").join("node.exe");
        write(&bundled, "");
        let script = claude_package(&prefix).join("cli.js");

        // No node beside the shim or on PATH (fnm outside a hooked shell).
        let env = environment(root.path(), vec![prefix.clone()]);
        assert_eq!(
            identities(discover_with_fallback_node(&env, None, Some(&bundled))),
            identities(vec![ClaudeLaunch {
                program: bundled.clone(),
                script: Some(script.clone()),
            }])
        );

        // The user's own Node still wins when PATH has one.
        let nodejs = root.path().join("nodejs");
        write(&nodejs.join("node.exe"), "");
        let env = environment(root.path(), vec![prefix, nodejs.clone()]);
        assert_eq!(
            identities(discover_with_fallback_node(&env, None, Some(&bundled))),
            identities(vec![ClaudeLaunch {
                program: nodejs.join("node.exe"),
                script: Some(script),
            }])
        );
    }

    #[test]
    fn pnpm_shim_resolves_to_its_script() {
        let root = tempfile::tempdir().unwrap();
        let pnpm = root.path().join("Local").join("pnpm");
        write(&pnpm.join("claude.CMD"), PNPM_SHIM);
        let script = pnpm
            .join("global")
            .join("5")
            .join("node_modules")
            .join("@anthropic-ai")
            .join("claude-code")
            .join("cli.js");
        write(&script, "");
        let nodejs = root.path().join("nodejs");
        write(&nodejs.join("node.exe"), "");
        let env = environment(root.path(), vec![pnpm, nodejs.clone()]);
        assert_eq!(
            identities(discover(&env, None)),
            identities(vec![ClaudeLaunch {
                program: nodejs.join("node.exe"),
                script: Some(script),
            }])
        );
    }

    #[test]
    fn yarn_style_shim_chain_is_followed_one_level() {
        let root = tempfile::tempdir().unwrap();
        let yarn_bin = root.path().join("Local").join("Yarn").join("bin");
        write(
            &yarn_bin.join("claude.cmd"),
            "@\"%~dp0\\..\\Data\\global\\node_modules\\.bin\\claude.cmd\" %*\r\n",
        );
        let inner = root
            .path()
            .join("Local")
            .join("Yarn")
            .join("Data")
            .join("global")
            .join("node_modules")
            .join(".bin");
        write(
            &inner.join("claude.cmd"),
            "@\"%~dp0\\..\\@anthropic-ai\\claude-code\\bin\\claude.exe\" %*\r\n",
        );
        let binary = inner
            .join("..")
            .join("@anthropic-ai")
            .join("claude-code")
            .join("bin")
            .join("claude.exe");
        write(&binary, "");
        let env = environment(root.path(), vec![yarn_bin]);
        let found = discover(&env, None);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].script, None);
        assert!(found[0].program.ends_with("claude-code/bin/claude.exe"));
        assert!(found[0].program.is_file());
    }

    #[test]
    fn unresolvable_shims_and_shell_only_entries_are_skipped() {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("bin");
        // Points at a file that is not there (postinstall never ran).
        write(&bin.join("claude.cmd"), NPM_NATIVE_SHIM);
        // A hand-written wrapper that does not start at its own directory.
        let other = root.path().join("other");
        write(
            &other.join("claude.bat"),
            "@node C:\\somewhere\\cli.js %*\r\n",
        );
        let env = environment(root.path(), vec![bin, other]);
        assert!(discover(&env, None).is_empty());
    }

    #[test]
    fn native_install_on_path_is_listed_once_and_first() {
        let root = tempfile::tempdir().unwrap();
        let native_dir = root.path().join("home").join(".local").join("bin");
        write(&native_dir.join("claude.exe"), "");
        let prefix = npm_prefix(root.path(), NPM_NATIVE_SHIM, "bin/claude.exe");
        let env = environment(root.path(), vec![native_dir.clone(), prefix.clone()]);
        assert_eq!(
            discover(&env, None)
                .into_iter()
                .map(|launch| launch.program)
                .collect::<Vec<_>>(),
            vec![
                native_dir.join("claude.exe"),
                claude_package(&prefix).join("bin").join("claude.exe"),
            ]
        );
    }

    #[test]
    fn override_comes_first_and_shares_the_same_rules() {
        let root = tempfile::tempdir().unwrap();
        let custom = root.path().join("custom").join("claude.exe");
        write(&custom, "");
        let prefix = npm_prefix(root.path(), NPM_NATIVE_SHIM, "bin/claude.exe");
        let env = environment(root.path(), vec![]);
        let found = discover(&env, Some(custom.clone()));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].program, custom);
        // An override pointing at a shim is resolved, not run through cmd.
        let found = discover(&env, Some(prefix.join("claude.cmd")));
        assert_eq!(
            found[0].program,
            claude_package(&prefix).join("bin").join("claude.exe")
        );
    }

    #[test]
    fn shim_target_takes_the_last_path_rooted_at_the_shim() {
        assert_eq!(
            shim_target(NPM_NODE_SHIM).as_deref(),
            Some("node_modules\\@anthropic-ai\\claude-code\\cli.js")
        );
        assert_eq!(
            shim_target(NPM_NATIVE_SHIM).as_deref(),
            Some("node_modules\\@anthropic-ai\\claude-code\\bin\\claude.exe")
        );
        assert_eq!(shim_target("@node cli.js %*"), None);
    }
}
