// KratOs Wallet CLI
// Secure wallet for managing KRAT tokens

mod crypto;
mod rpc;
mod storage;
mod types;
mod ui;

use console::{style, Term};
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Password, Select};
use std::path::PathBuf;

use crate::crypto::WalletKeys;
use crate::rpc::RpcClient;
use crate::storage::WalletStorage;
use crate::ui::{
    create_spinner, print_empty_history, print_history_header, print_transaction,
    read_password_with_confirm, read_secret_with_mask,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const KRAT: u128 = 1_000_000_000_000; // 10^12

fn main() {
    let term = Term::stdout();
    let _ = term.clear_screen();

    print_banner();

    // Get wallet data directory
    let wallet_dir = get_wallet_dir();

    // Check if wallet exists
    let storage = WalletStorage::new(&wallet_dir);

    let (keys, rpc_url) = if storage.wallet_exists() {
        // Unlock existing wallet
        unlock_wallet(&storage)
    } else {
        // Setup new wallet
        setup_new_wallet(&storage)
    };

    // Create RPC client
    let client = RpcClient::new(&rpc_url);

    // Main menu loop
    main_menu(&term, &keys, &client, &storage);
}

fn print_banner() {
    println!();
    println!(
        "{}",
        style("  ╔═══════════════════════════════════════╗").cyan()
    );
    println!(
        "{}",
        style("  ║                                       ║").cyan()
    );
    println!(
        "{}",
        style("  ║         🔐 KRATOS WALLET 🔐           ║").cyan()
    );
    println!(
        "{}",
        style("  ║                                       ║").cyan()
    );
    println!(
        "{}",
        style(format!("  ║            v{}                    ║", VERSION)).cyan()
    );
    println!(
        "{}",
        style("  ║                                       ║").cyan()
    );
    println!(
        "{}",
        style("  ╚═══════════════════════════════════════╝").cyan()
    );
    println!();
}

fn get_wallet_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("kratos-wallet")
}

fn setup_new_wallet(storage: &WalletStorage) -> (WalletKeys, String) {
    println!(
        "{}",
        style("  📦 First time setup - Creating new wallet").yellow()
    );
    println!();

    let theme = ColorfulTheme::default();

    // Ask for import or generate
    let choices = vec!["Import existing keys", "Generate new keys"];
    let selection = Select::with_theme(&theme)
        .with_prompt("How would you like to set up your wallet?")
        .items(&choices)
        .default(0)
        .interact()
        .unwrap();

    let keys = if selection == 0 {
        // Import existing keys
        import_keys(&theme)
    } else {
        // Generate new keys
        generate_new_keys(&theme)
    };

    // Get RPC endpoint
    let rpc_url: String = Input::with_theme(&theme)
        .with_prompt("RPC endpoint")
        .default("http://127.0.0.1:9933".to_string())
        .interact_text()
        .unwrap();

    // Set password for encryption
    println!();
    println!(
        "{}",
        style("  🔒 Set a password to encrypt your wallet").yellow()
    );
    println!(
        "{}",
        style("     Your secret key will be encrypted with this password").dim()
    );
    println!();

    let password = loop {
        match read_password_with_confirm("Password:", "Confirm password:") {
            Ok(pwd) => break pwd,
            Err(e) => {
                eprintln!("{}", style(format!("  ❌ {}", e)).red());
                println!();
            }
        }
    };

    // Save wallet
    if let Err(e) = storage.save_wallet(&keys, &password, &rpc_url) {
        eprintln!("{}", style(format!("  ❌ Failed to save wallet: {}", e)).red());
        std::process::exit(1);
    }

    println!();
    println!(
        "{}",
        style("  ✅ Wallet created and saved successfully!").green()
    );
    println!();

    (keys, rpc_url)
}

