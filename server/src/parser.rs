use once_cell::sync::Lazy;
use regex::Regex;
use tower_lsp::lsp_types::{Position, Range};

#[derive(Debug, Clone)]
pub struct ExtractedKey {
    pub key: String,
    pub range: Range,
    pub start_offset: usize,
    pub end_offset: usize,
}

// 匹配 t('...', t("...", t(`...
static IN_T_CALL_LINE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(?:\$tc?|\btc?|\bi18n\.tc?)\s*\(\s*['"`][^'"`]*$"#).unwrap());

static FUNCTION_CALL_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?:\$tc?|\btc?|\bi18n\.tc?)\s*\(").unwrap());

static ATTRIBUTE_KEY_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"(?x)
        \b(?:keypath|i18nKey)\s*=\s*(?:"([^"]+)"|'([^']+)')
        | \bv-t\s*=\s*"'([^']+)'"
        | \bv-t\s*=\s*"\s*\{\s*path\s*:\s*(?:"([^"]+)"|'([^']+)')"
    "#,
    )
    .unwrap()
});

pub struct I18nParser;

impl I18nParser {
    pub fn extract_i18n_keys(text: &str) -> Vec<ExtractedKey> {
        let mut found = Vec::new();

        // 1. 函数调用提取：$t('key'), t("key")
        for m in FUNCTION_CALL_RE.find_iter(text) {
            let paren_offset = m.end() - 1;
            if let Some(call_text) = Self::read_parens(text, paren_offset) {
                let inner = &call_text[1..call_text.len() - 1];
                let trimmed = inner.trim_start();
                if let Some(quote) = trimmed.chars().next() {
                    if quote == '\'' || quote == '"' || quote == '`' {
                        let string_body = &trimmed[1..];
                        if let Some(end_quote) = string_body.find(quote) {
                            let key = &string_body[..end_quote];
                            if !key.is_empty() {
                                let key_start = paren_offset
                                    + (call_text.len() - inner.len())
                                    + (inner.len() - trimmed.len())
                                    + 1;
                                let key_end = key_start + key.len();
                                found.push(ExtractedKey {
                                    key: key.to_string(),
                                    range: Range {
                                        start: Self::offset_to_position(text, key_start),
                                        end: Self::offset_to_position(text, key_end),
                                    },
                                    start_offset: key_start,
                                    end_offset: key_end,
                                });
                            }
                        }
                    }
                }
            }
        }

        // 2. 指令与属性提取：v-t="'key'", keypath="key"
        for cap in ATTRIBUTE_KEY_RE.captures_iter(text) {
            let matched_key = cap.iter().skip(1).flatten().next();
            if let Some(m_key) = matched_key {
                let key = m_key.as_str();
                let start = m_key.start();
                let end = m_key.end();
                found.push(ExtractedKey {
                    key: key.to_string(),
                    range: Range {
                        start: Self::offset_to_position(text, start),
                        end: Self::offset_to_position(text, end),
                    },
                    start_offset: start,
                    end_offset: end,
                });
            }
        }

        found.sort_by_key(|k| k.start_offset);
        found
    }

    pub fn is_cursor_in_t_call(text: &str, offset: usize) -> bool {
        let safe_offset = Self::floor_char_boundary(text, offset);
        let before_cursor = &text[..safe_offset];

        // 获取光标之前的当前行内容
        let line_before = match before_cursor.lines().last() {
            Some(line) => line,
            None => return false,
        };

        // 正则判断光标前是否有未闭合的 t( 或 $t( 调用
        // static IN_T_CALL_LINE_RE: Lazy<Regex> = Lazy::new(|| {
        //     Regex::new(r#"(?:\$tc?|\btc?|\bi18n\.tc?)\s*\(\s*['"`][^'"`]*$"#).unwrap()
        // });

        IN_T_CALL_LINE_RE.is_match(line_before)
    }

    pub fn find_key_location(file_text: &str, key: &str) -> Option<Position> {
        let parts: Vec<&str> = key.split('.').collect();
        let mut cursor = 0;

        for (idx, part) in parts.iter().enumerate() {
            let re_pattern = format!(r#"(?:["']?){}(?:["']?)\s*:"#, regex::escape(part));
            let re = Regex::new(&re_pattern).ok()?;

            let target_text = file_text.get(cursor..)?;
            if let Some(mat) = re.find(target_text) {
                let match_pos = cursor + mat.start();
                cursor += mat.end();
                if idx == parts.len() - 1 {
                    return Some(Self::offset_to_position(file_text, match_pos));
                }
            } else {
                return None;
            }
        }
        None
    }

    pub fn read_parens(text: &str, start: usize) -> Option<String> {
        if text.as_bytes().get(start)? != &b'(' {
            return None;
        }
        let mut depth = 0;
        let mut in_quote = None;
        let mut escaped = false;

        for (i, ch) in text[start..].char_indices() {
            if let Some(q) = in_quote {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == q {
                    in_quote = None;
                }
                continue;
            }

            match ch {
                '"' | '\'' | '`' => in_quote = Some(ch),
                '(' | '{' | '[' => depth += 1,
                ')' | '}' | ']' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(text[start..=start + i].to_string());
                    }
                }
                _ => {}
            }
        }
        None
    }

    // 安全截断至 UTF-8 字符边界
    pub fn floor_char_boundary(text: &str, mut index: usize) -> usize {
        if index >= text.len() {
            return text.len();
        }
        while !text.is_char_boundary(index) {
            if index == 0 {
                break;
            }
            index -= 1;
        }
        index
    }

    // 准确计算 Line/Char -> Byte Offset
    pub fn position_to_offset(text: &str, pos: Position) -> usize {
        let mut line_start = 0usize;
        let mut current_line = 0u32;

        for line in text.split_inclusive('\n') {
            if current_line == pos.line {
                let mut char_count = 0u32;
                for (offset, _) in line.char_indices() {
                    if char_count == pos.character {
                        return line_start + offset;
                    }
                    char_count += 1;
                }
                let trim_len = if line.ends_with("\r\n") {
                    2
                } else if line.ends_with('\n') {
                    1
                } else {
                    0
                };
                return line_start + line.len().saturating_sub(trim_len);
            }
            line_start += line.len();
            current_line += 1;
        }

        text.len()
    }

    // 准确计算 Byte Offset -> Line/Char Position
    pub fn offset_to_position(text: &str, offset: usize) -> Position {
        let safe_offset = Self::floor_char_boundary(text, offset);
        let before = &text[..safe_offset];

        let mut line = 0u32;
        let mut last_line_start = 0;

        for (i, b) in before.bytes().enumerate() {
            if b == b'\n' {
                line += 1;
                last_line_start = i + 1;
            }
        }

        let line_str = &before[last_line_start..];
        let character = line_str.chars().count() as u32;

        Position { line, character }
    }
}
