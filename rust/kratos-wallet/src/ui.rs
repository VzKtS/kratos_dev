// UI utilities for wallet CLI

use console::{style, Term};
use indicatif::{ProgressBar, ProgressStyle};
use std::io::{self, Write};

/// Create a spinner with a message
pub fn create_spinner(message: &str) -> ProgressBar {
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template("{spinner:.cyan} {msg}")
            .unwrap(),
    );
    spinner.set_message(message.to_string());
    spinner.enable_steady_tick(std::time::Duration::from_millis(80));
    spinner
}

/// Format balance for display
#[allow(dead_code)]
pub fn format_balance(amount: u128) -> String {
    const KRAT: u128 = 1_000_000_000_000;

    let whole = amount / KRAT;
    let frac = amount % KRAT;

    if frac == 0 {
        format!("{} KRAT", format_with_commas(whole))
    } else {
        let frac_str = format!("{:012}", frac);
        let trimmed = frac_str.trim_end_matches('0');
        let decimals = if trimmed.len() > 6 {
            &trimmed[..6]
        } else {
            trimmed
        };
        format!("{}.{} KRAT", format_with_commas(whole), decimals)
    }
}

/// Format number with commas for readability
fn format_with_commas(n: u128) -> String {
    let s = n.to_string();
    let chars: Vec<char> = s.chars().rev().collect();
    let mut result = String::new();

    for (i, c) in chars.iter().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(*c);
    }

    result.chars().rev().collect()
}

/// Print a horizontal line
#[allow(dead_code)]
pub fn print_line() {
    println!(
        "{}",
        style("─".repeat(50)).dim()
    );
}

/// Print a section header
#[allow(dead_code)]
pub fn print_header(title: &str) {
    println!();
    println!("{}", style(title).cyan().bold());
    println!();
}

/// Print success message
#[allow(dead_code)]
pub fn print_success(message: &str) {
    println!("{}", style(format!("  ✅ {}", message)).green());
}

/// Print error message
#[allow(dead_code)]
pub fn print_error(message: &str) {
    println!("{}", style(format!("  ❌ {}", message)).red());
}

/// Print warning message
#[allow(dead_code)]
pub fn print_warning(message: &str) {
    println!("{}", style(format!("  ⚠️  {}", message)).yellow());
}

/// Print info message
#[allow(dead_code)]
pub fn print_info(message: &str) {
    println!("{}", style(format!("  ℹ️  {}", message)).blue());
}

/// Read secret input with masked display (shows * for each character)
/// Returns the entered string
pub fn read_secret_with_mask(prompt: &str) -> String {
    let term = Term::stderr();
    let mut input = String::new();

    print!("{} ", style(prompt).cyan());
    let _ = io::stdout().flush();

    loop {
        match term.read_key() {
            Ok(console::Key::Enter) => {
                println!(); // New line after input
                break;
            }
            Ok(console::Key::Backspace) => {
                if !input.is_empty() {
                    input.pop();
                    // Move cursor back, print space, move back again
                    print!("\x08 \x08");
                    let _ = io::stdout().flush();
                }
            }
            Ok(console::Key::Char(c)) => {
                input.push(c);
                print!("{}", style("*").yellow());
                let _ = io::stdout().flush();
            }
            Ok(console::Key::Escape) => {
                // Cancel input
                println!();
                return String::new();
            }
            _ => {}
        }
    }

    input
}

/// Read password with confirmation (shows * for each character)
/// Returns the password if both entries match, or empty string on cancel/mismatch
pub fn read_password_with_confirm(prompt: &str, confirm_prompt: &str) -> Result<String, String> {
    let password = read_secret_with_mask(prompt);
    if password.is_empty() {
        return Err("Input cancelled".to_string());
    }

    let confirm = read_secret_with_mask(confirm_prompt);
    if confirm.is_empty() {
        return Err("Input cancelled".to_string());
    }

    if password != confirm {
        return Err("Passwords don't match".to_string());
    }

    Ok(password)
}

// =============================================================================
// TRANSACTION HISTORY UI HELPERS
// =============================================================================

use crate::types::{TransactionDirection, TransactionRecord, TransactionStatus};

/// Format a timestamp as a human-readable date/time
pub fn format_timestamp(timestamp: u64) -> String {
    use std::time::{Duration, UNIX_EPOCH};

    let datetime = UNIX_EPOCH + Duration::from_secs(timestamp);

    // Simple formatting without external crate
    let now = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let diff = now.saturating_sub(timestamp);

    if diff < 60 {
        "Just now".to_string()
    } else if diff < 3600 {
        format!("{} min ago", diff / 60)
    } else if diff < 86400 {
        format!("{} hours ago", diff / 3600)
    } else if diff < 604800 {
        format!("{} days ago", diff / 86400)
    } else {
        // Format as date
        let secs = datetime.duration_since(UNIX_EPOCH).unwrap().as_secs();
        let days_since_epoch = secs / 86400;
        let years = 1970 + (days_since_epoch / 365);
        let remaining_days = days_since_epoch % 365;
        let month = remaining_days / 30 + 1;
        let day = remaining_days % 30 + 1;
        format!("{:04}-{:02}-{:02}", years, month.min(12), day.min(31))
    }
}

