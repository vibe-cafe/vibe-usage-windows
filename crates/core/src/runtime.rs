//! JS runtime detection — Windows counterpart of Services/RuntimeDetector.swift.
//!
//! The CLI is vendored inside the app resources and executed as
//! `node <resources>/cli/bin/vibe-usage.js sync`. Detection order:
//!   1. bundled Node (guaranteed ≥22.5 for `node:sqlite` — zero variance)
//!   2. system Node ≥ 22.5 (PATH + common Windows install locations)
//!   3. any system Node ≥ 20 (SQLite-based parsers degrade gracefully)
//!   4. Bun (last resort; `node:sqlite` support varies by version)

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeKind {
    BundledNode,
    Node,
    Bun,
}

#[derive(Debug, Clone)]
pub struct Runtime {
    pub kind: RuntimeKind,
    pub path: PathBuf,
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// Candidate directories that may hold node/bun, beyond PATH.
/// Mirrors RuntimeDetector.searchPaths with Windows equivalents.
pub fn candidate_dirs() -> Vec<PathBuf> {
    let mut dirs_list: Vec<PathBuf> = Vec::new();

    if let Ok(path_var) = std::env::var("PATH") {
        for p in std::env::split_paths(&path_var) {
            dirs_list.push(p);
        }
    }

    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));

    #[cfg(windows)]
    {
        let env_dir = |var: &str| std::env::var(var).ok().map(PathBuf::from);
        if let Some(pf) = env_dir("ProgramFiles") {
            dirs_list.push(pf.join("nodejs"));
        }
        if let Some(pf86) = env_dir("ProgramFiles(x86)") {
            dirs_list.push(pf86.join("nodejs"));
        }
        if let Some(local) = env_dir("LOCALAPPDATA") {
            dirs_list.push(local.join("Programs").join("nodejs"));
            dirs_list.push(local.join("Volta").join("bin"));
            dirs_list.push(local.join("fnm_multishells"));
            dirs_list.push(local.join("pnpm"));
        }
        if let Some(appdata) = env_dir("APPDATA") {
            dirs_list.push(appdata.join("npm"));
        }
        if let Some(pd) = env_dir("ProgramData") {
            dirs_list.push(pd.join("chocolatey").join("bin"));
        }
        // nvm-windows: NVM_SYMLINK points at the active version's dir.
        if let Some(symlink) = env_dir("NVM_SYMLINK") {
            dirs_list.push(symlink);
        }
        dirs_list.push(home.join("scoop").join("shims"));
    }
    #[cfg(not(windows))]
    {
        dirs_list.push(PathBuf::from("/usr/local/bin"));
        dirs_list.push(PathBuf::from("/opt/homebrew/bin"));
        dirs_list.push(home.join(".volta").join("bin"));
        dirs_list.push(home.join(".fnm").join("current").join("bin"));
        // nvm: newest installed version (no "current" symlink)
        let versions = home.join(".nvm").join("versions").join("node");
        if let Ok(entries) = std::fs::read_dir(&versions) {
            let mut vs: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
            vs.sort();
            if let Some(latest) = vs.last() {
                dirs_list.push(latest.join("bin"));
            }
        }
    }

    dirs_list.push(home.join(".bun").join("bin"));
    dirs_list
}

pub fn find_in_dirs(name: &str, dirs_list: &[PathBuf]) -> Option<PathBuf> {
    let file = exe(name);
    for dir in dirs_list {
        let candidate = dir.join(&file);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Parse `node -v` output ("v22.11.0") into (major, minor, patch).
pub fn parse_node_version(raw: &str) -> Option<(u32, u32, u32)> {
    let trimmed = raw.trim().trim_start_matches('v');
    let mut parts = trimmed.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts
        .next()
        .map(|p| {
            p.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
        })
        .and_then(|p| p.parse().ok())
        .unwrap_or(0);
    Some((major, minor, patch))
}

/// node:sqlite (used by the Cursor/OpenCode/Kiro/Hermes/ZCode parsers)
/// requires Node ≥ 22.5.
pub fn supports_node_sqlite(version: (u32, u32, u32)) -> bool {
    version.0 > 22 || (version.0 == 22 && version.1 >= 5)
}

pub fn meets_cli_minimum(version: (u32, u32, u32)) -> bool {
    version.0 >= 20
}

/// Pick the best runtime. `bundled_node` is the node.exe shipped in app
/// resources (may be absent in dev builds); `probe_version` runs `node -v`
/// (injected so tests don't spawn processes).
pub fn detect(
    bundled_node: Option<PathBuf>,
    probe_version: impl Fn(&Path) -> Option<(u32, u32, u32)>,
) -> Option<Runtime> {
    if let Some(bundled) = bundled_node {
        if bundled.is_file() {
            return Some(Runtime {
                kind: RuntimeKind::BundledNode,
                path: bundled,
            });
        }
    }

    let dirs_list = candidate_dirs();
    let mut fallback_node: Option<PathBuf> = None;

    if let Some(node) = find_in_dirs("node", &dirs_list) {
        if let Some(version) = probe_version(&node) {
            if supports_node_sqlite(version) {
                return Some(Runtime {
                    kind: RuntimeKind::Node,
                    path: node,
                });
            }
            if meets_cli_minimum(version) {
                fallback_node = Some(node);
            }
        }
    }

    if let Some(node) = fallback_node {
        return Some(Runtime {
            kind: RuntimeKind::Node,
            path: node,
        });
    }

    find_in_dirs("bun", &dirs_list).map(|bun| Runtime {
        kind: RuntimeKind::Bun,
        path: bun,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parses_node_versions() {
        assert_eq!(parse_node_version("v22.11.0\n"), Some((22, 11, 0)));
        assert_eq!(parse_node_version("v20.18.2"), Some((20, 18, 2)));
        assert_eq!(parse_node_version("v23.0.0-nightly"), Some((23, 0, 0)));
        assert_eq!(parse_node_version("garbage"), None);
    }

    #[test]
    fn sqlite_support_boundary() {
        assert!(supports_node_sqlite((22, 5, 0)));
        assert!(supports_node_sqlite((23, 0, 0)));
        assert!(!supports_node_sqlite((22, 4, 9)));
        assert!(!supports_node_sqlite((20, 18, 2)));
    }

    #[test]
    fn bundled_node_wins() {
        let dir = tempdir().unwrap();
        let bundled = dir.path().join(if cfg!(windows) { "node.exe" } else { "node" });
        std::fs::write(&bundled, "stub").unwrap();
        let r = detect(Some(bundled.clone()), |_| None).unwrap();
        assert_eq!(r.kind, RuntimeKind::BundledNode);
        assert_eq!(r.path, bundled);
    }

    #[test]
    fn missing_bundled_falls_through() {
        let r = detect(Some(PathBuf::from("/definitely/not/here/node")), |_| None);
        // May still find a system node/bun on the dev machine — just must not
        // report the missing bundled path.
        if let Some(rt) = r {
            assert_ne!(rt.kind, RuntimeKind::BundledNode);
        }
    }
}
