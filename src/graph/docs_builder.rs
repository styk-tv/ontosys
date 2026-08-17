//! # Docs Builder
//!
//! Builds structured documentation metadata (`docs.json`) from extracted
//! comments and markdown files.

use std::collections::BTreeMap;
use serde::Serialize;
use crate::parser::comment_extractor::{CommentKind, FileComments};
use crate::parser::markdown_parser::MarkdownFile;
use crate::parser::AstNode;

/// Top-level docs metadata structure written to `docs.json`
#[derive(Debug, Clone, Serialize)]
pub struct DocsMetadata {
    pub version: String,
    pub generated: String,
    pub files: BTreeMap<String, FileDocsEntry>,
    pub markdown: BTreeMap<String, MarkdownEntry>,
    pub summary: DocsSummary,
}

/// Docs entry for a single source file
#[derive(Debug, Clone, Serialize)]
pub struct FileDocsEntry {
    pub language: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_doc: Option<String>,
    pub comments: Vec<CommentEntry>,
}

/// A single comment in the docs output
#[derive(Debug, Clone, Serialize)]
pub struct CommentEntry {
    pub kind: String,
    pub text: String,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_iri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
}

/// A markdown file entry in the docs output
#[derive(Debug, Clone, Serialize)]
pub struct MarkdownEntry {
    pub headings: Vec<MarkdownHeadingEntry>,
    pub content: String,
}

/// A heading within a markdown file
#[derive(Debug, Clone, Serialize)]
pub struct MarkdownHeadingEntry {
    pub level: usize,
    pub text: String,
    pub line: usize,
}

/// Summary statistics for the docs metadata
#[derive(Debug, Clone, Serialize)]
pub struct DocsSummary {
    pub total_doc_comments: usize,
    pub total_regular_comments: usize,
    pub total_todos: usize,
    pub total_fixmes: usize,
    pub markdown_files: usize,
}

/// Builder that accumulates docs metadata from multiple files
pub struct DocsBuilder {
    files: BTreeMap<String, FileDocsEntry>,
    markdown: BTreeMap<String, MarkdownEntry>,
}

impl DocsBuilder {
    pub fn new() -> Self {
        Self {
            files: BTreeMap::new(),
            markdown: BTreeMap::new(),
        }
    }

    /// Add a source file's comments with optional entity IRI cross-referencing
    pub fn add_source_file(
        &mut self,
        relative_path: &str,
        file_comments: FileComments,
        ast_nodes: &[AstNode],
        project_id: &str,
    ) {
        let file_iri_base = build_file_iri(project_id, relative_path);

        let comments: Vec<CommentEntry> = file_comments.comments.iter().map(|c| {
            let entity_iri = if c.kind == CommentKind::Doc {
                find_nearest_entity_iri(c.line, ast_nodes, &file_iri_base)
            } else {
                None
            };

            CommentEntry {
                kind: comment_kind_str(&c.kind),
                text: c.text.clone(),
                line: c.line,
                entity_iri,
                marker: c.marker.clone(),
            }
        }).collect();

        self.files.insert(relative_path.to_string(), FileDocsEntry {
            language: file_comments.language,
            module_doc: file_comments.module_doc,
            comments,
        });
    }

    /// Add a parsed markdown file
    pub fn add_markdown_file(&mut self, relative_path: &str, md: MarkdownFile) {
        let headings = md.headings.into_iter().map(|h| MarkdownHeadingEntry {
            level: h.level,
            text: h.text,
            line: h.line,
        }).collect();

        self.markdown.insert(relative_path.to_string(), MarkdownEntry {
            headings,
            content: md.content,
        });
    }

