mod config;
mod locale_store;
mod parser;
mod service;

use config::ConfigManager;
use service::I18nService;
use std::collections::HashMap;
use std::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

struct Backend {
    client: Client,
    config_manager: RwLock<ConfigManager>,
    workspace_root: RwLock<Option<std::path::PathBuf>>,
    document_map: RwLock<HashMap<Url, String>>,
    service: I18nService,
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let root_path = params
            .workspace_folders
            .and_then(|folders| folders.get(0).cloned())
            .and_then(|f| f.uri.to_file_path().ok())
            .or_else(|| params.root_uri.and_then(|u| u.to_file_path().ok()));

        if let Some(root) = root_path {
            *self.workspace_root.write().unwrap() = Some(root);
        }

        // 直接通过 Zed 配置的 initializationOptions 更新配置
        if let Some(options) = params.initialization_options {
            if let Ok(parsed) = serde_json::from_value::<crate::config::ProjectConfig>(options) {
                let mut cm = self.config_manager.write().unwrap();
                crate::config::ProjectConfig::update_from_json(&mut cm.config, parsed);
            }
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".into(), "\"".into(), "'".into()]),
                    ..Default::default()
                }),
                inlay_hint_provider: Some(OneOf::Left(true)),
                definition_provider: Some(OneOf::Left(true)),
                ..ServerCapabilities::default()
            },
            ..InitializeResult::default()
        })
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.document_map.write().unwrap().insert(
            params.text_document.uri.clone(),
            params.text_document.text.clone(),
        );
        self.validate_document(params.text_document.uri, params.text_document.text)
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.into_iter().next() {
            self.document_map
                .write()
                .unwrap()
                .insert(params.text_document.uri.clone(), change.text.clone());
            self.validate_document(params.text_document.uri, change.text)
                .await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.document_map
            .write()
            .unwrap()
            .remove(&params.text_document.uri);
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let docs = self.document_map.read().unwrap();

        if let Some(text) = docs.get(&uri) {
            if let Ok(file_path) = uri.to_file_path() {
                let cm = self.config_manager.read().unwrap();
                let root = self.get_workspace_root(&file_path);
                let context = cm.resolve_project_context(&file_path, &root);
                return Ok(self.service.get_hover(text, pos, &context));
            }
        }
        Ok(None)
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let docs = self.document_map.read().unwrap();

        if let Some(text) = docs.get(&uri) {
            if let Ok(file_path) = uri.to_file_path() {
                let cm = self.config_manager.read().unwrap();
                let root = self.get_workspace_root(&file_path);
                let context = cm.resolve_project_context(&file_path, &root);
                let items = self.service.get_completions(text, pos, &context);
                return Ok(Some(CompletionResponse::Array(items)));
            }
        }
        Ok(Some(CompletionResponse::Array(vec![])))
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        let uri = params.text_document.uri;
        let docs = self.document_map.read().unwrap();

        if let Some(text) = docs.get(&uri) {
            if let Ok(file_path) = uri.to_file_path() {
                let cm = self.config_manager.read().unwrap();
                let root = self.get_workspace_root(&file_path);
                let context = cm.resolve_project_context(&file_path, &root);
                return Ok(Some(self.service.get_inlay_hints(
                    text,
                    params.range,
                    &context,
                )));
            }
        }
        Ok(Some(vec![]))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let docs = self.document_map.read().unwrap();

        if let Some(text) = docs.get(&uri) {
            if let Ok(file_path) = uri.to_file_path() {
                let cm = self.config_manager.read().unwrap();
                let root = self.get_workspace_root(&file_path);
                let context = cm.resolve_project_context(&file_path, &root);
                let locs = self.service.get_definition(&uri, text, pos, &context);
                if !locs.is_empty() {
                    return Ok(Some(GotoDefinitionResponse::Array(locs)));
                }
            }
        }
        Ok(None)
    }
}

impl Backend {
    fn get_workspace_root(&self, file_path: &std::path::Path) -> std::path::PathBuf {
        self.workspace_root
            .read()
            .unwrap()
            .clone()
            .unwrap_or_else(|| file_path.parent().unwrap_or(file_path).to_path_buf())
    }

    async fn validate_document(&self, uri: Url, text: String) {
        if let Ok(file_path) = uri.to_file_path() {
            let diagnostics = {
                let cm = self.config_manager.read().unwrap();
                let root = self.get_workspace_root(&file_path);
                let context = cm.resolve_project_context(&file_path, &root);
                let (mut locales, _) =
                    locale_store::LocaleStore::load_locales_for_context(&context);
                let sfc = locale_store::LocaleStore::extract_vue_sfc_i18n(&text);
                for (lang, data) in sfc.locales {
                    locales.entry(lang).or_default().flat.extend(data.flat);
                }

                self.service.get_diagnostics(&text, &locales, &file_path)
            };

            self.client
                .publish_diagnostics(uri, diagnostics, None)
                .await;
        }
    }
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| Backend {
        client,
        config_manager: RwLock::new(ConfigManager::new()),
        workspace_root: RwLock::new(None),
        document_map: RwLock::new(HashMap::new()),
        service: I18nService,
    });

    Server::new(stdin, stdout, socket).serve(service).await;
}
