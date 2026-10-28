import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type Source = "official" | "local" | "unofficial";

interface Money {
  amount: string;
  currency: string;
}

interface UsageWindow {
  label: string;
  used_pct: number | null;
  resets_at: string | null;
}

interface Snapshot {
  provider: string;
  display_name: string;
  windows: UsageWindow[];
  balance: Money | null;
  spend: Money | null;
  source: Source;
  fetched_at: string;
  error: string | null;
  notes: string[];
}

interface Toggle {
  enabled: boolean;
}

interface Settings {
  general: {
    refresh_secs: number;
    notify_used_pct: number | null;
    notify_balance_below: string | null;
    notify_balance_currency: string;
  };
  providers: {
    deepseek: Toggle;
    xai: { enabled: boolean; team_id: string };
    openai: Toggle;
    anthropic: Toggle;
    claude_code: {
      enabled: boolean;
      cache_path: string | null;
      projects_dir: string | null;
      aggregate_tokens: boolean;
    };
    codex: {
      enabled: boolean;
      sessions_dir: string | null;
      allow_unofficial_wham: boolean;
      try_app_server: boolean;
    };
    opencode: {
      enabled: boolean;
      data_dir: string | null;
      allow_unofficial_usage_api: boolean;
    };
    antigravity: { enabled: boolean; cache_path: string | null };
  };
}

interface SecretInfo {
  name: string;
  label: string;
  configured: boolean;
  error: string | null;
}

const SOURCE: Record<Source, string> = {
  official: "公式",
  local: "ローカル",
  unofficial: "非公式",
};

function mount(): HTMLDivElement {
  const node = document.querySelector("#app");
  if (!(node instanceof HTMLDivElement)) {
    throw new Error("missing #app");
  }
  return node;
}

const app = mount();

let tab: "usage" | "settings" = "usage";
let snapshots: Snapshot[] = [];
let settings: Settings | null = null;
let secrets: SecretInfo[] = [];
let loading = false;
let message = "";
let error = "";
const inTauri = "__TAURI_INTERNALS__" in window;

function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

function resetLabel(iso: string | null, now: number): string {
  if (!iso) return "リセット時刻なし";
  const at = Date.parse(iso);
  if (Number.isNaN(at)) return "リセット時刻なし";
  const diff = at - now;
  if (diff <= 0) return "リセット済み";
  const minutes = Math.floor(diff / 60000);
  const days = Math.floor(minutes / 1440);
  const hours = Math.floor((minutes % 1440) / 60);
  const rest = minutes % 60;
  if (days > 0) return `あと ${days}日 ${hours}時間`;
  if (hours > 0) return `あと ${hours}時間 ${rest}分`;
  return `あと ${Math.max(rest, 1)}分`;
}

function fetchedLabel(iso: string, now: number): string {
  const at = Date.parse(iso);
  if (Number.isNaN(at)) return "取得時刻不明";
  const minutes = Math.floor(Math.max(0, now - at) / 60000);
  if (minutes < 1) return "たった今";
  if (minutes < 60) return `${minutes}分前`;
  const hours = Math.floor(minutes / 60);
  if (hours < 48) return `${hours}時間前`;
  return `${Math.floor(hours / 24)}日前`;
}

function barClass(percent: number): string {
  if (percent >= 95) return "fill hot";
  if (percent >= 80) return "fill warn";
  return "fill";
}

