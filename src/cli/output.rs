//! CLI Output Utilities
//!
//! Provides styled output, color management, table formatting, and other
//! visual enhancements for CLI commands.

use std::io::{self, IsTerminal, Write};

/// Terminal color support detection
static mut COLOR_ENABLED: Option<bool> = None;

/// Check if color output should be used
pub fn use_color() -> bool {
    unsafe {
        if let Some(enabled) = COLOR_ENABLED {
            return enabled;
        }

        // Check NO_COLOR environment variable (https://no-color.org/)
        if std::env::var("NO_COLOR").is_ok() {
            COLOR_ENABLED = Some(false);
            return false;
        }

        // Check CLICOLOR_FORCE for forcing color
        if std::env::var("CLICOLOR_FORCE").map(|v| v == "1").unwrap_or(false) {
            COLOR_ENABLED = Some(true);
            return true;
        }

        // Check if stdout is a TTY
        let enabled = io::stdout().is_terminal();
        COLOR_ENABLED = Some(enabled);
        enabled
    }
}

/// Force enable/disable color output
pub fn set_color_enabled(enabled: bool) {
    unsafe {
        COLOR_ENABLED = Some(enabled);
    }
}

/// Output style variants
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Style {
    /// Success - green
    Success,
    /// Error - red
    Error,
    /// Warning - yellow
    Warning,
    /// Info - blue
    Info,
    /// Header - bold/magenta
    Header,
    /// Dim - gray
    Dim,
    /// Bold - white bold
    Bold,
}

impl Style {
    /// Get ANSI color code
    fn ansi_code(&self) -> &'static str {
        match self {
            Style::Success => "\x1b[32m",      // Green
            Style::Error => "\x1b[31m",        // Red
            Style::Warning => "\x1b[33m",      // Yellow
            Style::Info => "\x1b[34m",         // Blue
            Style::Header => "\x1b[35;1m",     // Bold Magenta
            Style::Dim => "\x1b[90m",          // Bright black (gray)
            Style::Bold => "\x1b[1m",          // Bold
        }
    }

    fn reset_code() -> &'static str {
        "\x1b[0m"
    }
}

/// Apply styling to text
pub fn styled(text: impl AsRef<str>, style: Style) -> String {
    if use_color() {
        format!("{}{}{}", style.ansi_code(), text.as_ref(), Style::reset_code())
    } else {
        text.as_ref().to_string()
    }
}

/// Quick styling functions
pub fn success(text: impl AsRef<str>) -> String {
    styled(text, Style::Success)
}

pub fn error(text: impl AsRef<str>) -> String {
    styled(text, Style::Error)
}

pub fn warning(text: impl AsRef<str>) -> String {
    styled(text, Style::Warning)
}

pub fn info(text: impl AsRef<str>) -> String {
    styled(text, Style::Info)
}

pub fn header(text: impl AsRef<str>) -> String {
    styled(text, Style::Header)
}

pub fn dim(text: impl AsRef<str>) -> String {
    styled(text, Style::Dim)
}

pub fn bold(text: impl AsRef<str>) -> String {
    styled(text, Style::Bold)
}

/// Print a success message with checkmark
pub fn print_success(message: impl AsRef<str>) {
    println!("{} {}", success("✓"), message.as_ref());
}

/// Print an error message with X
pub fn print_error(message: impl AsRef<str>) {
    eprintln!("{} {}", error("✗"), message.as_ref());
}

/// Print a warning message with warning symbol
pub fn print_warning(message: impl AsRef<str>) {
    println!("{} {}", warning("⚠"), message.as_ref());
}

/// Print an info message with info symbol
pub fn print_info(message: impl AsRef<str>) {
    println!("{} {}", info("ℹ"), message.as_ref());
}

/// Print a table with headers and rows
pub fn print_table(headers: &[&str], rows: &[Vec<String>]) {
    if headers.is_empty() || rows.is_empty() {
        return;
    }

    // Calculate column widths
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(cell.len());
            }
        }
    }

    // Add padding
    widths = widths.iter().map(|w| w + 2).collect();

    // Print header
    let header_line: String = headers
        .iter()
        .enumerate()
        .map(|(i, h)| format!("{:width$}", h, width = widths[i]))
        .collect::<Vec<_>>()
        .join("| ")
        .trim_end()
        .to_string();
    
    println!("{}", header(header_line));

    // Print separator
    let separator: String = widths
        .iter()
        .map(|w| "-".repeat(*w))
        .collect::<Vec<_>>()
        .join("+-");
    println!("{}", dim(separator));

    // Print rows
    for row in rows {
        let line: String = row
            .iter()
            .enumerate()
            .map(|(i, cell)| {
                let width = if i < widths.len() { widths[i] } else { cell.len() + 2 };
                format!("{:width$}", cell, width = width)
            })
            .collect::<Vec<_>>()
            .join("| ")
            .trim_end()
            .to_string();
        println!("{}", line);
    }
}

/// Print a list with bullet points
pub fn print_list(items: &[impl AsRef<str>], bullet: &str) {
    for item in items {
        println!("{} {}", dim(bullet), item.as_ref());
    }
}

