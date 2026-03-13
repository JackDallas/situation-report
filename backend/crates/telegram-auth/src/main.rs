use std::io::{self, Write};
use std::sync::Arc;

use grammers_client::{Client, SenderPool};
use grammers_session::storages::SqliteSession;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let api_id: i32 = std::env::var("TELEGRAM_API_ID")
        .map_err(|_| anyhow::anyhow!("TELEGRAM_API_ID not set"))?
        .parse()
        .map_err(|e| anyhow::anyhow!("TELEGRAM_API_ID is not a valid integer: {e}"))?;

    let api_hash = std::env::var("TELEGRAM_API_HASH")
        .map_err(|_| anyhow::anyhow!("TELEGRAM_API_HASH not set"))?;

    let session_path = std::env::var("TELEGRAM_SESSION_PATH")
        .unwrap_or_else(|_| "data/telegram.session".to_string());

    // Ensure parent directory exists
    if let Some(parent) = std::path::Path::new(&session_path).parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }

    println!("Telegram Auth Tool");
    println!("==================");
    println!("API ID:       {api_id}");
    println!("Session path: {session_path}");
    println!();

    let session = Arc::new(SqliteSession::open(&session_path).await.map_err(|e| {
        anyhow::anyhow!("Failed to open session at {session_path}: {e}")
    })?);

    let pool = SenderPool::new(Arc::clone(&session), api_id);
    let SenderPool {
        runner,
        handle,
        updates: _,
    } = pool;

    let client = Client::new(handle.clone());
    let runner_task = tokio::spawn(runner.run());

    match client.is_authorized().await {
        Ok(true) => {
            println!("Already authorized! Session file is valid.");
            println!("You can start the server — Telegram source will connect automatically.");
            handle.quit();
            let _ = runner_task.await;
            return Ok(());
        }
        Ok(false) => {
            println!("Not yet authorized. Starting login flow...");
        }
        Err(e) => {
            anyhow::bail!("Failed to check auth status: {e}");
        }
    }

    // Step 1: Get phone number
    let phone = prompt("Enter your phone number (international format, e.g. +1234567890): ")?;
    let phone = phone.trim().to_string();

    println!("Requesting login code for {phone}...");
    let token = client
        .request_login_code(&phone, &api_hash)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to request login code: {e}"))?;

    println!("Login code sent! Check your Telegram app or SMS.");
    println!();

    // Step 2: Get code
    let code = prompt("Enter the login code: ")?;
    let code = code.trim().to_string();

    println!("Signing in...");
    match client.sign_in(&token, &code).await {
        Ok(user) => {
            println!(
                "Signed in as: {} (ID: {})",
                user.first_name().unwrap_or("unknown"),
                user.id()
            );
        }
        Err(grammers_client::SignInError::PasswordRequired(password_token)) => {
            println!("Two-factor authentication is enabled.");
            let password = prompt("Enter your 2FA password: ")?;
            let password = password.trim().to_string();

            client
                .check_password(password_token, password.as_bytes())
                .await
                .map_err(|e| anyhow::anyhow!("2FA check failed: {e}"))?;

            println!("2FA verified successfully!");
        }
        Err(e) => {
            handle.quit();
            let _ = runner_task.await;
            anyhow::bail!("Sign-in failed: {e}");
        }
    }

    println!();
    println!("Session saved to: {session_path}");
    println!("You can now start the server — Telegram source will use this session.");

    handle.quit();
    let _ = runner_task.await;

    Ok(())
}

fn prompt(msg: &str) -> io::Result<String> {
    print!("{msg}");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input)
}
