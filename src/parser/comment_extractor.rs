//! # Comment Extractor
//!
//! Extracts all comments from source files — doc comments, regular comments,
//! module-level docs, and TODO/FIXME markers — for the structured docs metadata layer.

use serde::Serialize;

/// The kind of comment extracted from source
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub enum CommentKind {
    /// Doc comments (`///`, docstrings, JSDoc)
    Doc,
    /// Module-level doc comments (`//!`)
    ModuleDoc,
    /// Regular comments (`//`, `#`, `/* */`)
    Regular,
    /// Comments containing TODO/FIXME/HACK/NOTE markers
    Todo,
}

/// A single extracted comment
#[derive(Debug, Clone, Serialize)]
pub struct ExtractedComment {
    pub kind: CommentKind,
    pub text: String,
    pub line: usize,
    /// The marker found (e.g. "TODO", "FIXME"), if any
    pub marker: Option<String>,
}

/// All comments extracted from a single file
#[derive(Debug, Clone, Serialize)]
pub struct FileComments {
    pub language: String,
    pub module_doc: Option<String>,
    pub comments: Vec<ExtractedComment>,
}

/// Check if text contains a TODO/FIXME/HACK/NOTE marker (case-insensitive).
/// Returns the marker string if found.
fn detect_marker(text: &str) -> Option<String> {
    let upper = text.to_uppercase();
    for marker in &["TODO", "FIXME", "HACK", "NOTE"] {
        if upper.contains(marker) {
            return Some(marker.to_string());
        }
    }
    None
}

/// Extract comments from Rust source code
pub fn extract_rust_comments(source: &str) -> FileComments {
    let mut comments = Vec::new();
    let mut module_doc_lines: Vec<String> = Vec::new();
    let mut in_block_comment = false;
    let mut block_comment_text = String::new();
    let mut block_comment_start_line = 0;

    for (line_num, line) in source.lines().enumerate() {
        let line_number = line_num + 1; // 1-indexed
        let trimmed = line.trim();

        // Handle block comments
        if in_block_comment {
            if let Some(end_pos) = trimmed.find("*/") {
                let before_end = &trimmed[..end_pos];
                if !before_end.is_empty() {
                    if !block_comment_text.is_empty() {
                        block_comment_text.push('\n');
                    }
                    block_comment_text.push_str(before_end.trim());
                }
                in_block_comment = false;

                let text = block_comment_text.clone();
                block_comment_text.clear();

                if let Some(marker) = detect_marker(&text) {
                    comments.push(ExtractedComment {
                        kind: CommentKind::Todo,
                        text,
                        line: block_comment_start_line,
                        marker: Some(marker),
                    });
                } else {
                    comments.push(ExtractedComment {
                        kind: CommentKind::Regular,
                        text,
                        line: block_comment_start_line,
                        marker: None,
                    });
                }
            } else {
                if !block_comment_text.is_empty() {
                    block_comment_text.push('\n');
                }
                block_comment_text.push_str(trimmed.trim_start_matches('*').trim());
            }
            continue;
        }

        // Check for block comment start
        if trimmed.starts_with("/*") {
            let after_start = trimmed[2..].trim();
            if let Some(end_pos) = after_start.find("*/") {
                // Single-line block comment
                let text = after_start[..end_pos].trim().to_string();
                if let Some(marker) = detect_marker(&text) {
                    comments.push(ExtractedComment {
                        kind: CommentKind::Todo,
                        text,
                        line: line_number,
                        marker: Some(marker),
                    });
                } else {
                    comments.push(ExtractedComment {
                        kind: CommentKind::Regular,
                        text,
                        line: line_number,
                        marker: None,
                    });
                }
            } else {
                // Multi-line block comment starts
                in_block_comment = true;
                block_comment_start_line = line_number;
                let rest = after_start.to_string();
                block_comment_text = rest;
            }
            continue;
        }

        // Module-level doc comments (`//!`)
        if trimmed.starts_with("//!") {
            let text = trimmed.trim_start_matches("//!").trim().to_string();
            module_doc_lines.push(text);
            continue;
        }

        // Doc comments (`///`)
        if trimmed.starts_with("///") {
            let text = trimmed.trim_start_matches("///").trim().to_string();
            if let Some(marker) = detect_marker(&text) {
                comments.push(ExtractedComment {
                    kind: CommentKind::Todo,
                    text,
                    line: line_number,
                    marker: Some(marker),
                });
            } else {
                comments.push(ExtractedComment {
                    kind: CommentKind::Doc,
                    text,
                    line: line_number,
                    marker: None,
                });
            }
            continue;
        }

        // Regular line comments (`//`)
        if trimmed.starts_with("//") {
            let text = trimmed.trim_start_matches("//").trim().to_string();
            if let Some(marker) = detect_marker(&text) {
                comments.push(ExtractedComment {
                    kind: CommentKind::Todo,
                    text,
                    line: line_number,
                    marker: Some(marker),
                });
            } else {
                comments.push(ExtractedComment {
                    kind: CommentKind::Regular,
                    text,
                    line: line_number,
                    marker: None,
                });
            }
            continue;
        }
    }

    let module_doc = if module_doc_lines.is_empty() {
        None
    } else {
        Some(module_doc_lines.join("\n"))
    };

    FileComments {
        language: "rust".to_string(),
        module_doc,
        comments,
    }
}

