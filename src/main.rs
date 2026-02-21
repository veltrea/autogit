use clap::{Parser, Subcommand};
use dialoguer::{Confirm, MultiSelect, theme::ColorfulTheme};
use git2::{Repository, Signature};
use notify::{Config as WatcherConfig, RecommendedWatcher, RecursiveMode, Watcher};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::time::Duration;
use tokio::time::sleep;

#[derive(Parser)]
#[command(name = "autogit")]
#[command(about = "Automatically manage your Git repositories with safety guards", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Watch a directory for changes and auto-commit/push
    Watch {
        /// The path to the repository to watch
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Delay in seconds before committing after a change
        #[arg(short, long, default_value_t = 5)]
        delay: u64,

        /// Interactive mode: ask which files to stage
        #[arg(short, long)]
        interactive: bool,
    },
    /// Initialize the configuration file
    InitConfig,
    /// Link a local directory to a GitHub repository
    LinkRepo {
        /// The GitHub repository URL
        url: String,
        /// The local path (default: current directory)
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Generate a script for safe public release (no history)
    PublishInit {
        /// The PUBLIC GitHub repository URL
        url: String,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct AppConfig {
    #[serde(default = "default_ng_words")]
    pub ng_words: Vec<String>,
    #[serde(default = "default_token_patterns")]
    pub token_patterns: Vec<String>,
    #[serde(default = "default_risky_filenames")]
    pub risky_filenames: Vec<String>,
}

fn default_ng_words() -> Vec<String> {
    vec![
        "PRIVATE_NAME_HERE".to_string(),
        "YOUR_HOME_PATH_HERE".to_string(),
    ]
}

fn default_token_patterns() -> Vec<String> {
    vec![
        r"ghp_[a-zA-Z0-9]{36}".to_string(), // GitHub personal access token
        r"sk-[a-zA-Z0-9]{48}".to_string(),  // OpenAI API key
    ]
}

fn default_risky_filenames() -> Vec<String> {
    vec![
        "id_rsa".to_string(),
        "id_ed25519".to_string(),
        ".env".to_string(),
        "token.txt".to_string(),
        "secrets.json".to_string(),
    ]
}

impl AppConfig {
    fn load() -> Self {
        let config_path = dirs::home_dir()
            .map(|h| h.join(".autogit.json"))
            .unwrap_or_else(|| PathBuf::from(".autogit.json"));
        if let Ok(content) = fs::read_to_string(&config_path) {
            serde_json::from_str(&content).unwrap_or_else(|_| Self::default())
        } else {
            Self::default()
        }
    }

    fn save(&self) -> std::io::Result<()> {
        let config_path = dirs::home_dir()
            .map(|h| h.join(".autogit.json"))
            .unwrap_or_else(|| PathBuf::from(".autogit.json"));
        let content = serde_json::to_string_pretty(self).unwrap();
        fs::write(config_path, content)
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            ng_words: default_ng_words(),
            token_patterns: default_token_patterns(),
            risky_filenames: default_risky_filenames(),
        }
    }
}

// Mock dirs library for now to avoid additional dependency if possible, or just use hardcoded path for this user context
mod dirs {
    use std::path::PathBuf;
    pub fn home_dir() -> Option<PathBuf> {
        Some(PathBuf::from("/Users/primalcolors"))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::InitConfig => {
            let config = AppConfig::default();
            config.save()?;
            println!("設定ファイルを ~/.autogit.json に作成しました。");
            println!(
                "このファイルを編集して、あなたの本名や守りたいキーワードを登録してください。"
            );
        }
        Commands::Watch {
            path,
            delay,
            interactive,
        } => {
            let config = AppConfig::load();
            watch_repo(path, Duration::from_secs(delay), interactive, config).await?;
        }
        Commands::LinkRepo { url, path } => {
            println!(
                "ローカルディレクトリ {:?} をリモートリポジトリ {} に紐付けます...",
                path, url
            );
            link_repo(path, url)?;
            println!("\n[V] 紐付けが完了しました！以下のコマンドで監視を開始できます:");
            println!("    autogit watch . --interactive");
        }
        Commands::PublishInit { url } => {
            generate_publish_script(url)?;
            println!("\n[V] 公開用スクリプト 'publish_public.sh' を生成しました。");
            println!(
                "    このスクリプトを実行すると、現在のファイルをクリーンな状態で公開リポジリへ書き出します。"
            );
            println!(
                "    実行前に内容を確認し、chmod +x publish_public.sh で実行権限を付与してください。"
            );
        }
    }

    Ok(())
}

fn generate_publish_script(url: String) -> std::io::Result<()> {
    let ignore_file = ".autogit-publish-ignore";
    if !Path::new(ignore_file).exists() {
        println!(
            "公開用除外ルールファイル '{}' が見つからないため、デフォルトを作成します。",
            ignore_file
        );
        fs::write(
            ignore_file,
            "# AutoGit Public Publish Ignore\n# ここに記述したファイルは公開リポジトリには含まれません。\n\n.autogit.json\npublish_public.sh\n.autogit-publish-ignore\n# プライベートなメモや日記などは以下に追記してください\nprivate_notes/\n*.tmp\n",
        )?;
    }

    let script_content = format!(
        r#"#!/bin/bash
# AutoGit: Safe Public Release Script
# This script copies the current files to a temporary directory and pushes to a public repo WITHOUT history.

PUBLIC_REPO_URL="{}"
TEMP_DIR="/tmp/autogit_public_release_$(date +%s)"
PUBLISH_IGNORE=".autogit-publish-ignore"

echo "[1/4] Preparing temporary directory: $TEMP_DIR"
mkdir -p "$TEMP_DIR"

echo "[2/4] Copying files (excluding .git and sensitive data)..."
# Use rsync with a dedicated ignore file for public release
if [ -f "$PUBLISH_IGNORE" ]; then
    echo "Using $PUBLISH_IGNORE for exclusions."
    rsync -av --exclude='.git/' --exclude-from="$PUBLISH_IGNORE" ./ "$TEMP_DIR/"
else
    echo "Warning: $PUBLISH_IGNORE not found. Basic exclusion only."
    rsync -av --exclude='.git/' --exclude='target/' --exclude='publish_public.sh' ./ "$TEMP_DIR/"
fi

cd "$TEMP_DIR"

echo "[3/4] Initializing new clean repository..."
git init
git add .
git commit -m "Initial release via AutoGit safe-publish"

echo "[4/4] Pushing to public repository (FORCE PUSH)..."
git remote add origin "$PUBLIC_REPO_URL"
git push -u origin master --force || git push -u origin main --force

echo ""
echo "[V] Public release completed successfully!"
echo "Temporary directory $TEMP_DIR can be safely removed."
"#,
        url
    );

    fs::write("publish_public.sh", script_content)?;
    Ok(())
}

async fn watch_repo(
    path: PathBuf,
    delay: Duration,
    interactive: bool,
    config: AppConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("--- AutoGit 監視モード開始 ---");
    println!(
        "対象パス: {:?}",
        path.canonicalize().unwrap_or(path.clone())
    );
    println!(
        "待機時間: {}秒 | 対話モード: {}",
        delay.as_secs(),
        interactive
    );
    println!("\n[!] 重要: このターミナルを閉じると監視が止まります。");
    println!("    作業中は、このウィンドウを最小化して開きっぱなしにしておいてください。");

    // 0. Public Repo Guard
    if let Ok(repo) = Repository::open(&path) {
        if let Ok(remote) = repo.find_remote("origin") {
            if let Some(url) = remote.url() {
                if is_github_public(url) {
                    println!("\n[!] 警告: 公開リポジトリを検知しました");
                    println!("このリポジトリはGitHub上で誰でも見られる状態です。");
                    println!("機密情報が公開されないよう、十分に注意してください。");
                    if !Confirm::new()
                        .with_prompt("このまま監視を開始しますか？")
                        .default(false)
                        .interact()?
                    {
                        println!("中止しました。");
                        return Ok(());
                    }
                } else if !url.contains("-private") {
                    println!("\n[!] 注意: リポジトリ名に '-private' が含まれていません。");
                    println!(
                        "プライベート用であれば、紛らわしさを避けるために名前に '-private' を付与することをおすすめします。"
                    );
                    if !Confirm::new()
                        .with_prompt("このまま監視を開始しますか？")
                        .default(true)
                        .interact()?
                    {
                        println!("中止しました。");
                        return Ok(());
                    }
                }
            }
        }
    }

    let (tx, rx) = channel();

    let mut watcher = RecommendedWatcher::new(tx, WatcherConfig::default())?;
    watcher.watch(&path, RecursiveMode::Recursive)?;

    println!("\n変更を監視しています... (中止するには Ctrl+C を押してください)");

    loop {
        if let Ok(Ok(event)) = rx.recv() {
            if is_ignored_path(&event.paths) {
                continue;
            }

            println!("\n[!] 変更を検知しました: {:?}", event.kind);
            println!("データが落ち着くまで {}秒待機します...", delay.as_secs());
            sleep(delay).await;

            // Drain pending events
            while let Ok(_) = rx.try_recv() {}

            println!("同期プロセスを開始します (セキュア・パトロール起動中)...");
            if let Err(e) = sync_git(&path, interactive, &config) {
                eprintln!("[X] 同期に失敗しました: {}", e);
            } else {
                println!("[V] 同期サイクルが完了しました。");
            }
            println!("\n次の変更を待っています...");
        }
    }
}

fn is_ignored_path(paths: &[PathBuf]) -> bool {
    for p in paths {
        let s = p.to_string_lossy();
        if s.contains("/.git/") || s.contains("/target/") {
            return true;
        }
    }
    false
}

fn sync_git(
    repo_path: &Path,
    interactive: bool,
    config: &AppConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo = Repository::open(repo_path)?;
    let mut index = repo.index()?;

    let mut status_opts = git2::StatusOptions::new();
    status_opts.include_untracked(true);
    let statuses = repo.statuses(Some(&mut status_opts))?;

    if statuses.is_empty() {
        println!("変更されたファイルはありません。");
        return Ok(());
    }

    let mut files_to_stage = Vec::new();
    let mut warnings = Vec::new();

    for entry in statuses.iter() {
        if let Some(path) = entry.path() {
            // Safety Check
            let full_path = repo_path.join(path);
            if let Some(reason) = check_file_safety(&full_path, path, config) {
                warnings.push((path.to_string(), reason));
            }
            files_to_stage.push(path.to_string());
        }
    }

    if !warnings.is_empty() {
        println!("\n[!] 警告: 以下のファイルに機密情報の可能性があります:");
        for (path, reason) in &warnings {
            println!("  - {}: {}", path, reason);
        }

        let proceed = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("これらのファイルをそのままGitHubへ公開してもよろしいですか？")
            .default(false)
            .interact()?;

        if !proceed {
            println!("同期を中断しました。ファイルを修正してから再度お試しください。");
            return Ok(());
        }
    }

    if interactive {
        println!("\n--- どの変更をGitHubへ公開（ステージ）しますか？ ---");
        let available_files: Vec<_> = files_to_stage.iter().cloned().collect();

        let defaults: Vec<bool> = vec![true; available_files.len()];
        let selections = MultiSelect::with_theme(&ColorfulTheme::default())
            .with_prompt("スペースキーで選択・解除、Enterで確定してください")
            .items(&available_files)
            .defaults(&defaults)
            .interact()?;

        let mut final_selection = Vec::new();
        for idx in selections {
            final_selection.push(available_files[idx].clone());
        }
        files_to_stage = final_selection;
    }

    if files_to_stage.is_empty() {
        println!("何も選択されませんでした。今回はスキップします。");
        return Ok(());
    }

    // Stage selected files
    for path in &files_to_stage {
        index.add_path(Path::new(path))?;
    }
    index.write()?;

    // Commit
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;
    let sig = repo
        .signature()
        .or_else(|_| Signature::now("AutoGit", "autogit@example.com"))?;

    let head = repo.head()?;
    let parent_commit = head.peel_to_commit()?;

    let commit_id = repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        "auto update by autogit",
        &tree,
        &[&parent_commit],
    )?;

    println!("[C] コミット完了: {}", commit_id);

    // Push
    println!("GitHubへプッシュ中 (origin)...");
    let mut remote = repo.find_remote("origin")?;
    let mut callbacks = git2::RemoteCallbacks::new();

    callbacks.credentials(|_url, _username_from_url, _allowed_types| {
        git2::Cred::ssh_key_from_agent("git")
    });

    let mut push_opts = git2::PushOptions::new();
    push_opts.remote_callbacks(callbacks);

    let branch_name = repo.head()?.shorthand().unwrap_or("main").to_string();
    let refspec = format!("refs/heads/{}:refs/heads/{}", branch_name, branch_name);

    remote.push(&[&refspec], Some(&mut push_opts))?;
    println!("[P] プッシュ完了: origin/{}", branch_name);

    Ok(())
}