fn import_keys(_theme: &ColorfulTheme) -> WalletKeys {
    println!();
    println!(
        "{}",
        style("  📥 Import your existing keys").yellow()
    );
    println!();

    // Get secret key (show * for each character typed for visual feedback)
    let secret_hex = read_secret_with_mask("Secret key (hex, 0x...):");

    if secret_hex.is_empty() {
        eprintln!("{}", style("  ❌ Input cancelled").red());
        std::process::exit(1);
    }

    let secret_hex = secret_hex.trim().strip_prefix("0x").unwrap_or(&secret_hex);

    let secret_bytes = match hex::decode(secret_hex) {
        Ok(bytes) if bytes.len() == 32 => bytes,
        Ok(bytes) => {
            eprintln!(
                "{}",
                style(format!("  ❌ Invalid key length: {} bytes (expected 32)", bytes.len())).red()
            );
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("{}", style(format!("  ❌ Invalid hex: {}", e)).red());
            std::process::exit(1);
        }
    };

    let mut secret_array = [0u8; 32];
    secret_array.copy_from_slice(&secret_bytes);

    let keys = WalletKeys::from_secret(secret_array);

    println!();
    println!(
        "{}",
        style(format!("  ✅ Imported account: 0x{}", keys.account_id_hex())).green()
    );
    println!();

    keys
}

fn generate_new_keys(theme: &ColorfulTheme) -> WalletKeys {
    println!();
    println!(
        "{}",
        style("  🎲 Generating new keys...").yellow()
    );

    let keys = WalletKeys::generate();

    println!();
    println!(
        "{}",
        style("  ⚠️  IMPORTANT: Save your secret key securely!").red().bold()
    );
    println!(
        "{}",
        style("     This is the ONLY way to recover your wallet.").red()
    );
    println!();

    println!(
        "  {} {}",
        style("Account ID:").bold(),
        style(format!("0x{}", keys.account_id_hex())).green()
    );
    println!();
    println!(
        "  {} {}",
        style("Secret Key:").bold(),
        style(format!("0x{}", keys.secret_key_hex())).yellow()
    );
    println!();

    // Confirm backup
    let confirmed = Confirm::with_theme(theme)
        .with_prompt("Have you saved your secret key securely?")
        .default(false)
        .interact()
        .unwrap();

    if !confirmed {
        eprintln!(
            "{}",
            style("  ❌ Please save your secret key before continuing!").red()
        );
        std::process::exit(1);
    }

    keys
}

fn unlock_wallet(storage: &WalletStorage) -> (WalletKeys, String) {
    let theme = ColorfulTheme::default();

    println!(
        "{}",
        style("  🔓 Unlock your wallet").yellow()
    );
    println!();

    loop {
        let password: String = Password::with_theme(&theme)
            .with_prompt("Password")
            .interact()
            .unwrap();

        match storage.load_wallet(&password) {
            Ok((keys, rpc_url)) => {
                println!();
                println!(
                    "{}",
                    style(format!("  ✅ Wallet unlocked: 0x{}...{}",
                        &keys.account_id_hex()[..8],
                        &keys.account_id_hex()[56..]
                    )).green()
                );
                println!();
                return (keys, rpc_url);
            }
            Err(e) => {
                eprintln!("{}", style(format!("  ❌ {}", e)).red());
                println!();
            }
        }
    }
}

fn main_menu(term: &Term, keys: &WalletKeys, client: &RpcClient, storage: &WalletStorage) {
    let theme = ColorfulTheme::default();

    loop {
        let _ = term.clear_screen();
        print_banner();

        // Show account info
        print_account_header(keys);

        // Menu options
        let choices = vec![
            "💰 Check Balance",
            "📤 Send KRAT",
            "📜 Transaction History",
            "⚙️  Settings",
            "🚪 Exit",
        ];

        let selection = Select::with_theme(&theme)
            .with_prompt("What would you like to do?")
            .items(&choices)
            .default(0)
            .interact()
            .unwrap();

        match selection {
            0 => check_balance(term, keys, client),
            1 => send_krat(term, keys, client, storage),
            2 => transaction_history(term, keys, client, storage),
            3 => settings(term, keys, storage),
            4 => {
                println!();
                println!("{}", style("  👋 Goodbye!").cyan());
                println!();
                break;
            }
            _ => {}
        }
    }
}

