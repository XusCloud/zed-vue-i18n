use crate::config::ProjectContext;
use crate::locale_store::{LocaleData, LocaleStore};
use crate::parser::I18nParser;
use std::collections::{HashMap, HashSet};
use tower_lsp::lsp_types::*;
use url::Url;

pub struct I18nService;

fn is_lang_match(lang_a: &str, lang_b: &str) -> bool {
    let a = lang_a.to_lowercase().replace('_', "-");
    let b = lang_b.to_lowercase().replace('_', "-");
    a == b || a.starts_with(&b) || b.starts_with(&a)
}

impl I18nService {
    pub fn get_hover(&self, text: &str, pos: Position, context: &ProjectContext) -> Option<Hover> {
        let keys = I18nParser::extract_i18n_keys(text);
        let offset = I18nParser::position_to_offset(text, pos);
        let item = keys
            .iter()
            .find(|k| k.start_offset <= offset && offset <= k.end_offset)?;

        let (mut locales, _) = LocaleStore::load_locales_for_context(context);
        let sfc = LocaleStore::extract_vue_sfc_i18n(text);
        for (lang, data) in sfc.locales {
            locales.entry(lang).or_default().flat.extend(data.flat);
        }

        let mut lines = vec![
            format!("**{}**", item.key),
            "".into(),
            "| Locale | Text |".into(),
            "|---|---|".into(),
        ];
        for (lang, data) in &locales {
            let val = data
                .flat
                .get(&item.key)
                .cloned()
                .unwrap_or_else(|| "Missing".into());
            lines.push(format!("| {} | {} |", lang, val.replace('|', "\\|")));
        }

        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: lines.join("\n"),
            }),
            range: Some(item.range),
        })
    }

    pub fn get_completions(
        &self,
        text: &str,
        pos: Position,
        context: &ProjectContext,
    ) -> Vec<CompletionItem> {
        let offset = I18nParser::position_to_offset(text, pos);
        if !I18nParser::is_cursor_in_t_call(text, offset) {
            return vec![];
        }

        let (mut locales, _) = LocaleStore::load_locales_for_context(context);
        let sfc = LocaleStore::extract_vue_sfc_i18n(text);
        for (lang, data) in sfc.locales {
            locales.entry(lang).or_default().flat.extend(data.flat);
        }

        let default_lang = &context.config.default_locale;
        let default_locale_data = locales.get(default_lang).or_else(|| {
            let norm = default_lang.to_lowercase().replace('_', "-");
            locales
                .iter()
                .find(|(k, _)| {
                    let nk = k.to_lowercase().replace('_', "-");
                    nk == norm || norm.starts_with(&nk) || nk.starts_with(&norm)
                })
                .map(|(_, v)| v)
        });

        let mut all_keys = HashSet::new();
        for data in locales.values() {
            for k in data.flat.keys() {
                all_keys.insert(k.clone());
            }
        }

        let mut items: Vec<CompletionItem> = all_keys
            .into_iter()
            .map(|key| {
                let detail_text = default_locale_data.and_then(|d| d.flat.get(&key)).cloned();

                CompletionItem {
                    label: key.clone(),
                    kind: Some(CompletionItemKind::VALUE),
                    detail: detail_text,
                    filter_text: Some(key.clone()),
                    insert_text: Some(key),
                    ..Default::default()
                }
            })
            .collect();

        items.sort_by(|a, b| a.label.cmp(&b.label));
        items
    }

    pub fn get_diagnostics(
        &self,
        text: &str,
        locales: &HashMap<String, LocaleData>,
        file_path: &std::path::Path,
    ) -> Vec<Diagnostic> {
        if locales.is_empty() {
            return vec![];
        }

        let extension = file_path.extension().and_then(|s| s.to_str());
        let is_js_ts = extension == Some("js") || extension == Some("ts");

        let mut diagnostics = Vec::new();
        for item in I18nParser::extract_i18n_keys(text) {
            let missing_count = locales
                .values()
                .filter(|loc| !loc.flat.contains_key(&item.key))
                .count();
            if missing_count == locales.len() {
                if !is_js_ts {
                    diagnostics.push(Diagnostic {
                        range: item.range,
                        severity: Some(DiagnosticSeverity::WARNING),
                        message: format!("Missing i18n key: {}", item.key),
                        source: Some("i18n-lens".into()),
                        ..Default::default()
                    });
                }
            }
        }
        diagnostics
    }

    pub fn get_inlay_hints(
        &self,
        text: &str,
        _range: Range,
        context: &ProjectContext,
    ) -> Vec<InlayHint> {
        if !context.config.inlay_hints.enabled {
            return vec![];
        }

        let (mut locales, _) = LocaleStore::load_locales_for_context(context);
        let sfc = LocaleStore::extract_vue_sfc_i18n(text);
        for (lang, data) in sfc.locales {
            locales.entry(lang).or_default().flat.extend(data.flat);
        }

        let default_lang = &context.config.default_locale;
        // 增加语言模糊匹配（如 "zh-CN" 可以匹配到 "zh"）
        let preferred = locales
            .get(default_lang)
            .or_else(|| {
                let norm_default = default_lang.to_lowercase().replace('_', "-");
                locales
                    .iter()
                    .find(|(k, _)| {
                        let norm_k = k.to_lowercase().replace('_', "-");
                        norm_k == norm_default
                            || norm_default.starts_with(&norm_k)
                            || norm_k.starts_with(&norm_default)
                    })
                    .map(|(_, v)| v)
            })
            .or_else(|| locales.values().next());

        let preferred = match preferred {
            Some(p) => p,
            None => return vec![],
        };

        let max_len = context.config.inlay_hints.max_length;
        I18nParser::extract_i18n_keys(text)
            .into_iter()
            .filter_map(|item| {
                let val = preferred.flat.get(&item.key)?;
                let display_val = if val.chars().count() > max_len {
                    format!("{}…", val.chars().take(max_len - 1).collect::<String>())
                } else {
                    val.clone()
                };

                Some(InlayHint {
                    position: item.range.end,
                    label: InlayHintLabel::String(format!("{}", display_val)),
                    padding_left: Some(true),
                    padding_right: None,
                    kind: None, // 设为 None 以兼容 Zed 的 show_other_hints
                    tooltip: Some(InlayHintTooltip::String(val.clone())),
                    data: None,
                    text_edits: None,
                })
            })
            .collect()
    }

    pub fn get_definition(
        &self,
        doc_uri: &Url,
        text: &str,
        pos: Position,
        context: &ProjectContext,
    ) -> Vec<Location> {
        let keys = I18nParser::extract_i18n_keys(text);
        let offset = I18nParser::position_to_offset(text, pos);
        let item = match keys
            .iter()
            .find(|k| k.start_offset <= offset && offset <= k.end_offset)
        {
            Some(i) => i,
            None => return vec![],
        };

        let default_lang = &context.config.default_locale;

        // 1. 优先搜索 Vue SFC <i18n> 中的默认语言
        let sfc = LocaleStore::extract_vue_sfc_i18n(text);
        for (lang, pos_map) in &sfc.positions {
            if is_lang_match(lang, default_lang) {
                if let Some(&p) = pos_map.get(&item.key) {
                    return vec![Location {
                        uri: doc_uri.clone(),
                        range: Range {
                            start: p,
                            end: Position {
                                line: p.line,
                                character: p.character + item.key.len() as u32,
                            },
                        },
                    }];
                }
            }
        }

        // 2. 优先在全局语言包中搜索默认语言 (例如 zh / zh-CN)
        let (locales, locale_texts) = LocaleStore::load_locales_for_context(context);
        let target_lang = locales
            .keys()
            .find(|l| is_lang_match(l, default_lang))
            .cloned();

        if let Some(ref matched_lang) = target_lang {
            if let Some(data) = locales.get(matched_lang) {
                for path in &data.files {
                    if let Some(file_text) = locale_texts.get(path) {
                        // 包含两种情况：
                        // ① common.json 内顶层为 "zh" -> 拼接查找 "zh.common.generate"
                        // ② zh/test.json 独立语言文件 -> 直接查找 "aaa"
                        let full_key_with_lang = format!("{}.{}", matched_lang, item.key);
                        let pos = I18nParser::find_key_location(file_text, &full_key_with_lang)
                            .or_else(|| I18nParser::find_key_location(file_text, &item.key));

                        if let Some(p) = pos {
                            if let Ok(file_url) = Url::from_file_path(path) {
                                return vec![Location {
                                    uri: file_url,
                                    range: Range {
                                        start: p,
                                        end: Position {
                                            line: p.line,
                                            character: p.character + item.key.len() as u32,
                                        },
                                    },
                                }];
                            }
                        }
                    }
                }
            }
        }

        // 3. 兜底策略：如果默认语言未搜到，再全局查找其他语言
        let mut fallback_locations = Vec::new();
        for (path, file_text) in locale_texts {
            if let Some(p) = I18nParser::find_key_location(&file_text, &item.key) {
                if let Ok(file_url) = Url::from_file_path(path) {
                    fallback_locations.push(Location {
                        uri: file_url,
                        range: Range {
                            start: p,
                            end: Position {
                                line: p.line,
                                character: p.character + item.key.len() as u32,
                            },
                        },
                    });
                }
            }
        }

        fallback_locations
    }
}
