use crate::config::{is_supported_locale, ProjectContext};
use crate::parser::I18nParser;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value as JsonValue;
use serde_yaml::Value as YamlValue;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tower_lsp::lsp_types::Position;
use walkdir::WalkDir;

static VUE_I18N_TAG_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)<i18n([^>]*)>([\s\S]*?)</i18n>").unwrap());

#[derive(Debug, Clone, Default)]
pub struct LocaleData {
    pub files: Vec<PathBuf>,
    pub flat: HashMap<String, String>,
}

#[derive(Debug, Default)]
pub struct VueSfcResult {
    pub locales: HashMap<String, LocaleData>,
    pub positions: HashMap<String, HashMap<String, Position>>,
}

pub struct LocaleStore;

impl LocaleStore {
    pub fn flatten_json(val: &JsonValue, prefix: &str, out: &mut HashMap<String, String>) {
        if let Some(obj) = val.as_object() {
            for (k, v) in obj {
                let full_key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{}.{}", prefix, k)
                };
                if v.is_object() {
                    Self::flatten_json(v, &full_key, out);
                } else if let Some(s) = v.as_str() {
                    out.insert(full_key, s.to_string());
                } else {
                    out.insert(full_key, v.to_string());
                }
            }
        }
    }

    pub fn flatten_yaml(val: &YamlValue, prefix: &str, out: &mut HashMap<String, String>) {
        if let Some(mapping) = val.as_mapping() {
            for (k, v) in mapping {
                if let Some(k_str) = k.as_str() {
                    let full_key = if prefix.is_empty() {
                        k_str.to_string()
                    } else {
                        format!("{}.{}", prefix, k_str)
                    };
                    if v.is_mapping() {
                        Self::flatten_yaml(v, &full_key, out);
                    } else if let Some(s) = v.as_str() {
                        out.insert(full_key, s.to_string());
                    } else {
                        out.insert(full_key, format!("{:?}", v));
                    }
                }
            }
        }
    }

    pub fn extract_vue_sfc_i18n(text: &str) -> VueSfcResult {
        let mut result = VueSfcResult::default();
        if !text.contains("<i18n") {
            return result;
        }

        for cap in VUE_I18N_TAG_RE.captures_iter(text) {
            let attrs = &cap[1];
            let content = &cap[2];
            if content.trim().is_empty() {
                continue;
            }

            let content_offset = cap.get(2).map_or(0, |m| m.start());
            let content_start_pos = I18nParser::offset_to_position(text, content_offset);

            let lang_attr = Regex::new(r#"locale=["']([^"']+)["']"#)
                .unwrap()
                .captures(attrs)
                .map(|c| c[1].to_string());

            let json_parsed: Option<JsonValue> = serde_json::from_str(content).ok();
            let yaml_parsed: Option<YamlValue> = if json_parsed.is_none() {
                serde_yaml::from_str(content).ok()
            } else {
                None
            };

            let mut process_locale = |lang: &str, flat: HashMap<String, String>| {
                let entry = result.locales.entry(lang.to_string()).or_default();
                entry.flat.extend(flat.clone());

                let pos_map = result.positions.entry(lang.to_string()).or_default();
                for k in flat.keys() {
                    if let Some(rel_pos) = I18nParser::find_key_location(content, k) {
                        pos_map.insert(
                            k.clone(),
                            Position {
                                line: content_start_pos.line + rel_pos.line,
                                character: if rel_pos.line == 0 {
                                    content_start_pos.character + rel_pos.character
                                } else {
                                    rel_pos.character
                                },
                            },
                        );
                    }
                }
            };

            if let Some(lang) = lang_attr {
                if is_supported_locale(&lang) {
                    let mut flat = HashMap::new();
                    if let Some(json) = json_parsed {
                        Self::flatten_json(&json, "", &mut flat);
                    } else if let Some(yaml) = yaml_parsed {
                        Self::flatten_yaml(&yaml, "", &mut flat);
                    }
                    process_locale(&lang, flat);
                }
            } else if let Some(json) = json_parsed {
                if let Some(obj) = json.as_object() {
                    for (k, v) in obj {
                        if is_supported_locale(k) {
                            let mut flat = HashMap::new();
                            Self::flatten_json(v, "", &mut flat);
                            process_locale(k, flat);
                        }
                    }
                }
            } else if let Some(yaml) = yaml_parsed {
                if let Some(map) = yaml.as_mapping() {
                    for (k, v) in map {
                        if let Some(lang_str) = k.as_str() {
                            if is_supported_locale(lang_str) {
                                let mut flat = HashMap::new();
                                Self::flatten_yaml(v, "", &mut flat);
                                process_locale(lang_str, flat);
                            }
                        }
                    }
                }
            }
        }
        result
    }

    /// 解析 JS/MJS/TS 文件中的对象字面量并转换为 JsonValue
    pub fn parse_js_mjs_to_json(text: &str) -> Option<JsonValue> {
        // 1. 移除 JS 注释 (// 和 /* */)
        let re_comments = Regex::new(r"//.*|/\*[\s\S]*?\*/").unwrap();
        let cleaned = re_comments.replace_all(text, "");

        // 2. 寻找导出的对象起始 `{`
        let re_object_start = Regex::new(
            r"(?:export\s+default|module\.exports\s*=|\bvar\b|\bconst\b|\blet\b)[\s\S]*?\{",
        )
        .unwrap();

        let start_idx = if let Some(m) = re_object_start.find(&cleaned) {
            cleaned[m.start()..m.end()]
                .rfind('{')
                .map(|i| m.start() + i)
        } else {
            cleaned.find('{')
        }?;

        // 3. 利用精确的括号配对，取出整个 JS 对象字符串
        let end_idx = Self::find_matching_brace(&cleaned, start_idx)?;
        let obj_str = &cleaned[start_idx..=end_idx];

        // 尝试直接作为标准 JSON 解析
        if let Ok(json) = serde_json::from_str::<JsonValue>(obj_str) {
            return Some(json);
        }

        // 4. 清理并转义 JS 语法特性使之满足 JSON 规范
        let mut json_str = obj_str.to_string();

        // 将单引号字符串 'xxx' 替换为双引号 "xxx"
        let re_single_quote = Regex::new(r"'([^'\\]*(?:\\.[^'\\]*)*)'").unwrap();
        json_str = re_single_quote.replace_all(&json_str, "\"$1\"").to_string();

        // 给未加双引号的 key 补齐双引号 (例如 el: 或 name: -> "el":)
        let re_unquoted_key = Regex::new(r"(?m)([{,]\s*)([a-zA-Z_][a-zA-Z0-9_-]*)\s*:").unwrap();
        json_str = re_unquoted_key
            .replace_all(&json_str, "$1\"$2\":")
            .to_string();

        // 清理末尾逗号 ,} 或 ,]
        let re_trailing_comma = Regex::new(r",\s*([}\]])").unwrap();
        while re_trailing_comma.is_match(&json_str) {
            json_str = re_trailing_comma.replace_all(&json_str, "$1").to_string();
        }

        serde_json::from_str::<JsonValue>(&json_str).ok()
    }

    /// 匹配成对的大括号（自动跳过字符串内部的字符）
    fn find_matching_brace(s: &str, start_idx: usize) -> Option<usize> {
        let mut depth = 0;
        let mut in_string = false;
        let mut string_char = ' ';
        let mut escape = false;

        for (i, c) in s[start_idx..].char_indices() {
            let idx = start_idx + i;
            if in_string {
                if escape {
                    escape = false;
                } else if c == '\\' {
                    escape = true;
                } else if c == string_char {
                    in_string = false;
                }
            } else {
                match c {
                    '"' | '\'' | '`' => {
                        in_string = true;
                        string_char = c;
                    }
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(idx);
                        }
                    }
                    _ => {}
                }
            }
        }
        None
    }

    fn process_single_locale_file(
        path: &Path,
        locales: &mut HashMap<String, LocaleData>,
        locale_texts: &mut HashMap<PathBuf, String>,
    ) {
        eprintln!("[i18n-lens] Processing file: {:?}", path);
        if !path.is_file() {
            eprintln!(
                "[i18n-lens] File does not exist or is not a file: {:?}",
                path
            );
            return;
        }

        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        if !matches!(
            ext,
            "json" | "yaml" | "yml" | "js" | "mjs" | "cjs" | "ts" | "mts"
        ) {
            eprintln!("[i18n-lens] File has unsupported extension: {:?}", path);
            return;
        }

        if let Ok(text) = fs::read_to_string(path) {
            eprintln!("[i18n-lens] Successfully read file: {:?}", path);
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let parent_name = path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("");

            let lang = if is_supported_locale(stem) {
                Some(stem.to_string())
            } else if is_supported_locale(parent_name) {
                Some(parent_name.to_string())
            } else {
                None
            };

            // 依据扩展名解析 JSON / JS / TS 文件
            let json_opt = if ext == "json" {
                serde_json::from_str::<JsonValue>(&text).ok()
            } else if matches!(ext, "js" | "mjs" | "cjs" | "ts" | "mts") {
                serde_json::from_str::<JsonValue>(&text)
                    .ok()
                    .or_else(|| Self::parse_js_mjs_to_json(&text))
            } else {
                None
            };

            if let Some(json) = json_opt {
                eprintln!("[i18n-lens] Parsed JSON/JS for file: {:?}", path);
                if let Some(ref l) = lang {
                    eprintln!("[i18n-lens] Identified language from name: {}", l);
                    let mut flat = HashMap::new();
                    Self::flatten_json(&json, "", &mut flat);
                    let entry = locales.entry(l.clone()).or_default();
                    entry.files.push(path.to_path_buf());
                    entry.flat.extend(flat);
                } else if let Some(obj) = json.as_object() {
                    eprintln!(
                        "[i18n-lens] No language in name, checking top-level keys for: {:?}",
                        path
                    );
                    for (k, v) in obj {
                        if is_supported_locale(k) {
                            eprintln!("[i18n-lens] Found supported language key: {}", k);
                            let mut flat = HashMap::new();
                            Self::flatten_json(v, "", &mut flat);
                            let entry = locales.entry(k.clone()).or_default();
                            if !entry.files.contains(&path.to_path_buf()) {
                                entry.files.push(path.to_path_buf());
                            }
                            entry.flat.extend(flat);
                        }
                    }
                }
                locale_texts.insert(path.to_path_buf(), text);
            } else if let Ok(yaml) = serde_yaml::from_str::<YamlValue>(&text) {
                eprintln!("[i18n-lens] Parsed YAML for file: {:?}", path);
                if let Some(ref l) = lang {
                    eprintln!("[i18n-lens] Identified language from name: {}", l);
                    let mut flat = HashMap::new();
                    Self::flatten_yaml(&yaml, "", &mut flat);
                    let entry = locales.entry(l.clone()).or_default();
                    entry.files.push(path.to_path_buf());
                    entry.flat.extend(flat);
                } else if let Some(map) = yaml.as_mapping() {
                    eprintln!(
                        "[i18n-lens] No language in name, checking top-level keys for: {:?}",
                        path
                    );
                    for (k, v) in map {
                        if let Some(k_str) = k.as_str() {
                            if is_supported_locale(k_str) {
                                eprintln!("[i18n-lens] Found supported language key: {}", k_str);
                                let mut flat = HashMap::new();
                                Self::flatten_yaml(v, "", &mut flat);
                                let entry = locales.entry(k_str.to_string()).or_default();
                                if !entry.files.contains(&path.to_path_buf()) {
                                    entry.files.push(path.to_path_buf());
                                }
                                entry.flat.extend(flat);
                            }
                        }
                    }
                }
                locale_texts.insert(path.to_path_buf(), text);
            } else {
                eprintln!(
                    "[i18n-lens] Failed to parse as JSON, JS, or YAML: {:?}",
                    path
                );
            }
        } else {
            eprintln!("[i18n-lens] Failed to read file to string: {:?}", path);
        }
    }

    pub fn load_locales_for_context(
        context: &ProjectContext,
    ) -> (HashMap<String, LocaleData>, HashMap<PathBuf, String>) {
        eprintln!(
            "[i18n-lens] Loading locales for context. Root: {:?}",
            context.root
        );
        eprintln!("[i18n-lens] Workspace Root: {:?}", context.workspace_root);
        eprintln!(
            "[i18n-lens] Config locale_dirs: {:?}",
            context.config.locale_dirs
        );
        eprintln!(
            "[i18n-lens] Config locale_files: {:?}",
            context.config.locale_files
        );

        let mut locales: HashMap<String, LocaleData> = HashMap::new();
        let mut locale_texts: HashMap<PathBuf, String> = HashMap::new();

        // 1. 处理目录
        for dir in &context.config.locale_dirs {
            let full_dir = context.root.join(dir);
            eprintln!("[i18n-lens] Scanning directory: {:?}", full_dir);
            if !full_dir.exists() {
                eprintln!("[i18n-lens] Directory does not exist: {:?}", full_dir);
                continue;
            }

            for entry in WalkDir::new(full_dir).into_iter().filter_map(|e| e.ok()) {
                Self::process_single_locale_file(entry.path(), &mut locales, &mut locale_texts);
            }
        }

        // 2. 处理单文件 (解析 .json, .js, .mjs, .cjs, .ts 等)
        for file in &context.config.locale_files {
            if let Some(full_path) = context.resolve_file_path(file) {
                eprintln!("[i18n-lens] Loading specific locale file: {:?}", full_path);
                Self::process_single_locale_file(&full_path, &mut locales, &mut locale_texts);
            } else {
                eprintln!("[i18n-lens] Could not resolve file path for: {}", file);
            }
        }

        eprintln!("[i18n-lens] Total locales loaded: {}", locales.len());
        (locales, locale_texts)
    }
}