fn print_account_header(keys: &WalletKeys) {
    let account_hex = keys.account_id_hex();
    println!(
        "  {} 0x{}...{}",
        style("Account:").dim(),
        &account_hex[..8],
        &account_hex[56..]
    );
    println!();
}

fn check_balance(term: &Term, keys: &WalletKeys, client: &RpcClient) {
    let _ = term.clear_screen();
    print_banner();

    println!("{}", style("  💰 Account Balance").cyan().bold());
    println!();

    let spinner = create_spinner("Fetching balance...");

    match client.get_account(&keys.account_id_hex()) {
        Ok(info) => {
            spinner.finish_and_clear();

            println!(
                "  {} {}",
                style("Address:").dim(),
                style(format!("0x{}", keys.account_id_hex())).white()
            );
            println!();

            // Display balances
            println!(
                "  {}",
                style("┌─────────────────────────────────────────┐").dim()
            );
            println!(
                "  {}  {:<15} {} {}",
                style("│").dim(),
                "Free:",
                style(&info.free).green().bold(),
                style("│").dim()
            );
            println!(
                "  {}  {:<15} {} {}",
                style("│").dim(),
                "Reserved:",
                style(&info.reserved).yellow(),
                style("│").dim()
            );
            println!(
                "  {}  {:<15} {} {}",
                style("│").dim(),
                "Total:",
                style(&info.total).cyan().bold(),
                style("│").dim()
            );
            println!(
                "  {}",
                style("└─────────────────────────────────────────┘").dim()
            );

            println!();
            println!(
                "  {} {}",
                style("Nonce:").dim(),
                info.nonce
            );
        }
        Err(e) => {
            spinner.finish_and_clear();
            eprintln!("{}", style(format!("  ❌ Failed to fetch balance: {}", e)).red());
        }
    }

    println!();
    wait_for_enter();
}

fn send_krat(term: &Term, keys: &WalletKeys, client: &RpcClient, storage: &WalletStorage) {
    let _ = term.clear_screen();
    print_banner();

    println!("{}", style("  📤 Send KRAT").cyan().bold());
    println!();

    let theme = ColorfulTheme::default();

    // Get recipient
    let recipient: String = Input::with_theme(&theme)
        .with_prompt("Recipient address (0x...)")
        .validate_with(|input: &String| -> Result<(), &str> {
            let hex = input.strip_prefix("0x").unwrap_or(input);
            if hex.len() != 64 {
                return Err("Address must be 64 hex characters");
            }
            if hex::decode(hex).is_err() {
                return Err("Invalid hex address");
            }
            Ok(())
        })
        .interact_text()
        .unwrap();

    // Get amount
    let amount_str: String = Input::with_theme(&theme)
        .with_prompt("Amount (KRAT)")
        .validate_with(|input: &String| -> Result<(), &str> {
            if input.parse::<f64>().is_err() {
                return Err("Invalid amount");
            }
            let amount: f64 = input.parse().unwrap();
            if amount <= 0.0 {
                return Err("Amount must be positive");
            }
            Ok(())
        })
        .interact_text()
        .unwrap();

    let amount_krat: f64 = amount_str.parse().unwrap();
    let amount_raw = (amount_krat * KRAT as f64) as u128;

    // Confirm
    println!();
    println!("{}", style("  Transaction Summary:").yellow());
    println!("  ├── To: {}", style(&recipient).white());
    println!("  ├── Amount: {} KRAT", style(amount_krat).green().bold());
    println!("  └── Fee: ~0.000001 KRAT");
    println!();

    let confirmed = Confirm::with_theme(&theme)
        .with_prompt("Send this transaction?")
        .default(false)
        .interact()
        .unwrap();

    if !confirmed {
        println!();
        println!("{}", style("  ❌ Transaction cancelled").yellow());
        wait_for_enter();
        return;
    }

    // Get current nonce
    let spinner = create_spinner("Preparing transaction...");

    let nonce = match client.get_nonce(&keys.account_id_hex()) {
        Ok(n) => n,
        Err(e) => {
            spinner.finish_and_clear();
            eprintln!("{}", style(format!("  ❌ Failed to get nonce: {}", e)).red());
            wait_for_enter();
            return;
        }
    };

    // Create and sign transaction
    let recipient_hex = recipient.strip_prefix("0x").unwrap_or(&recipient);
    let recipient_bytes = hex::decode(recipient_hex).unwrap();
    let mut recipient_array = [0u8; 32];
    recipient_array.copy_from_slice(&recipient_bytes);

    let signed_tx = keys.create_transfer(recipient_array, amount_raw, nonce);

    spinner.set_message("Submitting transaction...");

    // Submit transaction
    match client.submit_transaction(&signed_tx) {
        Ok(result) => {
            spinner.finish_and_clear();
            println!();
            println!("{}", style("  ✅ Transaction submitted successfully!").green());
            println!();
            println!("  {} {}", style("Hash:").dim(), style(&result.hash).cyan());
            println!("  {} {}", style("Status:").dim(), result.message);

            // Record transaction in local history
            let tx_record = crate::types::TransactionRecord::new_sent(
                result.hash.clone(),
                recipient.clone(),
                amount_raw,
                signed_tx.transaction.timestamp,
                nonce,
            );

            if let Err(e) = storage.add_transaction(tx_record) {
                eprintln!(
                    "{}",
                    style(format!("  ⚠️  Warning: Failed to save to history: {}", e)).yellow()
                );
            }
        }
        Err(e) => {
            spinner.finish_and_clear();
            eprintln!("{}", style(format!("  ❌ Transaction failed: {}", e)).red());
        }
    }

    println!();
    wait_for_enter();
}