/// Print a bulleted list with checkmarks for true items
pub fn print_checklist(items: &[(bool, impl AsRef<str>)]) {
    for (done, item) in items {
        if *done {
            println!("{} {}", success("✓"), item.as_ref());
        } else {
            println!("{} {}", dim("○"), dim(item.as_ref()));
        }
    }
}

/// Print a section header
pub fn print_section(title: impl AsRef<str>) {
    println!();
    println!("{}", header(title.as_ref()));
    println!("{}", dim("─".repeat(title.as_ref().len())));
}

/// Print a key-value pair
pub fn print_kv(key: impl AsRef<str>, value: impl AsRef<str>) {
    println!("{}: {}", bold(key), value.as_ref());
}

/// Format bytes to human-readable size
pub fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    
    if bytes == 0 {
        return "0 B".to_string();
    }
    
    let exp = (bytes as f64).log(1024.0).min(UNITS.len() as f64 - 1.0) as usize;
    let size = bytes as f64 / 1024f64.powi(exp as i32);
    
    if exp == 0 {
        format!("{} {}", bytes, UNITS[exp])
    } else {
        format!("{:.1} {}", size, UNITS[exp])
    }
}

/// Format duration in human-readable form
pub fn format_duration(duration: std::time::Duration) -> String {
    let secs = duration.as_secs();
    let millis = duration.subsec_millis();
    
    if secs == 0 {
        if millis < 100 {
            format!("{}µs", duration.subsec_micros())
        } else {
            format!("{}ms", millis)
        }
    } else if secs < 60 {
        format!("{}.{:03}s", secs, millis)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m {}s", secs / 3600, (secs % 3600) / 60, secs % 60)
    }
}

/// Format a timestamp for display
pub fn format_timestamp(ts: &str) -> String {
    // Simple ISO8601 formatting - in production would parse and format properly
    if ts.len() > 19 {
        ts[..19].to_string()
    } else {
        ts.to_string()
    }
}

/// Indented text block
pub fn indented(text: impl AsRef<str>, spaces: usize) -> String {
    let indent = " ".repeat(spaces);
    text.as_ref()
        .lines()
        .map(|line| format!("{}{}", indent, line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Wrap text to a maximum width
pub fn wrap_text(text: impl AsRef<str>, width: usize) -> String {
    let text = text.as_ref();
    let mut result = String::new();
    let mut line_len = 0;
    
    for word in text.split_whitespace() {
        if line_len + word.len() + 1 > width && line_len > 0 {
            result.push('\n');
            line_len = 0;
        }
        
        if line_len > 0 {
            result.push(' ');
            line_len += 1;
        }
        
        result.push_str(word);
        line_len += word.len();
    }
    
    result
}

/// Confirmation prompt
pub fn confirm(prompt: impl AsRef<str>, default: bool) -> io::Result<bool> {
    let prompt_text = if default {
        format!("{} [Y/n] ", prompt.as_ref())
    } else {
        format!("{} [y/N] ", prompt.as_ref())
    };
    
    print!("{}", styled(&prompt_text, Style::Bold));
    io::stdout().flush()?;
    
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    
    let input = input.trim().to_lowercase();
    
    if input.is_empty() {
        Ok(default)
    } else {
        Ok(input == "y" || input == "yes")
    }
}

/// Print a box around text
pub fn print_boxed(lines: &[impl AsRef<str>], style: Style) {
    let max_len = lines.iter().map(|l| l.as_ref().len()).max().unwrap_or(0);
    let width = max_len + 4;
    
    let top = format!("┌{}┐", "─".repeat(width - 2));
    let bottom = format!("└{}┘", "─".repeat(width - 2));
    
    println!("{}", styled(top, style));
    for line in lines {
        let padded = format!("│ {:<width$} │", line.as_ref(), width = max_len);
        println!("{}", styled(padded, style));
    }
    println!("{}", styled(bottom, style));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_styled_output() {
        set_color_enabled(false); // Disable for testing
        
        assert_eq!(success("test"), "test");
        assert_eq!(error("test"), "test");
        assert_eq!(warning("test"), "test");
        assert_eq!(info("test"), "test");
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_size(1024 * 1024 * 1024), "1.0 GB");
    }

    #[test]
    fn test_format_duration() {
        use std::time::Duration;
        
        assert_eq!(format_duration(Duration::from_micros(500)), "500µs");
        assert_eq!(format_duration(Duration::from_millis(5)), "5000µs");
        assert_eq!(format_duration(Duration::from_millis(150)), "150ms");
        assert_eq!(format_duration(Duration::from_secs(5)), "5.000s");
        assert_eq!(format_duration(Duration::from_secs(90)), "1m 30s");
        assert_eq!(format_duration(Duration::from_secs(3661)), "1h 1m 1s");
    }

    #[test]
    fn test_wrap_text() {
        let text = "This is a long sentence that needs to be wrapped.";
        let wrapped = wrap_text(text, 20);
        assert!(wrapped.contains('\n'));
        assert!(wrapped.lines().all(|l| l.len() <= 20));
    }

    #[test]
    fn test_indented() {
        let text = "line1\nline2";
        let indented = indented(text, 4);
        assert!(indented.starts_with("    line1"));
        assert!(indented.contains("\n    line2"));
    }
}
