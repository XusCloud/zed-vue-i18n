use std::fs;
use std::path::Path;
use zed_extension_api::{self as zed, Result};

struct VueI18nExtension {
    cached_binary_path: Option<String>,
}

impl VueI18nExtension {
    fn language_server_binary_path(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        _worktree: &zed::Worktree,
    ) -> Result<String> {
        // 1. 如果已有缓存且文件存在，直接返回
        if let Some(path) = &self.cached_binary_path {
            if Path::new(path).is_file() {
                return Ok(path.clone());
            }
        }

        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );

        // 2. 获取 GitHub 上的最新 Release
        let release = zed::latest_github_release(
            "XusCloud/zed-vue-i18n",
            zed::GithubReleaseOptions {
                require_assets: true,
                pre_release: false,
            },
        )?;

        // 3. 匹配当前平台架构对应的 Asset 名称
        let (platform, arch) = zed::current_platform();
        let asset_name = match (platform, arch) {
            (zed::Os::Mac, zed::Architecture::Aarch64) => "vue-i18n-server-aarch64-apple-darwin.gz",
            (zed::Os::Mac, zed::Architecture::X8664) => "vue-i18n-server-x86_64-apple-darwin.gz",
            (zed::Os::Linux, zed::Architecture::X8664) => {
                "vue-i18n-server-x86_64-unknown-linux-gnu.gz"
            }
            (zed::Os::Linux, zed::Architecture::Aarch64) => {
                "vue-i18n-server-aarch64-unknown-linux-gnu.gz"
            }
            (zed::Os::Windows, zed::Architecture::X8664) => {
                "vue-i18n-server-x86_64-pc-windows-msvc.gz"
            }
            _ => return Err(format!("Unsupported platform: {platform:?}/{arch:?}")),
        };

        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == asset_name)
            .ok_or_else(|| format!("No release asset found matching '{asset_name}'"))?;

        let version_dir = format!("vue-i18n-server-{}", release.version);
        let binary_path = if platform == zed::Os::Windows {
            format!("{version_dir}/vue-i18n-server.exe")
        } else {
            format!("{version_dir}/vue-i18n-server")
        };

        // 4. 下载并解压可执行文件
        if !Path::new(&binary_path).is_file() {
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::Downloading,
            );

            // 先确保版本目录已创建
            fs::create_dir_all(&version_dir)
                .map_err(|e| format!("Failed to create directory '{version_dir}': {e}"))?;

            zed::download_file(
                &asset.download_url,
                &binary_path,
                zed::DownloadedFileType::Gzip,
            )?;

            zed::make_file_executable(&binary_path)?;
        }

        self.cached_binary_path = Some(binary_path.clone());
        Ok(binary_path)
    }
}

impl zed::Extension for VueI18nExtension {
    fn new() -> Self {
        Self {
            cached_binary_path: None,
        }
    }

    fn language_server_initialization_options(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<Option<zed::serde_json::Value>> {
        // 读取 Zed settings.json 中 lsp.vue-i18n.settings 下的配置内容
        let settings = zed::settings::LspSettings::for_worktree("vue-i18n", worktree)?;
        Ok(settings.settings)
    }

    fn language_server_command(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        Ok(zed::Command {
            command: self.language_server_binary_path(language_server_id, worktree)?,
            args: vec![],
            env: Default::default(),
        })
    }
}

zed::register_extension!(VueI18nExtension);
