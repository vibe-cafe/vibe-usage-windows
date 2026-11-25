//! Local-only subscription product discovery and product selection policy.
//! Discovery checks conventional files/apps/commands but never opens a
//! credential store, reads credential contents, or performs network I/O.

use crate::RateLimitProvider;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ZCodeQuotaRegion {
    #[default]
    #[serde(rename = "bigModel")]
    BigModel,
    #[serde(rename = "zAI")]
    ZAi,
}

impl ZCodeQuotaRegion {
    pub fn environment_key(self) -> &'static str {
        match self {
            Self::BigModel => "BIGMODEL_API_KEY",
            Self::ZAi => "Z_AI_API_KEY",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum QuotaProductAvailability {
    Ready,
    PendingProtocol,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaProduct {
    pub provider: RateLimitProvider,
    pub display_name: &'static str,
    pub availability: QuotaProductAvailability,
    pub is_detected: bool,
}

impl QuotaProduct {
    pub fn is_selectable(&self) -> bool {
        self.availability == QuotaProductAvailability::Ready && self.is_detected
    }
}

#[derive(Debug, Clone)]
pub struct DiscoveryEnvironment {
    pub home: PathBuf,
    pub path_directories: Vec<PathBuf>,
    pub path_extensions: Vec<String>,
    pub app_data: Option<PathBuf>,
    pub local_app_data: Option<PathBuf>,
    pub program_files: Vec<PathBuf>,
    pub windows: bool,
}

impl DiscoveryEnvironment {
    pub fn live() -> Self {
        let path_directories = std::env::var_os("PATH")
            .map(|value| std::env::split_paths(&value).collect())
            .unwrap_or_default();
        let path_extensions = std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
            .split(';')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect();
        Self {
            home: dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")),
            path_directories,
            path_extensions,
            app_data: std::env::var_os("APPDATA").map(PathBuf::from),
            local_app_data: std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
            program_files: ["ProgramFiles", "ProgramFiles(x86)"]
                .into_iter()
                .filter_map(std::env::var_os)
                .map(PathBuf::from)
                .collect(),
            windows: cfg!(windows),
        }
    }
}

pub fn discover() -> Vec<QuotaProduct> {
    discover_with(&DiscoveryEnvironment::live())
}

pub fn discover_with(environment: &DiscoveryEnvironment) -> Vec<QuotaProduct> {
    catalog()
        .iter()
        .map(|product| QuotaProduct {
            provider: product.provider,
            display_name: product.display_name,
            availability: product.availability,
            is_detected: is_detected(product, environment),
        })
        .collect()
}

pub struct ProductDefinition {
    pub provider: RateLimitProvider,
    pub display_name: &'static str,
    pub availability: QuotaProductAvailability,
    pub cli_id: Option<&'static str>,
    command: &'static str,
    relative_paths: &'static [&'static str],
}

/// App-owned presentation, routing and discovery rules. The vendored CLI
/// keeps its independent versioned contract; changing it requires CLI review.
pub fn catalog() -> &'static [ProductDefinition] {
    use QuotaProductAvailability::Ready;
    use RateLimitProvider::*;
    &[
        ProductDefinition {
            provider: Codex,
            display_name: "Codex",
            availability: Ready,
            cli_id: None,
            command: "codex",
            relative_paths: &[".codex"],
        },
        ProductDefinition {
            provider: ClaudeCode,
            display_name: "Claude",
            availability: Ready,
            cli_id: None,
            command: "claude",
            relative_paths: &[".claude", ".claude.json"],
        },
        ProductDefinition {
            provider: KimiCode,
            display_name: "Kimi Code",
            availability: Ready,
            cli_id: Some("kimi-code"),
            command: "kimi",
            relative_paths: &[".kimi", ".kimi-code", ".config/kimi"],
        },
        ProductDefinition {
            provider: ZCode,
            display_name: "ZCode",
            availability: Ready,
            cli_id: Some("zcode"),
            command: "zcode",
            relative_paths: &[".zcode", ".config/zcode"],
        },
        ProductDefinition {
            provider: Grok,
            display_name: "Grok",
            availability: Ready,
            cli_id: Some("grok"),
            command: "grok",
            relative_paths: &[".grok"],
        },
        ProductDefinition {
            provider: OpenCodeGo,
            display_name: "OpenCode Go",
            availability: Ready,
            cli_id: Some("opencode-go"),
            command: "opencode",
            relative_paths: &[".local/share/opencode"],
        },
        ProductDefinition {
            provider: Cursor,
            display_name: "Cursor",
            availability: Ready,
            cli_id: Some("cursor"),
            command: "cursor",
            relative_paths: &[".cursor"],
        },
        ProductDefinition {
            provider: Antigravity,
            display_name: "Antigravity / AGY",
            availability: Ready,
            cli_id: Some("antigravity"),
            command: "agy",
            relative_paths: &[".gemini/antigravity-cli", ".gemini/antigravity"],
        },
    ]
}

/// Where the CLI looks for `agy.exe` besides PATH (`antigravityBinary()` in
/// the CLI's `src/quotas/providers/antigravity.js`; keep the two lists equal,
/// or a product can count as installed yet never be read):
/// `~\.local\bin\agy.exe`, and the official installer's default
/// `%LOCALAPPDATA%\agy\bin\agy.exe` (antigravity.google/docs/getting-started
/// and its `install.ps1` / `install.cmd`). That folder reaches PATH only after
/// `agy install` edits the user environment, which an app started earlier
/// (autostart) never sees, so it has to be checked directly.
pub fn antigravity_cli_paths(environment: &DiscoveryEnvironment) -> Vec<PathBuf> {
    let mut paths = vec![environment
        .home
        .join(".local")
        .join("bin")
        .join("agy.exe")];
    if let Some(local) = &environment.local_app_data {
        paths.push(local.join("agy").join("bin").join("agy.exe"));
    }
    paths
}

pub fn cli_id(provider: RateLimitProvider) -> Option<&'static str> {
    catalog()
        .iter()
        .find(|product| product.provider == provider)
        .and_then(|product| product.cli_id)
}

