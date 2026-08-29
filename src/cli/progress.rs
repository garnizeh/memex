use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::time::Duration;

/// Reporter for tracking and rendering multi-stage indexing progress.
/// All output is strictly routed to `stderr` to preserve stdout purity for MCP JSON-RPC.
pub struct IndexProgressReporter {
    multi: Option<MultiProgress>,
    spinner: Option<ProgressBar>,
    embed_bar: Option<ProgressBar>,
    quiet: bool,
    interactive: bool,
}

impl IndexProgressReporter {
    /// Creates a new progress reporter. If `quiet` is true or if stderr is not a TTY,
    /// animated progress bars are automatically disabled.
    pub fn new(quiet: bool) -> Self {
        let is_tty = std::io::IsTerminal::is_terminal(&std::io::stderr());
        let interactive = !quiet && is_tty;

        let multi = if interactive {
            Some(MultiProgress::new())
        } else {
            None
        };

        Self {
            multi,
            spinner: None,
            embed_bar: None,
            quiet,
            interactive,
        }
    }

    /// Creates a silent reporter (for testing or quiet mode).
    pub fn silent() -> Self {
        Self {
            multi: None,
            spinner: None,
            embed_bar: None,
            quiet: true,
            interactive: false,
        }
    }

    /// Stage 1: Scanning repository for markdown files.
    pub fn start_scan(&mut self) {
        if self.quiet {
            return;
        }

        if self.interactive {
            if let Some(ref multi) = self.multi {
                let pb = multi.add(ProgressBar::new_spinner());
                pb.set_style(
                    ProgressStyle::default_spinner()
                        .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                        .template("{spinner:.cyan} [1/4] Scanning repository for markdown documentation...")
                        .unwrap_or_else(|_| ProgressStyle::default_spinner()),
                );
                pb.enable_steady_tick(Duration::from_millis(80));
                self.spinner = Some(pb);
            }
        } else {
            eprintln!("[1/4] Scanning repository for markdown documentation...");
        }
    }

    /// Completes scanning and displays total files found.
    pub fn finish_scan(&mut self, total_scanned: usize, to_process: usize) {
        if self.quiet {
            return;
        }

        if let Some(ref pb) = self.spinner.take() {
            pb.finish_and_clear();
        }

        if self.interactive {
            eprintln!(
                "✓ [1/4] Discovered {} documentation file(s) ({} changed)",
                total_scanned, to_process
            );
        }
    }

    /// Stage 2: Parsing AST & Chunking.
    pub fn start_parsing(&mut self, file_count: usize) {
        if self.quiet {
            return;
        }

        if self.interactive {
            if let Some(ref multi) = self.multi {
                let pb = multi.add(ProgressBar::new_spinner());
                pb.set_style(
                    ProgressStyle::default_spinner()
                        .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                        .template("{spinner:.cyan} [2/4] Parsing AST & building hierarchical chunks ({msg})...")
                        .unwrap_or_else(|_| ProgressStyle::default_spinner()),
                );
                pb.set_message(format!("{} files", file_count));
                pb.enable_steady_tick(Duration::from_millis(80));
                self.spinner = Some(pb);
            }
        } else {
            eprintln!(
                "[2/4] Parsing AST & building hierarchical chunks ({} files)...",
                file_count
            );
        }
    }

    /// Completes parsing and displays total chunks and hierarchy edges extracted.
    pub fn finish_parsing(&mut self, chunk_count: usize, edge_count: usize) {
        if self.quiet {
            return;
        }

        if let Some(ref pb) = self.spinner.take() {
            pb.finish_and_clear();
        }

        if self.interactive {
            eprintln!(
                "✓ [2/4] Parsed {} semantic chunk(s) & extracted {} graph relationship(s)",
                chunk_count, edge_count
            );
        }
    }

