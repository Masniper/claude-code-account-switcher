// Enhanced UI/UX module for prettier, more informative output
// Provides colored output, icons, and rich formatting

use std::io::Write;

#[cfg(windows)]
pub fn enable_colors() {
    let _ = crate::windows_helpers::enable_ansi_colors();
}

#[cfg(not(windows))]
pub fn enable_colors() {
    // ANSI already supported on Unix-like systems
}

pub mod colors {
    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
    pub const DIM: &str = "\x1b[2m";
    pub const ITALIC: &str = "\x1b[3m";
    
    pub const GREEN: &str = "\x1b[32m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const BLUE: &str = "\x1b[34m";
    pub const MAGENTA: &str = "\x1b[35m";
    pub const CYAN: &str = "\x1b[36m";
    pub const GRAY: &str = "\x1b[90m";
    pub const WHITE: &str = "\x1b[97m";
    
    pub const BG_GREEN: &str = "\x1b[42m";
    pub const BG_YELLOW: &str = "\x1b[43m";
    pub const BG_BLUE: &str = "\x1b[44m";
}

pub mod icons {
    pub const CHECK: &str = "✓";
    pub const CROSS: &str = "✗";
    pub const STAR: &str = "★";
    pub const ARROW: &str = "→";
    pub const DOT: &str = "•";
    pub const WARN: &str = "⚠";
    pub const INFO: &str = "ℹ";
    pub const LINK: &str = "🔗";
    pub const CLOCK: &str = "🕐";
    pub const SPARKLE: &str = "✨";
}

/// Print a success message with green check mark
pub fn success(msg: &str) {
    println!("{}{}{} {}", colors::GREEN, icons::CHECK, colors::RESET, msg);
}

/// Print an error message with red cross
pub fn error(msg: &str) {
    eprintln!("{}{}{} {}", colors::YELLOW, icons::CROSS, colors::RESET, msg);
}

/// Print a warning with yellow icon
pub fn warn(msg: &str) {
    eprintln!("{}{}{} {}", colors::YELLOW, icons::WARN, colors::RESET, msg);
}

/// Print an info message with blue icon
pub fn info(msg: &str) {
    println!("{}{}{} {}", colors::CYAN, icons::INFO, colors::RESET, msg);
}

/// Print a section header (bold, with spacing)
pub fn header(msg: &str) {
    println!("\n{}{}{}\n", colors::BOLD, msg, colors::RESET);
}

/// Print a dimmed/secondary message
pub fn dim(msg: &str) {
    println!("{}{}{}", colors::DIM, msg, colors::RESET);
}

/// Print an empty state with helpful next steps
pub fn empty_state(title: &str, suggestion: &str, example: &str) {
    println!("\n{}{}{}", colors::DIM, title, colors::RESET);
    println!("  {} {}", icons::ARROW, suggestion);
    println!("  {}{}{}", colors::DIM, example, colors::RESET);
    println!();
}

/// Print an account list item with rich formatting
pub fn account_item(name: &str, email: Option<&str>, time_ago: Option<&str>, is_default: bool, has_drift: bool) {
    let marker = if is_default {
        format!("{}{}{}", colors::YELLOW, icons::STAR, colors::RESET)
    } else {
        "  ".to_string()
    };
    
    let drift_marker = if has_drift {
        format!(" {}*{}", colors::YELLOW, colors::RESET)
    } else {
        "".to_string()
    };
    
    let name_colored = format!("{}{}{}", colors::BOLD, name, colors::RESET);
    
    match (email, time_ago) {
        (Some(e), Some(t)) => println!("  {}  {}  {}{}  {}{}{}", 
            marker, name_colored, e, drift_marker,
            colors::DIM, t, colors::RESET),
        (Some(e), None) => println!("  {}  {}  {}{}", 
            marker, name_colored, e, drift_marker),
        (None, _) => println!("  {}  {}", marker, name_colored),
    }
}

/// Print a confirmation prompt with preview
pub fn confirm_prompt(question: &str, preview: &str) -> bool {
    println!("\n{}{}{}", colors::BOLD, question, colors::RESET);
    println!("{}{}{}", colors::DIM, preview, colors::RESET);
    print!("\n{}Confirm? [y/N]{} ", colors::YELLOW, colors::RESET);
    std::io::stdout().flush().ok();
    
    let mut reply = String::new();
    std::io::stdin().read_line(&mut reply).unwrap();
    let reply = reply.trim().to_lowercase();
    reply.starts_with('y') || reply.starts_with('д') // Russian 'yes' too
}

/// Print a before/after comparison
pub fn show_change(label: &str, before: &str, after: &str) {
    println!("\n{}{}{}", colors::BOLD, label, colors::RESET);
    println!("  {} {}  {}→{}  {}", 
        colors::DIM, before, colors::RESET,
        colors::GREEN, after);
}

/// Print a collapsible details section
pub fn details(items: &[(& str, &str)]) {
    for (key, value) in items {
        println!("  {}{:.<20}{} {}", colors::DIM, key, colors::RESET, value);
    }
}