function renderCard(snapshot: Snapshot, now: number): string {
  const windows = snapshot.windows
    .map((window) => {
      const percent = window.used_pct;
      const width = percent == null ? 0 : Math.max(0, Math.min(percent, 100));
      const text = percent == null ? "使用率なし" : `${percent.toFixed(percent % 1 === 0 ? 0 : 1)}%`;
      return `<div class="window">
        <div class="meta"><span>${escapeHtml(window.label)}</span><span class="pct">${text}</span></div>
        <div class="track"><div class="${percent == null ? "fill" : barClass(percent)}" style="width:${width}%"></div></div>
        <div class="meta"><span>${escapeHtml(resetLabel(window.resets_at, now))}</span></div>
      </div>`;
    })
    .join("");
  const balance = snapshot.balance
    ? `<div class="money"><span>残高</span><span class="pct">${escapeHtml(snapshot.balance.amount)} ${escapeHtml(snapshot.balance.currency)}</span></div>`
    : "";
  const spend = snapshot.spend
    ? `<div class="money"><span>支出</span><span class="pct">${escapeHtml(snapshot.spend.amount)} ${escapeHtml(snapshot.spend.currency)}</span></div>`
    : "";
  const warn =
    snapshot.error && (snapshot.windows.length > 0 || snapshot.balance || snapshot.spend)
      ? `<div class="warn">一部取得できません: ${escapeHtml(snapshot.error)}</div>`
      : "";
  const unavailable =
    snapshot.error && snapshot.windows.length === 0 && !snapshot.balance && !snapshot.spend
      ? `<div class="warn">取得できません<br>${escapeHtml(snapshot.error)}</div>`
      : "";
  const notes = snapshot.notes.map((note) => `<p class="note">${escapeHtml(note)}</p>`).join("");
  return `<article class="card">
    <header>
      <h2>${escapeHtml(snapshot.display_name)}</h2>
      <span class="badge ${snapshot.source}">${SOURCE[snapshot.source]}</span>
    </header>
    ${warn}${unavailable}${windows}${balance}${spend}${notes}
    <div class="foot"><span>${escapeHtml(fetchedLabel(snapshot.fetched_at, now))}</span></div>
  </article>`;
}

function checked(value: boolean): string {
  return value ? "checked" : "";
}

function renderSettings(): string {
  if (!settings) return `<p class="empty">設定を読み込んでいます…</p>`;
  const secretRows = secrets
    .map((secret) => {
      const state = secret.error
        ? escapeHtml(secret.error)
        : secret.configured
          ? "保存済み"
          : "未設定";
      return `<div class="provider">
        <div class="row"><strong>${escapeHtml(secret.label)}</strong><span class="note">${state}</span></div>
        <div class="secret">
          <input type="password" autocomplete="off" data-secret="${escapeHtml(secret.name)}" placeholder="貼り付けて保存" />
          <button type="button" class="primary" data-save-secret="${escapeHtml(secret.name)}">保存</button>
          <button type="button" class="ghost" data-delete-secret="${escapeHtml(secret.name)}">削除</button>
        </div>
        <p>値は OS の資格情報マネージャーに入り、設定ファイルには書きません。</p>
      </div>`;
    })
    .join("");
  const general = settings.general;
  const providers = settings.providers;
  return `<form id="settings-form">
    <label class="field">更新間隔（秒、60〜86400。目安は 5〜15 分）
      <input name="refresh_secs" type="number" min="60" max="86400" value="${general.refresh_secs}" />
    </label>
    <label class="field">使用率がこの値以上で通知（空欄でオフ）
      <input name="notify_used_pct" type="number" min="1" max="100" value="${general.notify_used_pct ?? ""}" />
    </label>
    <label class="field">残高がこの額を下回ったら通知（同じ通貨だけ。空欄でオフ）
      <input name="notify_balance_below" type="text" value="${escapeHtml(general.notify_balance_below ?? "")}" placeholder="5.00" />
    </label>
    <label class="field">残高通知の通貨
      <input name="notify_balance_currency" type="text" value="${escapeHtml(general.notify_balance_currency)}" />
    </label>
    ${toggle("Claude Code（statusLine）", "claude_code", providers.claude_code.enabled, "5時間と7日の使用率は、Claude Code が渡す statusLine だけを読みます。")}
    <label class="field"><input type="checkbox" name="aggregate_tokens" ${checked(providers.claude_code.aggregate_tokens)} /> 直近7日のトランスクリプトからトークン合計を足す（残量%にはしません）</label>
    ${toggle("Codex（ローカルセッション）", "codex", providers.codex.enabled, "~/.codex/sessions の rollout JSONL 末尾にある rate_limits を読みます。")}
    <label class="field"><input type="checkbox" name="try_app_server" ${checked(providers.codex.try_app_server)} /> ローカルに rate_limits がないとき codex app-server を試す</label>
    <label class="field"><input type="checkbox" name="allow_unofficial_wham" ${checked(providers.codex.allow_unofficial_wham)} /> 非公式の wham/usage を許可（既定オフ。ブラウザのクッキーは読みません）</label>
    ${toggle("DeepSeek", "deepseek", providers.deepseek.enabled, "公式の GET /user/balance。")}
    ${toggle("xAI Grok", "xai", providers.xai.enabled, "Management キーと team id。前払い残高は公式の反転セントです。")}
    <label class="field">xAI team id
      <input name="team_id" type="text" value="${escapeHtml(providers.xai.team_id)}" placeholder="チーム ID" />
    </label>
    ${toggle("OpenAI API", "openai", providers.openai.enabled, "Admin キーの organization/costs。残高 API はありません。")}
    ${toggle("Anthropic API", "anthropic", providers.anthropic.enabled, "sk-ant-admin の cost_report。残高 API はありません。")}
    ${toggle("OpenCode", "opencode", providers.opencode.enabled, "ローカルの opencode*.db にある cost。")}
    <label class="field"><input type="checkbox" name="allow_unofficial_usage_api" ${checked(providers.opencode.allow_unofficial_usage_api)} /> OpenCode Go の使用量 API を許可（非公式・既定オフ）</label>
    ${toggle("Antigravity（実験）", "antigravity", providers.antigravity.enabled, "公開 API はありません。/usage と /credits は手動確認です。")}
    <div class="actions">
      <button class="primary" type="submit">設定を保存</button>
      <button class="ghost" type="button" id="install-statusline">statusLine をインストール</button>
    </div>
    <p class="help">Claude Code の <code>~/.claude/settings.json</code> に、同梱スクリプトを statusLine として登録します。既に statusLine がある場合は上書きしません。認証情報ファイルは読みません。</p>
    <h2>キー</h2>
    ${secretRows || `<p class="empty">キー項目を読み込めませんでした。</p>`}
  </form>`;
}