    /// Stage 3: Generating ONNX dense vector embeddings with real-time throughput and progress.
    pub fn start_embeddings(&mut self, total_chunks: usize) {
        if self.quiet || total_chunks == 0 {
            return;
        }

        if self.interactive {
            if let Some(ref multi) = self.multi {
                let pb = multi.add(ProgressBar::new(total_chunks as u64));
                pb.set_style(
                    ProgressStyle::default_bar()
                        .template(
                            "{spinner:.green} [3/4] Generating ONNX embeddings [{bar:30.cyan/blue}] {pos}/{len} chunks ({per_sec}, ETA: {eta})",
                        )
                        .unwrap_or_else(|_| ProgressStyle::default_bar())
                        .progress_chars("█▉▊▋▌▍▎▏  "),
                );
                self.embed_bar = Some(pb);
            }
        } else {
            eprintln!(
                "[3/4] Generating ONNX embeddings ({} chunks)...",
                total_chunks
            );
        }
    }

    /// Increments embedding progress count.
    pub fn step_embeddings(&self, count: usize) {
        if let Some(ref pb) = self.embed_bar {
            pb.inc(count as u64);
        }
    }

    /// Completes embedding generation.
    pub fn finish_embeddings(&mut self) {
        if self.quiet {
            return;
        }

        if let Some(ref pb) = self.embed_bar.take() {
            pb.finish_and_clear();
            if self.interactive {
                eprintln!("✓ [3/4] Generated 384-dimensional ONNX vector embeddings");
            }
        }
    }

    /// Stage 4: Writing to SQLite database.
    pub fn start_writing_db(&mut self) {
        if self.quiet {
            return;
        }

        if self.interactive {
            if let Some(ref multi) = self.multi {
                let pb = multi.add(ProgressBar::new_spinner());
                pb.set_style(
                    ProgressStyle::default_spinner()
                        .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                        .template(
                            "{spinner:.cyan} [4/4] Writing SQLite & sqlite-vec index atomically...",
                        )
                        .unwrap_or_else(|_| ProgressStyle::default_spinner()),
                );
                pb.enable_steady_tick(Duration::from_millis(80));
                self.spinner = Some(pb);
            }
        } else {
            eprintln!("[4/4] Writing SQLite & sqlite-vec index atomically...");
        }
    }

    /// Completes database writing.
    pub fn finish_writing_db(&mut self) {
        if self.quiet {
            return;
        }

        if let Some(ref pb) = self.spinner.take() {
            pb.finish_and_clear();
        }

        if self.interactive {
            eprintln!("✓ [4/4] Persisted document graph & vector indexes to SQLite");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_reporter_silent() {
        let mut reporter = IndexProgressReporter::silent();
        assert!(reporter.quiet);
        assert!(!reporter.interactive);

        // All lifecycle methods must execute without crashing in silent mode
        reporter.start_scan();
        reporter.finish_scan(10, 2);
        reporter.start_parsing(2);
        reporter.finish_parsing(15, 30);
        reporter.start_embeddings(15);
        reporter.step_embeddings(5);
        reporter.step_embeddings(10);
        reporter.finish_embeddings();
        reporter.start_writing_db();
        reporter.finish_writing_db();
    }

    #[test]
    fn test_progress_reporter_non_interactive() {
        let mut reporter = IndexProgressReporter {
            multi: None,
            spinner: None,
            embed_bar: None,
            quiet: false,
            interactive: false,
        };

        // All lifecycle methods must execute cleanly in non-interactive/CI mode
        reporter.start_scan();
        reporter.finish_scan(5, 1);
        reporter.start_parsing(1);
        reporter.finish_parsing(4, 8);
        reporter.start_embeddings(4);
        reporter.step_embeddings(4);
        reporter.finish_embeddings();
        reporter.start_writing_db();
        reporter.finish_writing_db();
    }

    #[test]
    fn test_progress_reporter_zero_chunks() {
        let mut reporter = IndexProgressReporter::new(false);
        reporter.start_embeddings(0);
        reporter.finish_embeddings();
    }
}