    /// Build the final metadata, computing summary statistics
    pub fn build(self) -> DocsMetadata {
        let mut total_doc = 0usize;
        let mut total_regular = 0usize;
        let mut total_todos = 0usize;
        let mut total_fixmes = 0usize;

        for entry in self.files.values() {
            for c in &entry.comments {
                match c.kind.as_str() {
                    "doc" => total_doc += 1,
                    "regular" => total_regular += 1,
                    "todo" => {
                        if let Some(marker) = &c.marker {
                            match marker.as_str() {
                                "FIXME" => total_fixmes += 1,
                                _ => total_todos += 1,
                            }
                        } else {
                            total_todos += 1;
                        }
                    }
                    _ => {}
                }
            }
        }

        let markdown_files = self.markdown.len();

        DocsMetadata {
            version: "1.0".to_string(),
            generated: chrono::Utc::now().to_rfc3339(),
            files: self.files,
            markdown: self.markdown,
            summary: DocsSummary {
                total_doc_comments: total_doc,
                total_regular_comments: total_regular,
                total_todos,
                total_fixmes,
                markdown_files,
            },
        }
    }
}

/// Convert CommentKind to the string used in the JSON output
fn comment_kind_str(kind: &CommentKind) -> String {
    match kind {
        CommentKind::Doc => "doc".to_string(),
        CommentKind::ModuleDoc => "module_doc".to_string(),
        CommentKind::Regular => "regular".to_string(),
        CommentKind::Todo => "todo".to_string(),
    }
}

/// Build a file IRI base string matching the pattern used by `entities::File::new()`
fn build_file_iri(project_id: &str, relative_path: &str) -> String {
    let safe_path = relative_path.replace('/', "_").replace('.', "_");
    format!("http://example.org/data/file/{}/{}", project_id, safe_path)
}