function toggle(title: string, name: string, enabled: boolean, help: string): string {
  return `<div class="provider">
    <label class="row"><strong>${escapeHtml(title)}</strong><input type="checkbox" name="${name}" ${checked(enabled)} /></label>
    <p>${escapeHtml(help)}</p>
  </div>`;
}

function render(): void {
  const now = Date.now();
  const banner = inTauri
    ? ""
    : `<div class="banner">これはブラウザ用のサンプルです。実際の残量ではありません。トレイアプリの中では取得した値だけを出します。</div>`;
  const status = error
    ? `<div class="warn">${escapeHtml(error)}</div>`
    : message
      ? `<p class="note">${escapeHtml(message)}</p>`
      : "";
  const body =
    tab === "settings"
      ? renderSettings()
      : loading && snapshots.length === 0
        ? `<p class="empty">取得しています…</p>`
        : snapshots.length === 0
          ? `<p class="empty">表示できるプロバイダがありません。設定で有効にしてください。</p>`
          : snapshots.map((snapshot) => renderCard(snapshot, now)).join("");
  app.innerHTML = `
    ${banner}
    <div class="top">
      <div>
        <h1>AI使用量</h1>
        <p class="lede">取得できた数字だけを表示します。推定は出しません。</p>
      </div>
      <button class="primary" id="refresh" type="button" ${loading ? "disabled" : ""}>${loading ? "更新中" : "更新"}</button>
    </div>
    <div class="tabs">
      <button type="button" data-tab="usage" aria-selected="${tab === "usage"}">使用量</button>
      <button type="button" data-tab="settings" aria-selected="${tab === "settings"}">設定</button>
    </div>
    ${status}
    ${body}
  `;
  document.querySelectorAll<HTMLButtonElement>("[data-tab]").forEach((button) => {
    button.addEventListener("click", () => {
      tab = button.dataset.tab === "settings" ? "settings" : "usage";
      render();
    });
  });
  document.querySelector("#refresh")?.addEventListener("click", () => {
    void refresh();
  });
  document.querySelector("#settings-form")?.addEventListener("submit", (event) => {
    event.preventDefault();
    void saveSettings();
  });
  document.querySelector("#install-statusline")?.addEventListener("click", () => {
    void installStatusline();
  });
  document.querySelectorAll<HTMLButtonElement>("[data-save-secret]").forEach((button) => {
    button.addEventListener("click", () => {
      const name = button.dataset.saveSecret;
      if (name) void saveSecret(name);
    });
  });
  document.querySelectorAll<HTMLButtonElement>("[data-delete-secret]").forEach((button) => {
    button.addEventListener("click", () => {
      const name = button.dataset.deleteSecret;
      if (name) void removeSecret(name);
    });
  });
}

