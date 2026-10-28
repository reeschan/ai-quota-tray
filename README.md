# AI Quota

Windows のトレイに常駐し、使っている AI サービスの**残量・残高・支出**を表示するアプリです。数値がソースに無いときは「取得できません」と理由を出し、推定や 0 の埋め合わせはしません。

UI は日本語です。コードとコメントは英語です。

## 何が動くか

| プロバイダ | 状態 | ソース | 表示するもの |
| --- | --- | --- | --- |
| DeepSeek | 実装済み | 公式 | `GET https://api.deepseek.com/user/balance` の残高。USD を優先し、付与残高とチャージ残高を注記 |
| xAI Grok | 実装済み | 公式 | Management API の前払い残高。`total.val` は反転した USD セントなので、表示は `-total.val / 100` |
| OpenAI API | 実装済み | 公式 | Admin キーの `GET /v1/organization/costs`。直近約 30 日の支出。**残高 API は無い** |
| Anthropic API | 実装済み | 公式 | Admin キー（`sk-ant-admin`）の `cost_report`。直近約 30 日の支出と、メッセージ使用量のトークン合計。**残高 API は無い。残量%は作らない** |
| Claude Code | 実装済み | ローカル | statusLine が渡す `rate_limits.five_hour` / `seven_day`（とあれば `spend_limit`）。任意で直近 7 日の JSONL トークン合計 |
| Codex | 実装済み | ローカル | `~/.codex/sessions/**/rollout-*.jsonl` 末尾の `token_count.rate_limits`。任意で `codex app-server` の `account/rateLimits/read` |
| OpenCode | 実装済み | ローカル | `opencode*.db` の `cost` 合計。Go の使用率 API は非公式で既定オフ |
| Antigravity | スタブ | 手動 | 公開 API が無い。`/usage` と `/credits` は対話画面で確認する。数値を自分で JSON に書いたときだけ表示 |

優先して作り込んだのは DeepSeek、xAI、Codex のローカル読み取り、Claude Code の statusLine です。

## ソースの区分

- **公式** — そのベンダーが文書化している HTTP API。
- **ローカル** — 自分のマシン上のファイル、または公式 CLI が書いた値。ネットワークへ認証情報を送りません。
- **非公式** — 文書化された契約ではないエンドポイント。既定はオフで、設定で明示したときだけ呼びます。

非公式をオンにしても、ブラウザのクッキーや `~/.claude/.credentials.json` は読みません。トークンは資格情報マネージャーに自分で入れたものだけを使います。

## ToS とやらないこと

- Anthropic の利用規約に抵触するため、`api.anthropic.com/api/oauth/usage` は呼びません。`~/.claude/.credentials.json` のトークンも読みません。Claude のサブスクリプション枠は、Claude Code 自身が statusLine に渡す値だけを使います。
- ChatGPT の `https://chatgpt.com/backend-api/wham/usage` は非公式です。既定オフです。クッキーの抜き取りはしません。
- OpenCode Go の `GET https://opencode.ai/zen/go/v1/usage` は、このアプリでは非公式のオプトインです。既定オフです。
- `agy -p '/usage'` は実行しません。print モードはスラッシュコマンドにならず、通常のプロンプトとして枠を消費します。
- 取れなかった使用率をトークン数から計算したり、空応答を 0 として表示したりしません。

## Windows でのビルド

前提: Rust（stable）、Node.js 22 前後、Visual Studio の C++ ビルドツール。WebView2 は Windows 11 には入っています。msi を作るときは Tauri が WiX / NSIS を使います。

```powershell
.\scripts\windows-build.ps1
.\scripts\windows-build.ps1 -Bundle
```

`-Bundle` なしでは `apps\desktop\src-tauri\target\release\ai-quota.exe` ができます。`-Bundle` では `apps\desktop\src-tauri\target\release\bundle` に `.msi` と NSIS の `.exe` インストーラができます。

リリースビルドは起動時にウィンドウを隠してトレイに入ります。トレイの左クリックかメニューの「開く」で表示します。閉じるボタンは終了ではなく隠すだけです。デバッグビルド（`cargo run -p quota-desktop`）は起動時にウィンドウを開きます。

開発中の UI だけ見る場合:

```powershell
cd apps\desktop
npm ci
npm run dev
```

`http://127.0.0.1:43123` はサンプルデータです。実データの取得はトレイアプリの中だけです。

## ダウンロード / インストール

最新の Windows ビルドは GitHub Releases にあります。

https://github.com/reeschan/ai-quota-tray/releases/latest

`v*` タグの Release には次が付きます。

- `.msi` — WiX のインストーラ。トレイアプリを入れます。
- `*-setup.exe` — NSIS のセットアップ。同じトレイアプリです。
- `quota-<version>-windows-x64.zip` — 展開すると CLI の `quota.exe` だけが入っています。

