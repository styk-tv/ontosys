//! # Consistent CLI Logging
//!
//! One-line-per-action logging with color coding.
//! Inspired by Terraform/RKE output style.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use crate::log::{Log, Action};
//!
//! let log = Log::new();
//! log.action(Action::Discover, "Finding source files");
//! log.done(Action::Discover, "Found 42 files");
//! log.step("Parsing", "src/lib.rs");
//! log.ok("Parsed 156 AST nodes");
//! log.warn("Skipped binary file");
//! log.fail("Could not parse malformed file");
//! ```

use console::{style, Style};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Action types for the pipeline
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Init,
    Discover,
    Parse,
    Build,
    Validate,
    Write,
    Export,
    Query,
    Serve,
    Watch,
    Clean,
}

impl Action {
    pub fn label(&self) -> &'static str {
        match self {
            Action::Init => "init",
            Action::Discover => "discover",
            Action::Parse => "parse",
            Action::Build => "build",
            Action::Validate => "validate",
            Action::Write => "write",
            Action::Export => "export",
            Action::Query => "query",
            Action::Serve => "serve",
            Action::Watch => "watch",
            Action::Clean => "clean",
        }
    }

    pub fn style(&self) -> Style {
        match self {
            Action::Init => Style::new().cyan().bold(),
            Action::Discover => Style::new().blue().bold(),
            Action::Parse => Style::new().magenta().bold(),
            Action::Build => Style::new().yellow().bold(),
            Action::Validate => Style::new().green().bold(),
            Action::Write => Style::new().cyan().bold(),
            Action::Export => Style::new().blue().bold(),
            Action::Query => Style::new().magenta().bold(),
            Action::Serve => Style::new().green().bold(),
            Action::Watch => Style::new().yellow().bold(),
            Action::Clean => Style::new().red().bold(),
        }
    }
}

/// Log level for messages
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Step,   // Normal step in process
    Ok,     // Success
    Warn,   // Warning
    Fail,   // Error
    Info,   // Informational
    Debug,  // Debug (only in verbose mode)
}

/// Consistent logger for CLI output
#[derive(Debug)]
pub struct Log {
    verbose: bool,
    start: Instant,
    step_count: AtomicUsize,
}

impl Default for Log {
    fn default() -> Self {
        Self::new()
    }
}

impl Log {
    pub fn new() -> Self {
        Self {
            verbose: false,
            start: Instant::now(),
            step_count: AtomicUsize::new(0),
        }
    }

    pub fn verbose(mut self) -> Self {
        self.verbose = true;
        self
    }

    /// Start an action: "ontosys discover: Finding source files..."
    pub fn action(&self, action: Action, msg: &str) {
        let prefix = action.style().apply_to(format!("ontosys {}", action.label()));
        println!("{}: {}...", prefix, msg);
    }

    /// Complete an action: "ontosys discover: Found 42 files"
    pub fn done(&self, action: Action, msg: &str) {
        let prefix = action.style().apply_to(format!("ontosys {}", action.label()));
        println!("{}: {}", prefix, msg);
    }

    /// Log a step within an action
    pub fn step(&self, action: Action, msg: &str) {
        self.step_count.fetch_add(1, Ordering::SeqCst);
        let prefix = action.style().apply_to(format!("  {}", action.label()));
        println!("{}: {}", prefix, style(msg).dim());
    }

    /// Success message with checkmark
    pub fn ok(&self, msg: &str) {
        println!("  {} {}", style("✓").green().bold(), msg);
    }

    /// Warning message
    pub fn warn(&self, msg: &str) {
        println!("  {} {}", style("⚠").yellow().bold(), msg);
    }

    /// Error/failure message
    pub fn fail(&self, msg: &str) {
        eprintln!("  {} {}", style("✗").red().bold(), msg);
    }

    /// Info message (dimmed)
    pub fn info(&self, msg: &str) {
        println!("  {} {}", style("→").dim(), msg);
    }

    /// Debug message (only in verbose mode)
    pub fn debug(&self, msg: &str) {
        if self.verbose {
            println!("  {} {}", style("·").dim(), style(msg).dim());
        }
    }

    /// Print a blank line
    pub fn blank(&self) {
        println!();
    }

    /// Print elapsed time
    pub fn elapsed(&self) {
        let elapsed = self.start.elapsed();
        println!(
            "\n{} in {:.2}s",
            style("Done").green().bold(),
            elapsed.as_secs_f64()
        );
    }

    /// Print a summary line
    pub fn summary(&self, msg: &str) {
        println!("\n{}", style(msg).bold());
    }

    /// Format a count for display
    pub fn count(&self, n: usize, singular: &str, plural: &str) -> String {
        if n == 1 {
            format!("{} {}", n, singular)
        } else {
            format!("{} {}", n, plural)
        }
    }

    /// Format bytes for display
    pub fn bytes(&self, n: usize) -> String {
        if n < 1024 {
            format!("{} B", n)
        } else if n < 1024 * 1024 {
            format!("{:.1} KB", n as f64 / 1024.0)
        } else {
            format!("{:.1} MB", n as f64 / (1024.0 * 1024.0))
        }
    }

    /// Format duration for display
    pub fn duration(&self, d: Duration) -> String {
        let secs = d.as_secs_f64();
        if secs < 1.0 {
            format!("{:.0}ms", secs * 1000.0)
        } else if secs < 60.0 {
            format!("{:.2}s", secs)
        } else {
            let mins = (secs / 60.0).floor() as u64;
            let remaining_secs = (secs % 60.0).floor() as u64;
            format!("{}m {}s", mins, remaining_secs)
        }
    }
}

/// Global log instance for convenience
pub fn log() -> Log {
    Log::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_labels() {
        assert_eq!(Action::Init.label(), "init");
        assert_eq!(Action::Discover.label(), "discover");
        assert_eq!(Action::Parse.label(), "parse");
        assert_eq!(Action::Build.label(), "build");
    }

    #[test]
    fn count_formatting() {
        let log = Log::new();
        assert_eq!(log.count(0, "file", "files"), "0 files");
        assert_eq!(log.count(1, "file", "files"), "1 file");
        assert_eq!(log.count(42, "file", "files"), "42 files");
    }

    #[test]
    fn bytes_formatting() {
        let log = Log::new();
        assert_eq!(log.bytes(500), "500 B");
        assert_eq!(log.bytes(1500), "1.5 KB");
        assert_eq!(log.bytes(1500000), "1.4 MB");
    }

    #[test]
    fn duration_formatting() {
        let log = Log::new();
        assert_eq!(log.duration(Duration::from_millis(50)), "50ms");
        assert_eq!(log.duration(Duration::from_secs_f64(1.5)), "1.50s");
        assert_eq!(log.duration(Duration::from_secs(90)), "1m 30s");
    }
}