/// Extract comments from Python source code
pub fn extract_python_comments(source: &str) -> FileComments {
    let mut comments = Vec::new();

    for (line_num, line) in source.lines().enumerate() {
        let line_number = line_num + 1;
        let trimmed = line.trim();

        // Python line comments start with #
        if trimmed.starts_with('#') {
            let text = trimmed.trim_start_matches('#').trim().to_string();
            if let Some(marker) = detect_marker(&text) {
                comments.push(ExtractedComment {
                    kind: CommentKind::Todo,
                    text,
                    line: line_number,
                    marker: Some(marker),
                });
            } else {
                comments.push(ExtractedComment {
                    kind: CommentKind::Regular,
                    text,
                    line: line_number,
                    marker: None,
                });
            }
            continue;
        }

        // Check for inline comments (code followed by #)
        if let Some(hash_pos) = find_unquoted_hash(trimmed) {
            let text = trimmed[hash_pos + 1..].trim().to_string();
            if let Some(marker) = detect_marker(&text) {
                comments.push(ExtractedComment {
                    kind: CommentKind::Todo,
                    text,
                    line: line_number,
                    marker: Some(marker),
                });
            } else {
                comments.push(ExtractedComment {
                    kind: CommentKind::Regular,
                    text,
                    line: line_number,
                    marker: None,
                });
            }
        }
    }

    FileComments {
        language: "python".to_string(),
        module_doc: None,
        comments,
    }
}

/// Find the position of a `#` that's not inside a string literal
fn find_unquoted_hash(line: &str) -> Option<usize> {
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut prev_char = '\0';

    for (i, ch) in line.char_indices() {
        match ch {
            '\'' if !in_double_quote && prev_char != '\\' => in_single_quote = !in_single_quote,
            '"' if !in_single_quote && prev_char != '\\' => in_double_quote = !in_double_quote,
            '#' if !in_single_quote && !in_double_quote => {
                // Only return if there's code before the #
                if line[..i].trim().is_empty() {
                    return None; // This is a line-starting comment, handled separately
                }
                return Some(i);
            }
            _ => {}
        }
        prev_char = ch;
    }
    None
}

