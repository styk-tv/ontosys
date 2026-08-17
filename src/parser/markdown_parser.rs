//! # Markdown Parser
//!
//! Extracts headings and content from Markdown files for the docs metadata layer.

use serde::Serialize;

/// A heading extracted from a Markdown file
#[derive(Debug, Clone, Serialize)]
pub struct MarkdownHeading {
    pub level: usize,
    pub text: String,
    pub line: usize,
}

/// Parsed content of a Markdown file
#[derive(Debug, Clone, Serialize)]
pub struct MarkdownFile {
    pub headings: Vec<MarkdownHeading>,
    pub content: String,
}

/// Parse a Markdown file, extracting headings and preserving full content.
///
/// Detects ATX-style headings (`# `, `## `, etc.) at the start of lines.
pub fn parse_markdown(source: &str) -> MarkdownFile {
    let mut headings = Vec::new();

    for (line_num, line) in source.lines().enumerate() {
        let trimmed = line.trim();

        // ATX-style headings: count leading '#' characters
        if trimmed.starts_with('#') {
            let level = trimmed.chars().take_while(|&c| c == '#').count();
            if level <= 6 {
                let text = trimmed[level..].trim().to_string();
                // Only count as heading if there's a space after the '#' chars (or text is empty)
                if text.is_empty() || trimmed.as_bytes().get(level) == Some(&b' ') {
                    // Remove optional trailing '#' characters
                    let text = text.trim_end_matches('#').trim().to_string();
                    headings.push(MarkdownHeading {
                        level,
                        text,
                        line: line_num + 1,
                    });
                }
            }
        }
    }

    MarkdownFile {
        headings,
        content: source.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_headings() {
        let source = "# Title\n\nSome text\n\n## Section\n\n### Subsection\n";
        let result = parse_markdown(source);

        assert_eq!(result.headings.len(), 3);

        assert_eq!(result.headings[0].level, 1);
        assert_eq!(result.headings[0].text, "Title");
        assert_eq!(result.headings[0].line, 1);

        assert_eq!(result.headings[1].level, 2);
        assert_eq!(result.headings[1].text, "Section");
        assert_eq!(result.headings[1].line, 5);

        assert_eq!(result.headings[2].level, 3);
        assert_eq!(result.headings[2].text, "Subsection");
        assert_eq!(result.headings[2].line, 7);
    }

    #[test]
    fn preserves_content() {
        let source = "# Hello\n\nWorld";
        let result = parse_markdown(source);
        assert_eq!(result.content, source);
    }

    #[test]
    fn heading_with_trailing_hashes() {
        let source = "## Section ##\n";
        let result = parse_markdown(source);
        assert_eq!(result.headings[0].text, "Section");
    }

    #[test]
    fn no_headings() {
        let source = "Just text\nMore text\n";
        let result = parse_markdown(source);
        assert!(result.headings.is_empty());
    }

    #[test]
    fn heading_without_space_is_not_heading() {
        let source = "#hashtag is not a heading\n";
        let result = parse_markdown(source);
        assert!(result.headings.is_empty());
    }
}
