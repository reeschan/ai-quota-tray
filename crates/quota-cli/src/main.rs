use std::io::{self, Read, Write};

use clap::{Parser, Subcommand};
use quota_core::model::PROVIDER_ORDER;
use quota_core::secrets::{is_allowed_secret, secret_label, SECRET_KEYS};
use quota_core::service::{fetch_enabled, format_summary, Runtime};
use quota_core::statusline;

#[derive(Parser)]
#[command(
    name = "quota",
    version,
    about = "AI サービスの残量と残高を、取得できた値だけ表示します"
)]
struct Cli {
    /// 有効なプロバイダのスナップショットを JSON で出す
    #[arg(long)]
    json: bool,
    /// この provider id だけ取得する
    #[arg(long)]
    provider: Option<String>,
    /// いずれかのスナップショットに error があれば終了コード 1
    #[arg(long)]
    strict: bool,
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// 設定ファイルの場所と内容を表示する（秘密情報は含まない）
    Config,
    /// Claude Code の statusLine ヘルパーをインストールする
    InstallStatusline,
    /// 資格情報マネージャーに秘密情報を保存する。値は標準入力から読む
    SetSecret { name: String },
    /// 資格情報マネージャーから秘密情報を消す
    DeleteSecret { name: String },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let code = match run(cli).await {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err}");
            2
        }
    };
    std::process::exit(code);
}

async fn run(cli: Cli) -> Result<i32, String> {
    let runtime = Runtime::open()?;
    if let Some(command) = cli.command {
        return match command {
            Cmd::Config => {
                println!("{}", runtime.paths.config_path.display());
                println!("{}", runtime.config.to_toml()?);
                Ok(0)
            }
            Cmd::InstallStatusline => {
                println!("{}", statusline::install(&runtime.paths)?);
                Ok(0)
            }
            Cmd::SetSecret { name } => {
                store_secret(&runtime, &name)?;
                Ok(0)
            }
            Cmd::DeleteSecret { name } => {
                delete_secret(&runtime, &name)?;
                Ok(0)
            }
        };
    }
    let mut snapshots = fetch_enabled(
        &runtime.config,
        &runtime.paths,
        runtime.http.clone(),
        runtime.secrets.clone(),
    )
    .await;
    if let Some(id) = &cli.provider {
        snapshots.retain(|snapshot| snapshot.provider == *id);
        if snapshots.is_empty() {
            return Err(format!(
                "不明なプロバイダです: {id}（{}）",
                PROVIDER_ORDER.join(", ")
            ));
        }
    }
    for snapshot in &snapshots {
        if let Err(err) = runtime.store.insert(snapshot) {
            eprintln!("history: {err}");
        }
    }
    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&snapshots).map_err(|err| err.to_string())?
        );
    } else {
        println!("{}", format_summary(&snapshots));
    }
    let failed = snapshots.iter().any(|snapshot| snapshot.error.is_some());
    Ok(if cli.strict && failed { 1 } else { 0 })
}

fn store_secret(runtime: &Runtime, name: &str) -> Result<(), String> {
    if !is_allowed_secret(name) {
        return Err(format!(
            "未知の項目です: {name}。使える名前: {}",
            SECRET_KEYS.join(", ")
        ));
    }
    let mut value = String::new();
    io::stdin()
        .read_to_string(&mut value)
        .map_err(|err| err.to_string())?;
    let value = value.trim();
    if value.is_empty() {
        return Err("空の値は保存しません".into());
    }
    runtime
        .secrets
        .set(name, value)
        .map_err(|err| err.to_string())?;
    let mut stdout = io::stdout().lock();
    writeln!(
        stdout,
        "{} を資格情報マネージャーに保存しました",
        secret_label(name)
    )
    .ok();
    Ok(())
}

fn delete_secret(runtime: &Runtime, name: &str) -> Result<(), String> {
    if !is_allowed_secret(name) {
        return Err(format!("未知の項目です: {name}"));
    }
    runtime
        .secrets
        .delete(name)
        .map_err(|err| err.to_string())?;
    println!("{} を削除しました", secret_label(name));
    Ok(())
}
