use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub fn is_supported_locale(name: &str) -> bool {
    let normalized = name.trim().replace('_', "-").to_lowercase();
    matches!(
        normalized.as_str(),
        "zh" | "zh-cn"
            | "zh-tw"
            | "zh-hk"
            | "zh-hans"
            | "zh-hant"
            | "cn"
            | "en"
            | "en-us"
            | "en-gb"
            | "ja"
            | "jp"
            | "ja-jp"
            | "ko"
            | "kr"
            | "ko-kr"
            | "fr"
            | "fr-fr"
            | "de"
            | "de-de"
            | "es"
            | "es-es"
            | "ru"
            | "ru-ru"
            | "pt"
            | "pt-br"
            | "pt-pt"
            | "it"
            | "ar"
            | "th"
            | "vi"
            | "id"
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlayHintsConfig {
    #[serde(default = "default_inlay_enabled")]
    pub enabled: bool,
    #[serde(default = "default_inlay_max_length")]
    pub max_length: usize,
}

fn default_inlay_enabled() -> bool {
    true
}
fn default_inlay_max_length() -> usize {
    24
}

impl Default for InlayHintsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_length: 24,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageConfig {
    pub root: String,
    #[serde(default)]
    pub default_locale: String,
    // 子包不设置默认 dirs/files，为空时继承父配置
    #[serde(default)]
    pub locale_dirs: Vec<String>,
    #[serde(default)]
    pub locale_files: Vec<String>,
    #[serde(default)]
    pub inlay_hints: InlayHintsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConfig {
    #[serde(default = "default_locale")]
    pub default_locale: String,
    #[serde(default = "default_locale_dirs")]
    pub locale_dirs: Vec<String>,
    #[serde(default)]
    pub locale_files: Vec<String>,
    #[serde(default)]
    pub inlay_hints: InlayHintsConfig,
    #[serde(default)]
    pub packages: Vec<PackageConfig>,
}

fn default_locale() -> String {
    "zh".to_string()
}

fn default_locale_dirs() -> Vec<String> {
    vec![
        "src/locales".into(),
        "src/i18n".into(),
        "locales".into(),
        "i18n".into(),
    ]
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            default_locale: default_locale(),
            locale_dirs: default_locale_dirs(),
            locale_files: vec![],
            inlay_hints: InlayHintsConfig::default(),
            packages: vec![],
        }
    }
}

pub struct ProjectContext {
    pub root: PathBuf,
    pub workspace_root: PathBuf,
    pub config: ProjectConfig,
}

impl ProjectContext {
    /// 辅助方法：解析 locale_files 的真实绝对路径
    /// 优先从子包根目录找，找不到时自动退回工作区根目录找（兼容 node_modules 位于根目录的情况）
    pub fn resolve_file_path(&self, relative_path: &str) -> Option<PathBuf> {
        let path_in_pkg = self.root.join(relative_path);
        if path_in_pkg.is_file() {
            return Some(path_in_pkg);
        }
        let path_in_ws = self.workspace_root.join(relative_path);
        if path_in_ws.is_file() {
            return Some(path_in_ws);
        }
        None
    }
}

pub struct ConfigManager {
    pub config: ProjectConfig,
}

impl ConfigManager {
    pub fn new() -> Self {
        Self {
            config: ProjectConfig::default(),
        }
    }

    pub fn resolve_project_context(
        &self,
        file_path: &Path,
        workspace_root: &Path,
    ) -> ProjectContext {
        let mut candidates = vec![ProjectContext {
            root: workspace_root.to_path_buf(),
            workspace_root: workspace_root.to_path_buf(),
            config: self.config.clone(),
        }];

        for pkg in &self.config.packages {
            let pkg_root = workspace_root.join(&pkg.root);
            let mut pkg_config = self.config.clone();

            // 只有当子包显式配置了属性时才覆盖，否则继承全局配置
            if !pkg.default_locale.trim().is_empty() {
                pkg_config.default_locale = pkg.default_locale.clone();
            }
            if !pkg.locale_dirs.is_empty() {
                pkg_config.locale_dirs = pkg.locale_dirs.clone();
            }
            if !pkg.locale_files.is_empty() {
                pkg_config.locale_files = pkg.locale_files.clone();
            }
            pkg_config.inlay_hints = pkg.inlay_hints.clone();

            candidates.push(ProjectContext {
                root: pkg_root,
                workspace_root: workspace_root.to_path_buf(),
                config: pkg_config,
            });
        }

        candidates
            .into_iter()
            .filter(|c| file_path.starts_with(&c.root))
            .max_by_key(|c| c.root.as_os_str().len())
            .unwrap_or_else(|| ProjectContext {
                root: workspace_root.to_path_buf(),
                workspace_root: workspace_root.to_path_buf(),
                config: self.config.clone(),
            })
    }
}

impl ProjectConfig {
    pub fn update_from_json(target: &mut ProjectConfig, raw: ProjectConfig) {
        if !raw.default_locale.trim().is_empty() {
            target.default_locale = raw.default_locale;
        }
        if !raw.locale_dirs.is_empty() {
            target.locale_dirs = raw.locale_dirs;
        }
        if !raw.locale_files.is_empty() {
            target.locale_files = raw.locale_files;
        }
        target.inlay_hints = raw.inlay_hints;
        if !raw.packages.is_empty() {
            target.packages = raw.packages;
        }
    }
}
