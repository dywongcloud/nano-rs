//! CLI Progress Indicators
//!
//! Provides visual progress bars and spinners for long-running CLI operations.
//! Automatically disables output for fast operations (<100ms).

use crate::cli::output;
use std::io::{self, IsTerminal, Write};
use std::time::{Duration, Instant};

/// Minimum duration before showing progress (100ms)
const PROGRESS_THRESHOLD_MS: u128 = 100;

/// Checkmark character for success indication
const CHECK: &str = "✓";

/// Progress bar for CLI operations
pub struct ProgressBar {
    total: u64,
    current: u64,
    message: String,
    start_time: Instant,
    should_show: bool,
    width: usize,
}

impl ProgressBar {
    /// Create a new progress bar
    pub fn new(total: u64, message: impl Into<String>) -> Self {
        Self {
            total,
            current: 0,
            message: message.into(),
            start_time: Instant::now(),
            should_show: false,
            width: 40,
        }
    }

    /// Create a progress bar that shows immediately (for known long operations)
    pub fn new_immediate(total: u64, message: impl Into<String>) -> Self {
        let mut bar = Self::new(total, message);
        bar.should_show = true;
        bar
    }

    /// Increment progress by n
    pub fn inc(&mut self, n: u64) {
        self.current = (self.current + n).min(self.total);
        self.check_threshold();
        if self.should_show {
            self.render();
        }
    }

    /// Set current progress value
    pub fn set_position(&mut self, pos: u64) {
        self.current = pos.min(self.total);
        self.check_threshold();
        if self.should_show {
            self.render();
        }
    }

    /// Finish with a message
    pub fn finish(&mut self, message: impl AsRef<str>) {
        self.current = self.total;
        if self.should_show {
            let _ = self.render_final(message.as_ref());
        }
    }

    /// Finish with success message
    pub fn finish_success(&mut self) {
        let elapsed = self.start_time.elapsed();
        let msg = format!("Done in {:.1}s", elapsed.as_secs_f64());
        self.finish(&msg);
    }

    /// Check if we've passed the threshold to show progress
    fn check_threshold(&mut self) {
        if !self.should_show {
            let elapsed = self.start_time.elapsed().as_millis();
            if elapsed > PROGRESS_THRESHOLD_MS {
                self.should_show = true;
            }
        }
    }

    /// Render the progress bar
    fn render(&self) {
        if !is_tty() || std::env::var("NO_PROGRESS").is_ok() {
            return;
        }

        let percentage = if self.total > 0 {
            (self.current as f64 / self.total as f64 * 100.0) as u64
        } else {
            0
        };

        let filled = if self.total > 0 {
            (self.current as usize * self.width) / self.total as usize
        } else {
            0
        };

        let bar: String = std::iter::repeat('█')
            .take(filled)
            .chain(std::iter::repeat('░').take(self.width - filled))
            .collect();

        let elapsed = self.start_time.elapsed();
        let eta = if self.current > 0 && self.total > 0 {
            let rate = elapsed.as_millis() as f64 / self.current as f64;
            let remaining = (self.total - self.current) as f64 * rate;
            format_eta(Duration::from_millis(remaining as u64))
        } else {
            "--:--".to_string()
        };

        let line = format!(
            "\r{} [{}] {:>3}% ({}/{}) ETA: {}",
            self.message, bar, percentage, self.current, self.total, eta
        );

        print!("{:<80}", line);
        let _ = io::stdout().flush();
    }

    /// Render final state
    fn render_final(&self, message: &str) -> io::Result<()> {
        if !is_tty() {
            println!("{}", message);
            return Ok(());
        }

            let check = if output::use_color() {
                CHECK.green()
            } else {
                CHECK.to_string()
            };

        println!("\r{} {} {:<60}", check, message, "");
        io::stdout().flush()
    }
}

impl Drop for ProgressBar {
    fn drop(&mut self) {
        if self.should_show && self.current < self.total {
            // Ensure we clean up the line
            println!();
        }
    }
}

/// Simple spinner for indeterminate progress
pub struct Spinner {
    message: String,
    start_time: Instant,
    should_show: bool,
    frames: Vec<&'static str>,
    frame_idx: usize,
}