/// Extract comments from TypeScript/JavaScript source code
pub fn extract_typescript_comments(source: &str) -> FileComments {
    let mut comments = Vec::new();
    let mut in_block_comment = false;
    let mut block_comment_text = String::new();
    let mut block_comment_start_line = 0;
    let mut is_jsdoc = false;

    for (line_num, line) in source.lines().enumerate() {
        let line_number = line_num + 1;
        let trimmed = line.trim();

        // Handle block comments
        if in_block_comment {
            if let Some(end_pos) = trimmed.find("*/") {
                let before_end = &trimmed[..end_pos];
                if !before_end.is_empty() {
                    if !block_comment_text.is_empty() {
                        block_comment_text.push('\n');
                    }
                    block_comment_text.push_str(before_end.trim_start_matches('*').trim());
                }
                in_block_comment = false;

                let text = block_comment_text.clone();
                block_comment_text.clear();

                if let Some(marker) = detect_marker(&text) {
                    comments.push(ExtractedComment {
                        kind: CommentKind::Todo,
                        text,
                        line: block_comment_start_line,
                        marker: Some(marker),
                    });
                } else {
                    let kind = if is_jsdoc { CommentKind::Doc } else { CommentKind::Regular };
                    comments.push(ExtractedComment {
                        kind,
                        text,
                        line: block_comment_start_line,
                        marker: None,
                    });
                }
                is_jsdoc = false;
            } else {
                if !block_comment_text.is_empty() {
                    block_comment_text.push('\n');
                }
                block_comment_text.push_str(trimmed.trim_start_matches('*').trim());
            }
            continue;
        }

        // Check for block comment start (JSDoc: `/**`, regular: `/*`)
        if trimmed.starts_with("/**") && !trimmed.starts_with("/***") {
            is_jsdoc = true;
            let after_start = trimmed[3..].trim();
            if let Some(end_pos) = after_start.find("*/") {
                // Single-line JSDoc
                let text = after_start[..end_pos].trim().to_string();
                if let Some(marker) = detect_marker(&text) {
                    comments.push(ExtractedComment {
                        kind: CommentKind::Todo,
                        text,
                        line: line_number,
                        marker: Some(marker),
                    });
                } else {
                    comments.push(ExtractedComment {
                        kind: CommentKind::Doc,
                        text,
                        line: line_number,
                        marker: None,
                    });
                }
                is_jsdoc = false;
            } else {
                in_block_comment = true;
                block_comment_start_line = line_number;
                block_comment_text = after_start.to_string();
            }
            continue;
        } else if trimmed.starts_with("/*") {
            let after_start = trimmed[2..].trim();
            if let Some(end_pos) = after_start.find("*/") {
                let text = after_start[..end_pos].trim().to_string();
                if let Some(marker) = detect_marker(&text) {
                    comments.push(ExtractedComment {
                        kind: CommentKind::Todo,
                        text,
                        line: line_number,
                        marker: Some(marker),
                    });
                } else {
                    comments.push(ExtractedComment {
                        kind: CommentKind::Regular,
                        text,
                        line: line_number,
                        marker: None,
                    });
                }
            } else {
                in_block_comment = true;
                block_comment_start_line = line_number;
                block_comment_text = after_start.to_string();
            }
            continue;
        }

        // Line comments (`//`)
        if trimmed.starts_with("//") {
            let text = trimmed.trim_start_matches('/').trim().to_string();
            if let Some(marker) = detect_marker(&text) {
                comments.push(ExtractedComment {
                    kind: CommentKind::Todo,
                    text,
                    line: line_number,
                    marker: Some(marker),
                });
            } else {
                comments.push(ExtractedComment {
                    kind: CommentKind::Regular,
                    text,
                    line: line_number,
                    marker: None,
                });
            }
            continue;
        }
    }

    FileComments {
        language: "typescript".to_string(),
        module_doc: None,
        comments,
    }
}

