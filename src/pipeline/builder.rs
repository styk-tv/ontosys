//! # Fluent Pipeline Builder
//!
//! An alternative API using the builder pattern with type-state.

use super::*;
use std::path::PathBuf;

/// Builder for constructing pipelines with compile-time configuration checks
pub struct PipelineBuilder<Config> {
    repo_path: Option<PathBuf>,
    config: Config,
}

/// No configuration set yet
pub struct NoConfig;

/// Repository path configured
pub struct WithRepo {
    path: PathBuf,
}

/// Fully configured
pub struct Configured {
    path: PathBuf,
    options: PipelineOptions,
}

/// Pipeline configuration options
#[derive(Debug, Clone, Default)]
pub struct PipelineOptions {
    /// Include private items in the graph
    pub include_private: bool,
    /// Resolve cross-file references
    pub resolve_references: bool,
    /// Extract call graph
    pub extract_calls: bool,
    /// Include documentation comments
    pub include_docs: bool,
    /// Maximum file size to process (bytes)
    pub max_file_size: Option<usize>,
}

impl PipelineBuilder<NoConfig> {
    pub fn new() -> Self {
        Self {
            repo_path: None,
            config: NoConfig,
        }
    }

    /// Set the repository path
    pub fn repo(self, path: impl Into<PathBuf>) -> PipelineBuilder<WithRepo> {
        PipelineBuilder {
            repo_path: Some(path.into()),
            config: WithRepo {
                path: self.repo_path.unwrap_or_default(),
            },
        }
    }
}

impl Default for PipelineBuilder<NoConfig> {
    fn default() -> Self {
        Self::new()
    }
}

impl PipelineBuilder<WithRepo> {
    /// Configure pipeline options
    pub fn configure(self, options: PipelineOptions) -> PipelineBuilder<Configured> {
        PipelineBuilder {
            repo_path: self.repo_path,
            config: Configured {
                path: self.config.path,
                options,
            },
        }
    }

    /// Use default configuration
    pub fn with_defaults(self) -> PipelineBuilder<Configured> {
        self.configure(PipelineOptions::default())
    }
}

impl PipelineBuilder<Configured> {
    /// Build and return an initialized pipeline
    pub fn build(self) -> Result<Pipeline<Discovered>, PipelineError> {
        Pipeline::new().discover(self.repo_path.unwrap())
    }

    /// Build and run the full pipeline
    pub fn run(self) -> Result<Pipeline<Ready>, PipelineError> {
        self.build()?
            .parse()?
            .build()?
            .validate()
            .map(|p| p.ready())
    }
}

// ============================================================================
// CONVENIENCE CONSTRUCTORS
// ============================================================================

impl Pipeline<Uninitialized> {
    /// Create a pipeline builder
    pub fn builder() -> PipelineBuilder<NoConfig> {
        PipelineBuilder::new()
    }

    /// Quick pipeline from a path
    pub fn from_path(path: impl Into<PathBuf>) -> Result<Pipeline<Discovered>, PipelineError> {
        Pipeline::new().discover(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_pattern() {
        let _builder = Pipeline::builder()
            .repo("/some/path")
            .configure(PipelineOptions {
                include_private: true,
                resolve_references: true,
                extract_calls: true,
                include_docs: true,
                max_file_size: Some(1024 * 1024),
            });

        // Builder is now in Configured state and can call .build() or .run()
    }

    #[test]
    fn builder_with_defaults() {
        let _builder = Pipeline::builder().repo("/some/path").with_defaults();
    }
}