/// Format an address for display (shortened)
pub fn format_address_short(address: &str) -> String {
    let addr = address.strip_prefix("0x").unwrap_or(address);
    if addr.len() > 16 {
        format!("0x{}...{}", &addr[..8], &addr[addr.len() - 8..])
    } else {
        format!("0x{}", addr)
    }
}

/// Format amount for transaction display
pub fn format_tx_amount(amount: u128, direction: TransactionDirection) -> String {
    const KRAT: u128 = 1_000_000_000_000;

    let whole = amount / KRAT;
    let frac = amount % KRAT;

    let amount_str = if frac == 0 {
        format!("{}", format_with_commas(whole))
    } else {
        let frac_str = format!("{:012}", frac);
        let trimmed = frac_str.trim_end_matches('0');
        let decimals = if trimmed.len() > 4 {
            &trimmed[..4]
        } else {
            trimmed
        };
        format!("{}.{}", format_with_commas(whole), decimals)
    };

    match direction {
        TransactionDirection::Sent => format!("{}", style(format!("-{} KRAT", amount_str)).red()),
        TransactionDirection::Received => {
            format!("{}", style(format!("+{} KRAT", amount_str)).green())
        }
    }
}

/// Print a single transaction record
pub fn print_transaction(tx: &TransactionRecord, index: usize) {
    let dir_icon = match tx.direction {
        TransactionDirection::Sent => style("").red(),
        TransactionDirection::Received => style("").green(),
    };

    let status_icon = match tx.status {
        TransactionStatus::Pending => style("").yellow(),
        TransactionStatus::Confirmed => style("").green(),
        TransactionStatus::Failed => style("").red(),
    };

    println!(
        "  {} {} {} {}",
        style(format!("{:>3}.", index + 1)).dim(),
        dir_icon,
        format_tx_amount(tx.amount, tx.direction),
        status_icon,
    );

    let counterparty_label = match tx.direction {
        TransactionDirection::Sent => "To:",
        TransactionDirection::Received => "From:",
    };

    println!(
        "      {} {}",
        style(counterparty_label).dim(),
        style(format_address_short(&tx.counterparty)).white()
    );

    println!(
        "      {} {}  {} {}",
        style("Time:").dim(),
        format_timestamp(tx.timestamp),
        style("Block:").dim(),
        tx.block_number
            .map(|b| b.to_string())
            .unwrap_or_else(|| "pending".to_string())
    );

    println!(
        "      {} {}",
        style("Hash:").dim(),
        style(format_address_short(&tx.hash)).cyan()
    );

    println!();
}

/// Print transaction history header
pub fn print_history_header(total: usize, showing: usize, page: usize, total_pages: usize) {
    println!(
        "  {} {} {} (page {}/{})",
        style("Showing").dim(),
        style(showing).white(),
        style(format!("of {} transactions", total)).dim(),
        page,
        total_pages.max(1)
    );
    println!();
}

/// Print empty history message
pub fn print_empty_history() {
    println!();
    println!(
        "  {}",
        style("No transactions found").dim()
    );
    println!();
    println!(
        "  {}",
        style("Send or receive KRAT to see your transaction history here.").dim()
    );
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_balance() {
        const KRAT: u128 = 1_000_000_000_000;

        assert_eq!(format_balance(0), "0 KRAT");
        assert_eq!(format_balance(KRAT), "1 KRAT");
        assert_eq!(format_balance(1000 * KRAT), "1,000 KRAT");
        assert_eq!(format_balance(1_000_000 * KRAT), "1,000,000 KRAT");
        assert_eq!(format_balance(KRAT + KRAT / 2), "1.5 KRAT");
    }

    #[test]
    fn test_format_with_commas() {
        assert_eq!(format_with_commas(0), "0");
        assert_eq!(format_with_commas(100), "100");
        assert_eq!(format_with_commas(1000), "1,000");
        assert_eq!(format_with_commas(1000000), "1,000,000");
        assert_eq!(format_with_commas(1234567890), "1,234,567,890");
    }

    #[test]
    fn test_format_address_short() {
        let addr = "0x0101010101010101010101010101010101010101010101010101010101010101";
        assert_eq!(format_address_short(addr), "0x01010101...01010101");
    }

    #[test]
    fn test_format_timestamp() {
        // Test "Just now"
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(format_timestamp(now), "Just now");
    }
}