async function refresh(): Promise<void> {
  if (!inTauri) {
    snapshots = sampleSnapshots();
    loading = false;
    render();
    return;
  }
  loading = true;
  error = "";
  render();
  try {
    snapshots = await invoke<Snapshot[]>("refresh");
  } catch (err) {
    error = String(err);
  } finally {
    loading = false;
    render();
  }
}

async function loadCached(): Promise<void> {
  if (!inTauri) {
    snapshots = sampleSnapshots();
    settings = sampleSettings();
    secrets = sampleSecrets();
    render();
    return;
  }
  try {
    snapshots = await invoke<Snapshot[]>("snapshots");
    settings = await invoke<Settings>("get_settings");
    secrets = await invoke<SecretInfo[]>("secret_status");
  } catch (err) {
    error = String(err);
  }
  render();
}

function formData(): FormData | null {
  const form = document.querySelector<HTMLFormElement>("#settings-form");
  return form ? new FormData(form) : null;
}

function flag(data: FormData, name: string): boolean {
  return data.get(name) === "on";
}

async function saveSettings(): Promise<void> {
  if (!settings || !inTauri) {
    message = "プレビューでは設定を保存しません。";
    render();
    return;
  }
  const data = formData();
  if (!data) return;
  const next: Settings = structuredClone(settings);
  next.general.refresh_secs = Number(data.get("refresh_secs") ?? 600);
  const pct = String(data.get("notify_used_pct") ?? "").trim();
  next.general.notify_used_pct = pct === "" ? null : Number(pct);
  const balance = String(data.get("notify_balance_below") ?? "").trim();
  next.general.notify_balance_below = balance === "" ? null : balance;
  next.general.notify_balance_currency = String(data.get("notify_balance_currency") ?? "USD");
  next.providers.claude_code.enabled = flag(data, "claude_code");
  next.providers.claude_code.aggregate_tokens = flag(data, "aggregate_tokens");
  next.providers.codex.enabled = flag(data, "codex");
  next.providers.codex.try_app_server = flag(data, "try_app_server");
  next.providers.codex.allow_unofficial_wham = flag(data, "allow_unofficial_wham");
  next.providers.deepseek.enabled = flag(data, "deepseek");
  next.providers.xai.enabled = flag(data, "xai");
  next.providers.xai.team_id = String(data.get("team_id") ?? "");
  next.providers.openai.enabled = flag(data, "openai");
  next.providers.anthropic.enabled = flag(data, "anthropic");
  next.providers.opencode.enabled = flag(data, "opencode");
  next.providers.opencode.allow_unofficial_usage_api = flag(data, "allow_unofficial_usage_api");
  next.providers.antigravity.enabled = flag(data, "antigravity");
  try {
    settings = await invoke<Settings>("save_settings", { config: next });
    message = "設定を保存しました。";
    error = "";
  } catch (err) {
    error = String(err);
  }
  render();
}

async function saveSecret(name: string): Promise<void> {
  const input = document.querySelector<HTMLInputElement>(`input[data-secret="${cssEscape(name)}"]`);
  const value = input?.value ?? "";
  if (!inTauri) {
    message = "プレビューではキーを保存しません。";
    render();
    return;
  }
  try {
    await invoke("set_secret", { name, value });
    if (input) input.value = "";
    secrets = await invoke<SecretInfo[]>("secret_status");
    message = "資格情報を保存しました。";
    error = "";
  } catch (err) {
    error = String(err);
  }
  render();
}