pub fn cli_provider(id: &str) -> Option<RateLimitProvider> {
    catalog()
        .iter()
        .find(|product| product.cli_id == Some(id))
        .map(|product| product.provider)
}

pub fn initial_selection(products: &[QuotaProduct]) -> Vec<RateLimitProvider> {
    normalize_selection(
        products
            .iter()
            .filter(|product| product.is_selectable())
            .map(|product| product.provider),
    )
}

/// Apply one explicit user choice to the display list. Detection is only a
/// first-launch recommendation: the manual selector must remain usable when
/// discovery is incomplete. Every selection is kept — the panel scrolls, so
/// there is no reason to evict an earlier choice to make room.
pub fn update_selection(
    selection: &[RateLimitProvider],
    provider: RateLimitProvider,
    selected: bool,
) -> Vec<RateLimitProvider> {
    let mut next = normalize_selection(selection.iter().copied())
        .into_iter()
        .filter(|value| *value != provider)
        .collect::<Vec<_>>();
    if selected {
        next.push(provider);
    }
    normalize_selection(next)
}

pub fn normalize_selection(
    selection: impl IntoIterator<Item = RateLimitProvider>,
) -> Vec<RateLimitProvider> {
    let known: HashSet<_> = catalog().iter().map(|entry| entry.provider).collect();
    let mut seen = HashSet::new();
    selection
        .into_iter()
        .filter(|provider| known.contains(provider) && seen.insert(*provider))
        .collect()
}

fn is_detected(product: &ProductDefinition, environment: &DiscoveryEnvironment) -> bool {
    product
        .relative_paths
        .iter()
        .any(|relative| environment.home.join(relative).exists())
        || application_paths(product.provider, environment)
            .iter()
            .any(|path| path.exists())
        || executable_exists(product.command, environment)
}