impl Spinner {
    /// Create a new spinner
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            start_time: Instant::now(),
            should_show: false,
            frames: vec!["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
            frame_idx: 0,
        }
    }

    /// Tick the spinner animation
    pub fn tick(&mut self) {
        self.check_threshold();
        if self.should_show {
            self.frame_idx = (self.frame_idx + 1) % self.frames.len();
            self.render();
        }
    }

    /// Finish with a message
    pub fn finish(&mut self, message: impl AsRef<str>) {
        if self.should_show {
        let check = if output::use_color() {
                "✓".green()
            } else {
                "✓".to_string()
            };
            println!("\r{} {} {:<60}", check, message.as_ref(), "");
        }
    }

    /// Finish with error message
    pub fn finish_error(&mut self, message: impl AsRef<str>) {
        if self.should_show {
            let x = if output::use_color() {
                "✗".red()
            } else {
                "✗".to_string()
            };
            println!("\r{} {} {:<60}", x, message.as_ref(), "");
        }
    }

    fn check_threshold(&mut self) {
        if !self.should_show {
            let elapsed = self.start_time.elapsed().as_millis();
            if elapsed > PROGRESS_THRESHOLD_MS {
                self.should_show = true;
            }
        }
    }

    fn render(&self) {
        if !is_tty() || std::env::var("NO_PROGRESS").is_ok() {
            return;
        }

        let frame = self.frames[self.frame_idx];
        let elapsed = format_duration(self.start_time.elapsed());

        print!("\r{} {} ({}) {:<50}", frame, self.message, elapsed, "");
        let _ = io::stdout().flush();
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        if self.should_show {
            println!();
        }
    }
}

/// Execute a function with progress reporting
pub fn with_progress<F, T>(total: u64, message: impl Into<String>, mut f: F) -> T
where
    F: FnMut(&mut dyn FnMut(u64)) -> T,
{
    let mut bar = ProgressBar::new(total, message);
    let result = f(&mut |n| bar.inc(n));
    bar.finish_success();
    result
}

/// Execute a function with a spinner for indeterminate progress
pub fn with_spinner<F, T>(message: impl Into<String>, f: F) -> T
where
    F: FnOnce() -> T,
{
    let mut spinner = Spinner::new(message);
    let result = f();
    spinner.finish("Done");
    result
}

/// Check if stdout is a TTY
fn is_tty() -> bool {
    io::stdout().is_terminal()
}

/// Format duration for display
fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

/// Format ETA duration
fn format_eta(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{:02}s", secs)
    } else if secs < 3600 {
        format!("{:02}m{:02}s", secs / 60, secs % 60)
    } else {
        format!("{:02}h{:02}m", secs / 3600, (secs % 3600) / 60)
    }
}

// Color helpers (inline to avoid circular dependency)
trait ColorExt {
    fn green(&self) -> String;
    fn red(&self) -> String;
}

impl ColorExt for str {
    fn green(&self) -> String {
        if output::use_color() {
            format!("\x1b[32m{}\x1b[0m", self)
        } else {
            self.to_string()
        }
    }

    fn red(&self) -> String {
        if output::use_color() {
            format!("\x1b[31m{}\x1b[0m", self)
        } else {
            self.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_bar_creation() {
        let bar = ProgressBar::new(100, "Testing");
        assert_eq!(bar.total, 100);
        assert_eq!(bar.current, 0);
        assert!(!bar.should_show); // Not shown immediately
    }

    #[test]
    fn test_progress_bar_immediate() {
        let bar = ProgressBar::new_immediate(100, "Testing");
        assert!(bar.should_show); // Shown immediately
    }

    #[test]
    fn test_progress_bar_increment() {
        let mut bar = ProgressBar::new_immediate(100, "Testing");
        bar.inc(25);
        assert_eq!(bar.current, 25);
        bar.inc(25);
        assert_eq!(bar.current, 50);
    }

    #[test]
    fn test_progress_bar_set_position() {
        let mut bar = ProgressBar::new_immediate(100, "Testing");
        bar.set_position(75);
        assert_eq!(bar.current, 75);
        bar.set_position(150); // Should be clamped to total
        assert_eq!(bar.current, 100);
    }

    #[test]
    fn test_progress_bar_clamps_to_total() {
        let mut bar = ProgressBar::new_immediate(100, "Testing");
        bar.inc(150);
        assert_eq!(bar.current, 100);
    }

    #[test]
    fn test_spinner_creation() {
        let spinner = Spinner::new("Loading...");
        assert!(!spinner.should_show);
        assert_eq!(spinner.frames.len(), 10);
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(Duration::from_secs(45)), "45s");
        assert_eq!(format_duration(Duration::from_secs(90)), "1m 30s");
        assert_eq!(format_duration(Duration::from_secs(3661)), "1h 1m");
    }

    #[test]
    fn test_format_eta() {
        assert_eq!(format_eta(Duration::from_secs(45)), "45s");
        assert_eq!(format_eta(Duration::from_secs(90)), "01m30s");
        assert_eq!(format_eta(Duration::from_secs(3661)), "01h01m");
    }

    #[test]
    fn test_with_progress() {
        let result = with_progress(10, "Test", |progress| {
            for i in 1..=10 {
                progress(1);
                std::thread::sleep(Duration::from_millis(1));
            }
            42
        });
        assert_eq!(result, 42);
    }

    #[test]
    fn test_with_spinner() {
        let result = with_spinner("Loading...", || {
            std::thread::sleep(Duration::from_millis(10));
            "done"
        });
        assert_eq!(result, "done");
    }
}
