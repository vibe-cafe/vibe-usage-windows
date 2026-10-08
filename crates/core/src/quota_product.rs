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
    use QuotaProductAvailability::{PendingProtocol, Ready};
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
            availability: PendingProtocol,
            cli_id: None,
            command: "cursor",
            relative_paths: &[".cursor"],
        },
    ]
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
            // OpenCode keeps its data + credential store under
            // `%LOCALAPPDATA%\opencode` on Windows, which the home-relative
            // `relative_paths` cannot express.
            RateLimitProvider::OpenCodeGo => paths.push(root.join("opencode")),
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
    paths
}

fn executable_exists(name: &str, environment: &DiscoveryEnvironment) -> bool {
    environment.path_directories.iter().any(|directory| {
        executable_candidates(name, environment)
            .iter()
            .any(|candidate| directory.join(candidate).is_file())
    })
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
            HashSet::from(["kimi-code", "zcode", "grok", "opencode-go"])
        );
        assert_eq!(
            cli_provider("opencode-go"),
            Some(RateLimitProvider::OpenCodeGo)
        );
        assert_eq!(cli_id(RateLimitProvider::Cursor), None);
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

        // Windows keeps OpenCode's data and credential store in
        // `%LOCALAPPDATA%\opencode`, outside the home-relative roots.
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

    fn detected(products: &[QuotaProduct], provider: RateLimitProvider) -> bool {
        products
            .iter()
            .find(|product| product.provider == provider)
            .is_some_and(|product| product.is_detected)
    }

    #[test]
    fn first_launch_selects_detected_ready_products_but_not_cursor() {
        let products = vec![
            QuotaProduct {
                provider: RateLimitProvider::Cursor,
                display_name: "Cursor",
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
            vec![RateLimitProvider::KimiCode, RateLimitProvider::Grok]
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