/// Find the nearest AST entity to a doc comment line and return its IRI.
///
/// Doc comments precede their entity, so we look for the closest entity
/// whose start_line is >= the comment line and within a small window.
fn find_nearest_entity_iri(
    comment_line: usize,
    ast_nodes: &[AstNode],
    file_iri_base: &str,
) -> Option<String> {
    let mut best: Option<(usize, String)> = None; // (distance, iri)

    for node in ast_nodes {
        if let Some(loc) = node.location() {
            // Doc comments appear before the entity, so entity start_line >= comment_line
            if loc.start_line >= comment_line {
                let distance = loc.start_line - comment_line;
                // Only match within a reasonable proximity (10 lines)
                if distance <= 10 {
                    let iri = build_entity_iri(file_iri_base, node);
                    match &best {
                        Some((best_dist, _)) if distance < *best_dist => {
                            best = Some((distance, iri));
                        }
                        None => {
                            best = Some((distance, iri));
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    best.map(|(_, iri)| iri)
}

/// Build an entity IRI from the file IRI base and the AST node, matching
/// the pattern used in `ontology/entities.rs`.
fn build_entity_iri(file_iri_base: &str, node: &AstNode) -> String {
    let segment = match node {
        AstNode::Module(n) => format!("module/{}", n.name),
        AstNode::Use(n) => {
            let safe = n.path.replace("::", "_").replace('*', "glob");
            format!("import/{}", safe)
        }
        AstNode::Struct(n) => format!("struct/{}", n.name),
        AstNode::Enum(n) => format!("enum/{}", n.name),
        AstNode::Trait(n) => format!("trait/{}", n.name),
        AstNode::Impl(_) => return format!("{}/impl", file_iri_base),
        AstNode::Function(n) => format!("fn/{}", n.name),
        AstNode::Const(n) => format!("const/{}", n.name),
        AstNode::Static(n) => format!("static/{}", n.name),
        AstNode::TypeAlias(n) => format!("type/{}", n.name),
        AstNode::Macro(n) => format!("macro/{}", n.name),
    };

    format!("{}/{}", file_iri_base, segment)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::*;
    use crate::parser::comment_extractor::*;
    use crate::parser::markdown_parser;

    #[test]
    fn build_empty_docs() {
        let builder = DocsBuilder::new();
        let docs = builder.build();
        assert_eq!(docs.version, "1.0");
        assert!(docs.files.is_empty());
        assert!(docs.markdown.is_empty());
        assert_eq!(docs.summary.total_doc_comments, 0);
    }

    #[test]
    fn add_source_file_with_comments() {
        let mut builder = DocsBuilder::new();
        let comments = FileComments {
            language: "rust".to_string(),
            module_doc: Some("Module docs".to_string()),
            comments: vec![
                ExtractedComment {
                    kind: CommentKind::Doc,
                    text: "A doc comment".to_string(),
                    line: 5,
                    marker: None,
                },
                ExtractedComment {
                    kind: CommentKind::Regular,
                    text: "A regular comment".to_string(),
                    line: 10,
                    marker: None,
                },
                ExtractedComment {
                    kind: CommentKind::Todo,
                    text: "fix this".to_string(),
                    line: 15,
                    marker: Some("TODO".to_string()),
                },
            ],
        };

        let nodes = vec![
            AstNode::Struct(StructNode {
                name: "Point".to_string(),
                visibility: Visibility::Public,
                generics: vec![],
                fields: vec![],
                is_tuple: false,
                is_unit: false,
                doc_comment: Some("A doc comment".to_string()),
                attributes: vec![],
                location: Some(SourceLocation::new("src/lib.rs", 6, 0, 10, 1)),
            }),
        ];

        builder.add_source_file("src/lib.rs", comments, &nodes, "test-project");

        let docs = builder.build();
        assert_eq!(docs.files.len(), 1);
        let entry = &docs.files["src/lib.rs"];
        assert_eq!(entry.module_doc, Some("Module docs".to_string()));
        assert_eq!(entry.comments.len(), 3);
        assert_eq!(entry.comments[0].kind, "doc");
        // The doc comment on line 5 should be matched to the struct on line 6
        assert!(entry.comments[0].entity_iri.is_some());
        assert!(entry.comments[0].entity_iri.as_ref().unwrap().contains("struct/Point"));

        assert_eq!(docs.summary.total_doc_comments, 1);
        assert_eq!(docs.summary.total_regular_comments, 1);
        assert_eq!(docs.summary.total_todos, 1);
    }

    #[test]
    fn add_markdown_file() {
        let mut builder = DocsBuilder::new();
        let md = markdown_parser::parse_markdown("# Title\n\nContent here\n");
        builder.add_markdown_file("README.md", md);

        let docs = builder.build();
        assert_eq!(docs.markdown.len(), 1);
        assert_eq!(docs.markdown["README.md"].headings[0].text, "Title");
        assert_eq!(docs.summary.markdown_files, 1);
    }

    #[test]
    fn fixme_counted_separately() {
        let mut builder = DocsBuilder::new();
        let comments = FileComments {
            language: "rust".to_string(),
            module_doc: None,
            comments: vec![
                ExtractedComment {
                    kind: CommentKind::Todo,
                    text: "fix this".to_string(),
                    line: 1,
                    marker: Some("FIXME".to_string()),
                },
                ExtractedComment {
                    kind: CommentKind::Todo,
                    text: "do this".to_string(),
                    line: 2,
                    marker: Some("TODO".to_string()),
                },
            ],
        };
        builder.add_source_file("src/main.rs", comments, &[], "proj");

        let docs = builder.build();
        assert_eq!(docs.summary.total_fixmes, 1);
        assert_eq!(docs.summary.total_todos, 1);
    }

    #[test]
    fn file_iri_construction() {
        let iri = build_file_iri("my-app", "src/lib.rs");
        assert_eq!(iri, "http://example.org/data/file/my-app/src_lib_rs");
    }

    #[test]
    fn entity_iri_construction() {
        let file_iri = "http://example.org/data/file/proj/src_lib_rs";
        let node = AstNode::Function(FunctionNode {
            name: "parse".to_string(),
            visibility: Visibility::Public,
            generics: vec![],
            parameters: vec![],
            return_type: None,
            is_async: false,
            is_const: false,
            is_unsafe: false,
            is_extern: false,
            abi: None,
            body_calls: vec![],
            doc_comment: None,
            attributes: vec![],
            location: None,
        });
        let iri = build_entity_iri(file_iri, &node);
        assert_eq!(iri, "http://example.org/data/file/proj/src_lib_rs/fn/parse");
    }
}