/// Dispatch to the appropriate language comment extractor
pub fn extract_comments(source: &str, language: &str) -> FileComments {
    match language {
        "rust" | "rs" => extract_rust_comments(source),
        "python" | "py" => extract_python_comments(source),
        "typescript" | "ts" | "tsx" | "javascript" | "js" | "jsx" | "mjs" | "cjs" => {
            extract_typescript_comments(source)
        }
        _ => FileComments {
            language: language.to_string(),
            module_doc: None,
            comments: Vec::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_inline_comment_after_multibyte_text() {
        let source = "x = \"\u{1773}\u{1773}\" # TODO: wide text before the hash\n";
        let result = extract_python_comments(source);
        assert_eq!(result.comments.len(), 1);
        assert_eq!(result.comments[0].text, "TODO: wide text before the hash");
    }

    #[test]
    fn rust_doc_comments() {
        let source = "/// This is a doc comment\npub fn foo() {}";
        let result = extract_rust_comments(source);
        assert_eq!(result.comments.len(), 1);
        assert_eq!(result.comments[0].kind, CommentKind::Doc);
        assert_eq!(result.comments[0].text, "This is a doc comment");
        assert_eq!(result.comments[0].line, 1);
    }

    #[test]
    fn rust_module_doc() {
        let source = "//! Module docs\n//! More docs\nfn foo() {}";
        let result = extract_rust_comments(source);
        assert_eq!(result.module_doc, Some("Module docs\nMore docs".to_string()));
    }

    #[test]
    fn rust_regular_comment() {
        let source = "// Regular comment\nfn foo() {}";
        let result = extract_rust_comments(source);
        assert_eq!(result.comments.len(), 1);
        assert_eq!(result.comments[0].kind, CommentKind::Regular);
    }

    #[test]
    fn rust_todo_marker() {
        let source = "// TODO: fix this\nfn foo() {}";
        let result = extract_rust_comments(source);
        assert_eq!(result.comments.len(), 1);
        assert_eq!(result.comments[0].kind, CommentKind::Todo);
        assert_eq!(result.comments[0].marker, Some("TODO".to_string()));
    }

    #[test]
    fn rust_fixme_marker() {
        let source = "// FIXME: broken\nfn foo() {}";
        let result = extract_rust_comments(source);
        assert_eq!(result.comments[0].kind, CommentKind::Todo);
        assert_eq!(result.comments[0].marker, Some("FIXME".to_string()));
    }

    #[test]
    fn rust_block_comment() {
        let source = "/* block comment */\nfn foo() {}";
        let result = extract_rust_comments(source);
        assert_eq!(result.comments.len(), 1);
        assert_eq!(result.comments[0].kind, CommentKind::Regular);
        assert_eq!(result.comments[0].text, "block comment");
    }

    #[test]
    fn python_comments() {
        let source = "# This is a comment\ndef foo(): pass";
        let result = extract_python_comments(source);
        assert_eq!(result.comments.len(), 1);
        assert_eq!(result.comments[0].kind, CommentKind::Regular);
        assert_eq!(result.comments[0].text, "This is a comment");
    }

    #[test]
    fn python_todo() {
        let source = "# TODO: implement this\ndef foo(): pass";
        let result = extract_python_comments(source);
        assert_eq!(result.comments[0].kind, CommentKind::Todo);
        assert_eq!(result.comments[0].marker, Some("TODO".to_string()));
    }

    #[test]
    fn typescript_jsdoc() {
        let source = "/** This is JSDoc */\nfunction foo() {}";
        let result = extract_typescript_comments(source);
        assert_eq!(result.comments.len(), 1);
        assert_eq!(result.comments[0].kind, CommentKind::Doc);
    }

    #[test]
    fn typescript_line_comment() {
        let source = "// Regular comment\nfunction foo() {}";
        let result = extract_typescript_comments(source);
        assert_eq!(result.comments.len(), 1);
        assert_eq!(result.comments[0].kind, CommentKind::Regular);
    }

    #[test]
    fn dispatch_by_language() {
        let source = "/// doc\nfn foo() {}";
        let result = extract_comments(source, "rust");
        assert_eq!(result.language, "rust");
        assert_eq!(result.comments[0].kind, CommentKind::Doc);
    }

    #[test]
    fn case_insensitive_markers() {
        let source = "// todo: lowercase marker\nfn foo() {}";
        let result = extract_rust_comments(source);
        assert_eq!(result.comments[0].kind, CommentKind::Todo);
        assert_eq!(result.comments[0].marker, Some("TODO".to_string()));
    }
}