fn application_paths(
    provider: RateLimitProvider,
    environment: &DiscoveryEnvironment,
) -> Vec<PathBuf> {
    if !environment.windows {
        return Vec::new();
    }
    let mut roots = environment.program_files.clone();
    if let Some(local) = &environment.local_app_data {
        roots.push(local.clone());
        roots.push(local.join("Programs"));
    }
    let mut paths = Vec::new();
    for root in roots {
        match provider {
            RateLimitProvider::Codex => paths.push(root.join("Codex").join("Codex.exe")),
            RateLimitProvider::ClaudeCode => {
                paths.push(root.join("Claude").join("Claude.exe"));
            }
            RateLimitProvider::ZCode => paths.push(root.join("ZCode").join("ZCode.exe")),
            RateLimitProvider::Cursor => {
                paths.push(root.join("Cursor").join("Cursor.exe"));
                paths.push(root.join("cursor").join("Cursor.exe"));
            }
            RateLimitProvider::KimiCode | RateLimitProvider::Grok => {}
            // Detection only. OpenCode keeps its data and logins under
            // `%USERPROFILE%\.local\share\opencode` on Windows too (the
            // home-relative path above; official troubleshooting docs and
            // anomalyco/opencode `global-roots.ts`), and the CLI reads them
            // there. A per-user folder here only counts as a sign that
            // OpenCode is installed; nothing is read from it.
            RateLimitProvider::OpenCodeGo => paths.push(root.join("opencode")),
            // Not in Antigravity's docs (they give no desktop install folder):
            // community reports put the per-user Antigravity 2.0 desktop app
            // and the 1.x IDE in `Programs\Antigravity`, and the standalone
            // 2.x IDE in `Programs\Antigravity IDE`. Detection only; reading
            // the quota always needs `agy`.
            RateLimitProvider::Antigravity => {
                paths.push(root.join("Antigravity").join("Antigravity.exe"));
                paths.push(root.join("Antigravity IDE").join("Antigravity IDE.exe"));
            }
        }
    }
    if let Some(app_data) = &environment.app_data {
        match provider {
            RateLimitProvider::ClaudeCode => paths.push(app_data.join("Claude")),
            RateLimitProvider::Cursor => paths.push(app_data.join("Cursor")),
            _ => {}
        }
    }
    if provider == RateLimitProvider::ClaudeCode {
        paths.extend(crate::rate_limit::claude_desktop::data_dirs(environment));
    }
    if provider == RateLimitProvider::Antigravity {
        paths.extend(antigravity_cli_paths(environment));
    }
    paths
}

fn executable_exists(name: &str, environment: &DiscoveryEnvironment) -> bool {
    !path_executables(name, environment).is_empty()
}

/// Every PATH match for `name`, in the order a Windows shell tries them: PATH
/// directory by directory, and inside one directory the bare name first, then
/// PATHEXT order. Detection and the Claude quota probe share this one rule, so
/// a command can no longer count as installed yet be invisible to the reader.
pub fn path_executables(name: &str, environment: &DiscoveryEnvironment) -> Vec<PathBuf> {
    let candidates = executable_candidates(name, environment);
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    for directory in &environment.path_directories {
        for candidate in &candidates {
            let path = directory.join(candidate);
            if path.is_file() && seen.insert(path.to_string_lossy().to_ascii_lowercase()) {
                found.push(path);
            }
        }
    }
    found
}