async function removeSecret(name: string): Promise<void> {
  if (!inTauri) return;
  try {
    await invoke("delete_secret", { name });
    secrets = await invoke<SecretInfo[]>("secret_status");
    message = "資格情報を削除しました。";
    error = "";
  } catch (err) {
    error = String(err);
  }
  render();
}

async function installStatusline(): Promise<void> {
  if (!inTauri) {
    message = "インストールはトレイアプリから実行します。";
    render();
    return;
  }
  try {
    message = await invoke<string>("install_statusline");
    error = "";
  } catch (err) {
    error = String(err);
  }
  render();
}

function cssEscape(value: string): string {
  return value.replaceAll("\\", "\\\\").replaceAll('"', '\\"');
}

function sampleSnapshots(): Snapshot[] {
  const soon = new Date(Date.now() + 2 * 60 * 60 * 1000).toISOString();
  const week = new Date(Date.now() + 3 * 24 * 60 * 60 * 1000).toISOString();
  const now = new Date().toISOString();
  return [
    {
      provider: "claude_code",
      display_name: "Claude Code",
      windows: [
        { label: "5時間", used_pct: 62, resets_at: soon },
        { label: "7日", used_pct: 34, resets_at: week },
      ],
      balance: null,
      spend: null,
      source: "local",
      fetched_at: now,
      error: null,
      notes: ["サンプル: statusLine キャッシュ"],
    },
    {
      provider: "codex",
      display_name: "Codex",
      windows: [{ label: "5時間", used_pct: 18, resets_at: soon }],
      balance: null,
      spend: null,
      source: "local",
      fetched_at: now,
      error: null,
      notes: ["サンプル: rollout JSONL"],
    },
    {
      provider: "deepseek",
      display_name: "DeepSeek",
      windows: [],
      balance: { amount: "42.00", currency: "USD" },
      spend: null,
      source: "official",
      fetched_at: now,
      error: null,
      notes: ["サンプル"],
    },
    {
      provider: "xai",
      display_name: "xAI Grok",
      windows: [],
      balance: { amount: "10.00", currency: "USD" },
      spend: null,
      source: "official",
      fetched_at: now,
      error: null,
      notes: ["サンプル"],
    },
    {
      provider: "openai",
      display_name: "OpenAI API",
      windows: [],
      balance: null,
      spend: { amount: "1.25", currency: "USD" },
      source: "official",
      fetched_at: now,
      error: null,
      notes: ["サンプル: 残高 API はないので支出のみ"],
    },
    {
      provider: "anthropic",
      display_name: "Anthropic API",
      windows: [],
      balance: null,
      spend: null,
      source: "official",
      fetched_at: now,
      error: "認証情報 anthropic_admin_key が資格情報マネージャーにありません",
      notes: [],
    },
  ];
}

function sampleSettings(): Settings {
  return {
    general: {
      refresh_secs: 600,
      notify_used_pct: 80,
      notify_balance_below: null,
      notify_balance_currency: "USD",
    },
    providers: {
      deepseek: { enabled: true },
      xai: { enabled: true, team_id: "" },
      openai: { enabled: true },
      anthropic: { enabled: true },
      claude_code: {
        enabled: true,
        cache_path: null,
        projects_dir: null,
        aggregate_tokens: true,
      },
      codex: {
        enabled: true,
        sessions_dir: null,
        allow_unofficial_wham: false,
        try_app_server: false,
      },
      opencode: { enabled: true, data_dir: null, allow_unofficial_usage_api: false },
      antigravity: { enabled: false, cache_path: null },
    },
  };
}

function sampleSecrets(): SecretInfo[] {
  return [
    { name: "deepseek_api_key", label: "DeepSeek API キー", configured: false, error: null },
    { name: "xai_management_key", label: "xAI Management キー", configured: false, error: null },
  ];
}

void loadCached();
if (inTauri) {
  void listen("snapshots-updated", () => {
    void invoke<Snapshot[]>("snapshots").then((next) => {
      snapshots = next;
      if (tab === "usage") render();
    });
  });
}
window.setInterval(() => {
  if (tab === "usage") render();
}, 30000);