Windows 11 には WebView2 が入っています。自分でビルドするときの前提は「Windows でのビルド」と同じです。

コード署名用の証明書はまだありません。Release の `.msi` と `.exe` は未署名です。ダウンロードすると Windows SmartScreen が「Windows によって PC が保護されました」と表示することがあります。ファイルの入手元がこのリポジトリの Releases であることを確認したうえで、「詳細情報」から実行してください。Windows 11 で Smart App Control が有効なときは、未署名のアプリがブロックされ、そのアプリだけを許可することはできません。

## Linux での core テスト

GUI なしで core と CLI を検証できます。

```bash
cargo fmt --all --check
cargo clippy -p quota-core -p quota-cli --all-targets -- -D warnings
cargo test -p quota-core -p quota-cli
```

トレイアプリも Linux でコンパイルする場合は WebKitGTK 4.1 の開発パッケージが要ります。先に `apps/desktop` で `npm ci && npm run build` してから `cargo build -p quota-desktop` です。`frontendDist` の `apps/desktop/dist` が無いと Tauri のビルドが失敗します。

GitHub Actions の CI は `main` への push と pull request で動きます。Ubuntu では `quota-desktop` を除いた fmt、clippy、test と、トレイ UI の型チェックおよびビルドを行います。Windows ではワークスペース全体の fmt、clippy、test と、同じ UI ビルドを行います。`.msi` と NSIS のインストーラは、下のリリースタグを push したときに作ります。

## リリース手順

1. バージョン `X.Y.Z` を次の 3 か所に書いてコミットする。クレートのバージョンは workspace の値を参照しています。
   - `Cargo.toml` の `[workspace.package] version`
   - `apps/desktop/src-tauri/tauri.conf.json` の `version`
   - `apps/desktop/package.json` の `version`
2. そのコミットにタグ `vX.Y.Z` を打ち、タグだけを push する。`v` で始まるタグが Release ワークフローを起動します。

```bash
git tag vX.Y.Z
git push origin vX.Y.Z
```

3. Windows の runner が、ビルドの前にタグから `v` を除いた番号を上の 3 ファイルと `Cargo.lock` へ書き込みます。成果物のバージョンはタグが基準です。`.msi`、NSIS の setup `.exe`、`quota-<version>-windows-x64.zip` が GitHub Release に載ります。Release は draft ではなく、リリースノートは GitHub が生成します。

署名は任意です。次の secrets がすべて揃っているときだけ、証明書を runner に読み込み、`tauri.conf.json` に拇印を書いて署名します。一つでも欠けていると失敗します。どれも無ければ未署名のまま公開します。値はログに出しません。

- `WINDOWS_CERTIFICATE` — `.pfx` を Base64 にしたもの
- `WINDOWS_CERTIFICATE_PASSWORD` — その pfx のパスワード
- `WINDOWS_CERTIFICATE_THUMBPRINT` — 証明書の拇印

既にあるタグを指定して、Actions から Release ワークフローを手動実行することもできます。入力 `tag` は `vX.Y.Z` です。

## 設定とキー

設定ファイルは秘密情報を持ちません。

- Windows: `%APPDATA%\ai-quota\config.toml`
- Linux: `~/.config/ai-quota/config.toml`

履歴 SQLite は `%LOCALAPPDATA%\ai-quota\history.sqlite`（Linux は `~/.local/share/ai-quota/history.sqlite`）です。プロバイダごとに直近 400 件を残します。

キーは OS の資格情報マネージャーです。Windows では Credential Manager、Linux では Secret Service です。サービス名は `ai-quota` です。値は標準入力から渡します。コマンドライン引数には載せません。

```powershell
# PowerShell
"sk-..." | quota set-secret deepseek_api_key
quota delete-secret deepseek_api_key
```

```bash
printf '%s' "$DEEPSEEK_API_KEY" | quota set-secret deepseek_api_key
```

使える名前:

| 名前 | 用途 |
| --- | --- |
| `deepseek_api_key` | DeepSeek の API キー |
| `xai_management_key` | xAI の **Management** キー。推論キーでは残高は取れません |
| `openai_admin_key` | OpenAI の Admin キー |
| `anthropic_admin_key` | `sk-ant-admin...` |
| `opencode_api_key` | OpenCode Go の非公式 API をオンにしたときだけ |
| `chatgpt_access_token` | wham/usage をオンにしたときだけ。自分で貼る |

xAI の team id は秘密ではないので `config.toml` の `[providers.xai] team_id` に書きます。英数字、ハイフン、アンダースコアだけを受け付けます。

例は `config.example.toml` です。更新間隔の既定は 600 秒です。60 秒未満は 60 秒に、0 の使用率しきい値は通知オフになります。使用率 80% で一度だけ通知し、下回るとまた通知できる状態に戻します。残高通知は同じ通貨のときだけです。既定では残高通知はオフです。