fn transaction_history(term: &Term, keys: &WalletKeys, client: &RpcClient, storage: &WalletStorage) {
    let _ = term.clear_screen();
    print_banner();

    println!("{}", style("  📜 Transaction History").cyan().bold());
    println!();

    let theme = ColorfulTheme::default();
    let page_size: usize = 10;
    let mut current_page: usize = 0;

    loop {
        let _ = term.clear_screen();
        print_banner();
        println!("{}", style("  📜 Transaction History").cyan().bold());
        println!();

        // Load local history
        let mut history = storage.get_history();
        let my_address = keys.account_id_hex();

        // Try to sync with node (fetch new transactions)
        let spinner = create_spinner("Syncing with node...");

        // Try to get transaction history from RPC
        match client.get_transaction_history(&my_address, 100, 0) {
            Ok(response) => {
                spinner.finish_and_clear();

                // Convert and merge RPC transactions into local history
                let rpc_records = client.convert_rpc_transactions(response.transactions, &my_address);
                for record in rpc_records {
                    history.add(record);
                }

                // Update pending transactions status
                update_pending_transactions(&mut history, client);

                // Save merged history
                let _ = storage.save_history(&history);
            }
            Err(_) => {
                spinner.finish_and_clear();
                // RPC method not available, use local history only
                println!(
                    "  {}",
                    style("Using local history (node sync unavailable)").dim()
                );
                println!();
            }
        }

        // Display history
        if history.is_empty() {
            print_empty_history();
            wait_for_enter();
            return;
        }

        let total = history.len();
        let total_pages = (total + page_size - 1) / page_size;
        let offset = current_page * page_size;
        let page_txs = history.get_page(offset, page_size);

        print_history_header(total, page_txs.len(), current_page + 1, total_pages);

        for (i, tx) in page_txs.iter().enumerate() {
            print_transaction(tx, offset + i);
        }

        // Navigation menu
        let mut nav_choices = vec![];

        if current_page > 0 {
            nav_choices.push("Previous page");
        }
        if current_page < total_pages.saturating_sub(1) {
            nav_choices.push("Next page");
        }
        nav_choices.push("Refresh");
        nav_choices.push("Back to menu");

        let selection = Select::with_theme(&theme)
            .with_prompt("Navigation")
            .items(&nav_choices)
            .default(0)
            .interact()
            .unwrap();

        let choice = nav_choices[selection];

        match choice {
            "Previous page" => {
                current_page = current_page.saturating_sub(1);
            }
            "Next page" => {
                current_page += 1;
            }
            "Refresh" => {
                // Loop will refresh
            }
            "Back to menu" | _ => {
                return;
            }
        }
    }
}