fn executable_candidates(name: &str, environment: &DiscoveryEnvironment) -> Vec<String> {
    if !environment.windows {
        return vec![name.to_string()];
    }
    let mut candidates = vec![name.to_string()];
    for extension in &environment.path_extensions {
        let extension = extension.trim();
        let extension = if extension.starts_with('.') {
            extension.to_string()
        } else {
            format!(".{extension}")
        };
        candidates.push(format!("{name}{extension}"));
        candidates.push(format!("{name}{}", extension.to_ascii_lowercase()));
    }
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    #[test]
    fn catalog_routes_only_supported_cli_products_and_supplies_display_names() {
        let mut ids = HashSet::new();
        let mut providers = HashSet::new();
        for product in catalog() {
            assert!(providers.insert(product.provider));
            assert!(!product.display_name.is_empty());
            if let Some(id) = product.cli_id {
                assert!(ids.insert(id));
                assert_eq!(cli_provider(id), Some(product.provider));
                assert_eq!(serde_json::to_value(product.provider).unwrap(), id);
            }
        }
        assert_eq!(
            ids,
            HashSet::from([
                "kimi-code",
                "zcode",
                "grok",
                "opencode-go",
                "cursor",
                "antigravity"
            ])
        );
        assert_eq!(
            cli_provider("antigravity"),
            Some(RateLimitProvider::Antigravity)
        );
        assert_eq!(
            cli_provider("opencode-go"),
            Some(RateLimitProvider::OpenCodeGo)
        );
        assert_eq!(cli_id(RateLimitProvider::Cursor), Some("cursor"));
        assert_eq!(cli_provider("cursor"), Some(RateLimitProvider::Cursor));
        assert_eq!(cli_id(RateLimitProvider::Codex), None);
        assert_eq!(cli_id(RateLimitProvider::ClaudeCode), None);
        assert_eq!(cli_provider("unknown"), None);
    }

    fn environment(root: &Path) -> DiscoveryEnvironment {
        DiscoveryEnvironment {
            home: root.join("home"),
            path_directories: vec![root.join("bin")],
            path_extensions: vec![".EXE".into(), ".CMD".into()],
            app_data: Some(root.join("appdata")),
            local_app_data: Some(root.join("localappdata")),
            program_files: vec![root.join("program-files")],
            windows: true,
        }
    }

    #[test]
    fn discovers_windows_commands_and_ordinary_app_locations() {
        let root = tempfile::tempdir().unwrap();
        let env = environment(root.path());
        fs::create_dir_all(&env.path_directories[0]).unwrap();
        fs::write(env.path_directories[0].join("kimi.CMD"), "").unwrap();
        fs::create_dir_all(
            env.local_app_data
                .as_ref()
                .unwrap()
                .join("Programs")
                .join("Cursor"),
        )
        .unwrap();
        fs::write(
            env.local_app_data
                .as_ref()
                .unwrap()
                .join("Programs")
                .join("Cursor")
                .join("Cursor.exe"),
            "",
        )
        .unwrap();

        let products = discover_with(&env);
        assert!(products
            .iter()
            .any(|product| product.provider == RateLimitProvider::KimiCode && product.is_detected));
        assert!(products
            .iter()
            .any(|product| product.provider == RateLimitProvider::Cursor && product.is_detected));
    }

    #[test]
    fn discovers_opencode_go_in_the_windows_store_and_the_home_relative_root() {
        let store = tempfile::tempdir().unwrap();
        let store_env = environment(store.path());
        assert!(!detected(
            &discover_with(&store_env),
            RateLimitProvider::OpenCodeGo
        ));

        // A per-user `%LOCALAPPDATA%\opencode` folder counts as installed
        // (detection only; OpenCode's data and logins live in the
        // home-relative root checked below).
        fs::create_dir_all(store_env.local_app_data.as_ref().unwrap().join("opencode")).unwrap();
        assert!(detected(
            &discover_with(&store_env),
            RateLimitProvider::OpenCodeGo
        ));

        // The XDG-shaped root the CLI itself resolves still counts too.
        let home = tempfile::tempdir().unwrap();
        let home_env = environment(home.path());
        assert!(!detected(
            &discover_with(&home_env),
            RateLimitProvider::OpenCodeGo
        ));
        fs::create_dir_all(
            home_env
                .home
                .join(".local")
                .join("share")
                .join("opencode"),
        )
        .unwrap();
        assert!(detected(
            &discover_with(&home_env),
            RateLimitProvider::OpenCodeGo
        ));
    }

    #[test]
    fn discovers_antigravity_where_the_cli_finds_agy_and_where_the_apps_install() {
        let touch = |path: PathBuf| {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
        };
        let empty = tempfile::tempdir().unwrap();
        assert!(!detected(
            &discover_with(&environment(empty.path())),
            RateLimitProvider::Antigravity
        ));

        // The official installer's default, outside PATH for an app that was
        // already running when `agy install` edited the user environment.
        let official = tempfile::tempdir().unwrap();
        let env = environment(official.path());
        let installed = env
            .local_app_data
            .as_ref()
            .unwrap()
            .join("agy")
            .join("bin")
            .join("agy.exe");
        assert!(antigravity_cli_paths(&env).contains(&installed));
        touch(installed);
        assert!(detected(
            &discover_with(&env),
            RateLimitProvider::Antigravity
        ));

        let cases: [fn(&DiscoveryEnvironment) -> PathBuf; 5] = [
            |env| env.home.join(".local").join("bin").join("agy.exe"),
            |env| env.path_directories[0].join("agy.exe"),
            |env| env.home.join(".gemini").join("antigravity-cli").join("settings.json"),
            |env| {
                env.local_app_data
                    .as_ref()
                    .unwrap()
                    .join("Programs")
                    .join("Antigravity")
                    .join("Antigravity.exe")
            },
            |env| {
                env.local_app_data
                    .as_ref()
                    .unwrap()
                    .join("Programs")
                    .join("Antigravity IDE")
                    .join("Antigravity IDE.exe")
            },
        ];
        for path in cases {
            let root = tempfile::tempdir().unwrap();
            let env = environment(root.path());
            touch(path(&env));
            assert!(detected(
                &discover_with(&env),
                RateLimitProvider::Antigravity
            ));
        }
    }

    fn detected(products: &[QuotaProduct], provider: RateLimitProvider) -> bool {
        products
            .iter()
            .find(|product| product.provider == provider)
            .is_some_and(|product| product.is_detected)
    }

    #[test]
    fn first_launch_selects_detected_ready_products_only() {
        // Cursor is read through the CLI like the others now (macOS 0.7.4,
        // vibe-usage-app#53), so a detected Cursor is recommended too. A
        // product still waiting for its protocol never is.
        let products = vec![
            QuotaProduct {
                provider: RateLimitProvider::Cursor,
                display_name: "Cursor",
                availability: QuotaProductAvailability::Ready,
                is_detected: true,
            },
            QuotaProduct {
                provider: RateLimitProvider::OpenCodeGo,
                display_name: "OpenCode Go",
                availability: QuotaProductAvailability::PendingProtocol,
                is_detected: true,
            },
            QuotaProduct {
                provider: RateLimitProvider::KimiCode,
                display_name: "Kimi Code",
                availability: QuotaProductAvailability::Ready,
                is_detected: true,
            },
            QuotaProduct {
                provider: RateLimitProvider::Grok,
                display_name: "Grok",
                availability: QuotaProductAvailability::Ready,
                is_detected: true,
            },
        ];
        assert_eq!(
            initial_selection(&products),
            vec![
                RateLimitProvider::Cursor,
                RateLimitProvider::KimiCode,
                RateLimitProvider::Grok
            ]
        );
    }

    #[test]
    fn selecting_a_third_product_keeps_every_choice_in_order() {
        assert_eq!(
            update_selection(
                &[RateLimitProvider::Codex, RateLimitProvider::ClaudeCode],
                RateLimitProvider::Grok,
                true,
            ),
            vec![
                RateLimitProvider::Codex,
                RateLimitProvider::ClaudeCode,
                RateLimitProvider::Grok
            ]
        );
    }

    #[test]
    fn selection_is_deduplicated_without_evicting_earlier_choices() {
        assert_eq!(
            normalize_selection([
                RateLimitProvider::Codex,
                RateLimitProvider::Codex,
                RateLimitProvider::ClaudeCode,
                RateLimitProvider::Grok,
                RateLimitProvider::KimiCode,
                RateLimitProvider::ZCode,
                RateLimitProvider::Cursor,
            ]),
            vec![
                RateLimitProvider::Codex,
                RateLimitProvider::ClaudeCode,
                RateLimitProvider::Grok,
                RateLimitProvider::KimiCode,
                RateLimitProvider::ZCode,
                RateLimitProvider::Cursor
            ]
        );
    }
}