失敗した取得は 30 秒から倍々にし、通常の更新間隔で頭打ちにします。Claude の statusLine キャッシュ、Codex のセッション、OpenCode の db はファイル変更でも更新します。

## Claude Code の statusLine

Claude Code v2.1.80 以降は、statusLine コマンドの標準入力に `rate_limits.five_hour` と `rate_limits.seven_day`（`used_percentage` と `resets_at`）を渡します。このアプリはその JSON をローカルファイルに書くだけです。

インストール:

```bash
quota install-statusline
```

トレイの「設定」にも同じボタンがあります。次のファイルを `~/.claude/`（Windows も `%USERPROFILE%\.claude`）へコピーし、`settings.json` に `statusLine` が**無いときだけ**足します。既にある設定は上書きしません。変更前に `settings.json.ai-quota.bak` を残します。

- `ai-quota-statusline.ps1`（Windows）
- `ai-quota-statusline.sh`（bash / Git Bash。`APPDATA` があればそこへ書く）

手で書く場合:

```json
{
  "statusLine": {
    "type": "command",
    "command": "powershell.exe -NoProfile -ExecutionPolicy Bypass -File \"C:\\Users\\you\\.claude\\ai-quota-statusline.ps1\""
  }
}
```

キャッシュの場所は `%APPDATA%\ai-quota\claude-statusline.json` です。Claude Code で最初の応答があるまで `rate_limits` は来ません。12 時間より古い値には注記が付き、7 日より古い値はエラーにします。数値自体は最後に受け取ったものを残します。

任意のトークン集計は `%USERPROFILE%\.claude\projects\**\*.jsonl` のうち、直近 7 日に更新され、1 ファイル 8MB・合計 200 ファイル以内のものだけです。同じメッセージ ID は最大値だけを数えます。これはクォータではありません。条件を超えたら部分合計は出しません。

## Codex

セッションは `%USERPROFILE%\.codex\sessions\YYYY\MM\DD\rollout-*.jsonl`（`CODEX_HOME` があればそこ）と、同じ階層の `archived_sessions` です。各ファイルの末尾 512KB から、最後の `rate_limits` を読みます。`used_percent` が無いウィンドウは出しません。`resets_in_seconds` しか無いときは、その行の `timestamp` から絶対時刻を計算します。

`try_app_server = true` のときだけ、ローカルに rate limit が無い場合に `codex app-server` へ `account/rateLimits/read` を送ります。8 秒で打ち切ります。ChatGPT ログインが無いと Codex 側がエラーを返します。その文字列をそのまま理由にします。

`allow_unofficial_wham = true` かつ `chatgpt_access_token` があるときだけ、ローカルも app-server も失敗したあとで wham/usage を試します。成功したスナップショットのソースは「非公式」です。

## OpenCode と Antigravity

OpenCode は次のディレクトリにある `opencode*.db` を読み取り専用で開きます。

- `OPENCODE_DATA_DIR`
- `$XDG_DATA_HOME/opencode`
- `~/.local/share/opencode`
- OS のデータディレクトリの `opencode`

`message` / `session_message` / `part` の `cost` 列、または `data` JSON の `cost` を合計します。行が無ければ 0 円にはしません。通貨コードは列に無いため、OpenCode が USD で記録する前提であることを注記します。`opencode stats` のテキスト出力は安定した契約ではないので解析しません。

Go の使用率は `allow_unofficial_usage_api = true` のときだけ `GET https://opencode.ai/zen/go/v1/usage` です。`percent` が無い枠は作りません。

Antigravity は既定オフです。有効にすると、次のファイルが無いあいだは手動確認の案内だけを出します。

`%APPDATA%\ai-quota\antigravity.json`

自分で `/usage` や `/credits` を見て、次の形で保存したときだけバーと金額を出します。

```json
{
  "windows": [{ "label": "週次", "used_pct": 37, "resets_at": "2026-11-03T00:00:00Z" }],
  "balance": { "amount": "4.50", "currency": "USD" }
}
```

## CLI

```bash
quota              # 日本語の要約
quota --json       # スナップショットの配列
quota --json --strict
quota --provider deepseek --json
quota config
quota install-statusline
```

`--strict` は、いずれかに `error` があるとき終了コード 1 です。設定や入出力の失敗は 2 です。JSON の `used_pct` / `balance` / `spend` は、ソースが値を返したときだけ入ります。

## レイアウト

```
crates/quota-core     Provider、設定、SQLite、通知判定
crates/quota-cli      quota コマンド
apps/desktop          Tauri v2 のトレイとポップアップ
```

`Provider::fetch` は `Result<Snapshot, ProviderError>` です。パーサのテストは `crates/quota-core/fixtures` の JSON / JSONL を使います。