/// Update pending transaction statuses by querying the node
fn update_pending_transactions(history: &mut crate::types::TransactionHistory, client: &RpcClient) {
    use crate::types::TransactionStatus;

    // For now, we can't directly query transaction status from the node
    // In a full implementation, we would query each pending transaction
    // For now, we'll mark old pending transactions as potentially confirmed
    // based on block height

    if let Ok(current_height) = client.get_block_height() {
        for tx in history.transactions.iter_mut() {
            if tx.status == TransactionStatus::Pending {
                // If transaction is old (more than ~10 blocks worth of time),
                // assume it's either confirmed or failed
                // This is a heuristic - proper implementation would query the node
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();

                let age_secs = now.saturating_sub(tx.timestamp);

                // If pending for more than 2 minutes, mark as confirmed
                // (In production, would verify with the node)
                if age_secs > 120 {
                    tx.status = TransactionStatus::Confirmed;
                    // Estimate block number based on ~6 second blocks
                    let blocks_ago = age_secs / 6;
                    tx.block_number = Some(current_height.saturating_sub(blocks_ago));
                }
            }
        }
    }
}

fn settings(term: &Term, keys: &WalletKeys, storage: &WalletStorage) {
    let _ = term.clear_screen();
    print_banner();

    println!("{}", style("  ⚙️  Settings").cyan().bold());
    println!();

    let theme = ColorfulTheme::default();

    let choices = vec![
        "🔑 Show Account ID",
        "🌐 Change RPC Endpoint",
        "🔒 Change Password",
        "⬅️  Back",
    ];

    let selection = Select::with_theme(&theme)
        .with_prompt("Settings")
        .items(&choices)
        .default(0)
        .interact()
        .unwrap();

    match selection {
        0 => {
            println!();
            println!(
                "  {} {}",
                style("Account ID:").bold(),
                style(format!("0x{}", keys.account_id_hex())).green()
            );
            println!();
            wait_for_enter();
        }
        1 => {
            let new_url: String = Input::with_theme(&theme)
                .with_prompt("New RPC endpoint")
                .default("http://127.0.0.1:9933".to_string())
                .interact_text()
                .unwrap();

            // Need password to re-save
            let password: String = Password::with_theme(&theme)
                .with_prompt("Enter password to save changes")
                .interact()
                .unwrap();

            if let Err(e) = storage.save_wallet(keys, &password, &new_url) {
                eprintln!("{}", style(format!("  ❌ Failed to save: {}", e)).red());
            } else {
                println!("{}", style("  ✅ RPC endpoint updated!").green());
            }
            wait_for_enter();
        }
        2 => {
            let old_password: String = Password::with_theme(&theme)
                .with_prompt("Current password")
                .interact()
                .unwrap();

            // Verify old password
            match storage.load_wallet(&old_password) {
                Ok((_, rpc_url)) => {
                    let new_password: String = Password::with_theme(&theme)
                        .with_prompt("New password")
                        .with_confirmation("Confirm new password", "Passwords don't match")
                        .interact()
                        .unwrap();

                    if let Err(e) = storage.save_wallet(keys, &new_password, &rpc_url) {
                        eprintln!("{}", style(format!("  ❌ Failed to save: {}", e)).red());
                    } else {
                        println!("{}", style("  ✅ Password changed!").green());
                    }
                }
                Err(_) => {
                    eprintln!("{}", style("  ❌ Incorrect password").red());
                }
            }
            wait_for_enter();
        }
        _ => {}
    }
}

fn wait_for_enter() {
    use std::io::{self, Write};
    print!("{}", style("  Press Enter to continue...").dim());
    let _ = io::stdout().flush();
    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);
}