fn link_repo(path: PathBuf, url: String) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Git Init
    let repo = if !path.join(".git").exists() {
        println!("新しいGitリポジトリを初期化します...");
        Repository::init(&path)?
    } else {
        println!("既存のGitリポジトリが見つかりました。");
        Repository::open(&path)?
    };

    // 2. Add Remote
    if repo.find_remote("origin").is_ok() {
        println!("既に 'origin' が設定されています。URLを更新します: {}", url);
        repo.remote_set_url("origin", &url)?;
    } else {
        println!("リモート 'origin' を追加します: {}", url);
        repo.remote("origin", &url)?;
    }

    // 3. Create .gitignore if not exists
    let gitignore_path = path.join(".gitignore");
    if !gitignore_path.exists() {
        println!("基本的な .gitignore を作成します...");
        fs::write(gitignore_path, "target/\n.DS_Store\n")?;
    }

    Ok(())
}

fn is_github_public(url: &str) -> bool {
    // HTTPS or SSH URL handles
    let https_url = if url.starts_with("git@github.com:") {
        url.replace("git@github.com:", "https://github.com/")
            .replace(".git", "")
    } else {
        url.to_string()
    };

    if !https_url.contains("github.com") {
        return false;
    }

    // Use curl to check if the page is visible (HTTP 200)
    let output = std::process::Command::new("curl")
        .arg("-I")
        .arg("-s")
        .arg("-o")
        .arg("/dev/null")
        .arg("-w")
        .arg("%{http_code}")
        .arg(&https_url)
        .output();

    if let Ok(out) = output {
        let code = String::from_utf8_lossy(&out.stdout);
        code == "200"
    } else {
        false
    }
}

fn check_file_safety(full_path: &Path, _rel_path: &str, config: &AppConfig) -> Option<String> {
    // 1. Filename check
    let filename = full_path.file_name()?.to_string_lossy();
    for risky in &config.risky_filenames {
        if filename.contains(risky) {
            return Some(format!("危ないファイル名です ({})", risky));
        }
    }

    // 2. Content check (only for text-like files, simplified)
    if let Ok(content) = fs::read_to_string(full_path) {
        // NG words
        for word in &config.ng_words {
            if !word.is_empty() && content.contains(word) {
                return Some(format!("NGワード '{}' が含まれています", word));
            }
        }
        // Regex patterns
        for pattern in &config.token_patterns {
            if let Ok(re) = Regex::new(pattern) {
                if re.is_match(&content) {
                    return Some("機密情報（トークン等）のパターンに一致しました".to_string());
                }
            }
        }
    }

    None
}
