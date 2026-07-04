import {
  AlertCircle,
  Bot,
  CheckCircle2,
  ChevronRight,
  Clipboard,
  FileAudio,
  FileText,
  Home,
  Headphones,
  Keyboard,
  Mic,
  Plus,
  RefreshCw,
  Save,
  Settings,
  SlidersHorizontal,
  TestTube2,
  Trash2,
  Wand2,
  X,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { convertFileSrc } from "@tauri-apps/api/core";
import { call } from "./tauri";
import type {
  AppSettings,
  BootstrapData,
  PromptSettings,
  RecordPage,
  RecordingSession,
  SpeechRecord,
} from "./tauri";

type Tab = "home" | "models" | "preferences" | "settings";

const statusLabel: Record<string, string> = {
  idle: "待命",
  recording: "正在录音",
  processing: "整理中",
  pending: "等待处理",
  saving: "保存录音",
  transcribing: "文字转写中",
  optimizing: "内容优化中",
  completed: "完成",
  mocked: "本地模拟",
  failed: "失败",
  blocked: "等待转写",
  no_speech: "没有录音",
};

const pageSize = 60;

export function App() {
  const [activeTab, setActiveTab] = useState<Tab>("home");
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [prompts, setPrompts] = useState<PromptSettings | null>(null);
  const [records, setRecords] = useState<SpeechRecord[]>([]);
  const [hasMoreRecords, setHasMoreRecords] = useState(false);
  const [recordsLoading, setRecordsLoading] = useState(true);
  const [selectedRecordId, setSelectedRecordId] = useState<string | null>(null);
  const [deleteRecordId, setDeleteRecordId] = useState<string | null>(null);
  const [recording, setRecording] = useState<RecordingSession>({
    active: false,
    started_at: null,
    status: "idle",
    elapsed_ms: 0,
  });
  const [notice, setNotice] = useState("正在读取本地配置");

  useEffect(() => {
    call<BootstrapData>("get_bootstrap")
      .then((data) => {
        setSettings(data.settings);
        applyTheme(data.settings.theme);
        setPrompts(data.prompts);
        setRecords(data.records);
        setHasMoreRecords(data.records.length >= pageSize);
        setRecording(data.recording);
        setNotice("准备就绪");
        setRecordsLoading(false);
      })
      .catch((error) => {
        setNotice(`读取配置失败：${String(error)}`);
        setRecordsLoading(false);
      });
  }, []);

  useEffect(() => {
    const disposers = Promise.all([
      listen<string>("global-shortcut", () => {
        toggleRecording();
      }),
      listen<RecordingSession>("recording-state", (event) => {
        setRecording(event.payload);
      }),
      listen<SpeechRecord>("record-updated", (event) => {
        mergeRecord(event.payload);
      }),
      listen<string>("shortcut-error", (event) => {
        setNotice(event.payload);
      }),
    ]);

    return () => {
      disposers.then((items) => items.forEach((dispose) => dispose()));
    };
  });

  useEffect(() => {
    if (!recording.active || !recording.started_at) return;

    const startedAt = new Date(recording.started_at).getTime();
    const timer = window.setInterval(() => {
      setRecording((current) => ({
        ...current,
        elapsed_ms: Date.now() - startedAt,
      }));
    }, 250);

    return () => window.clearInterval(timer);
  }, [recording.active, recording.started_at]);

  const selectedTitle = useMemo(() => {
    if (activeTab === "models") return "模型配置";
    if (activeTab === "preferences") return "Preference";
    if (activeTab === "settings") return "设置";
    return "首页";
  }, [activeTab]);
  const selectedRecord = useMemo(
    () => records.find((record) => record.id === selectedRecordId) ?? null,
    [records, selectedRecordId],
  );
  const deleteTarget = useMemo(
    () => records.find((record) => record.id === deleteRecordId) ?? null,
    [records, deleteRecordId],
  );
  const stats = useMemo(() => buildStats(records), [records]);

  async function toggleRecording() {
    if (!recording.active) {
      const next = await call<RecordingSession>("start_recording");
      setRecording(next);
      setNotice("录音已开始");
      return;
    }

    const record = await call<SpeechRecord>("stop_recording");
    setRecording({ active: false, started_at: null, status: "idle", elapsed_ms: 0 });
    mergeRecord(record);
    setNotice(record.error_message ?? "录音处理完成");
  }

  async function saveSettings(next: AppSettings) {
    const saved = await call<AppSettings>("save_settings", { settings: next });
    setSettings(saved);
    applyTheme(saved.theme);
    setNotice("设置已保存");
  }

  async function savePrompts(next: PromptSettings) {
    const saved = await call<PromptSettings>("save_prompt_settings", { prompts: next });
    setPrompts(saved);
    setNotice("Preference 已保存");
  }

  async function copyRecord(record: SpeechRecord) {
    await call<boolean>("copy_text", { text: record.final_text });
    setNotice("已复制到剪贴板");
  }

  async function deleteRecord(id: string) {
    const next = await call<SpeechRecord[]>("delete_record", { id });
    setRecords(next);
    setNotice("记录已删除");
  }

  async function confirmDeleteRecord() {
    if (!deleteRecordId) return;
    await deleteRecord(deleteRecordId);
    if (selectedRecordId === deleteRecordId) setSelectedRecordId(null);
    setDeleteRecordId(null);
  }

  async function retryAsr(record: SpeechRecord) {
    setNotice("文字转写中");
    const next = await call<SpeechRecord>("retry_asr", { id: record.id });
    mergeRecord(next);
    setNotice(next.error_message ?? "文字转写完成");
  }

  async function retryOptimize(record: SpeechRecord) {
    setNotice("内容优化中");
    const next = await call<SpeechRecord>("retry_optimize", { id: record.id });
    mergeRecord(next);
    setNotice(next.error_message ?? "内容优化完成");
  }

  async function loadMoreRecords() {
    if (recordsLoading || !hasMoreRecords) return;
    setRecordsLoading(true);
    const page = await call<RecordPage>("list_records_page", {
      offset: records.length,
      limit: pageSize,
    });
    setRecords((current) => [...current, ...page.records]);
    setHasMoreRecords(page.has_more);
    setRecordsLoading(false);
  }

  function mergeRecord(record: SpeechRecord) {
    setRecords((current) => {
      const index = current.findIndex((item) => item.id === record.id);
      if (index === -1) return [record, ...current];
      const next = [...current];
      next[index] = record;
      return next;
    });
  }

  function handleWorkspaceScroll(event: React.UIEvent<HTMLElement>) {
    if (activeTab !== "home") return;
    const target = event.currentTarget;
    const threshold = 360;
    if (target.scrollTop + target.clientHeight >= target.scrollHeight - threshold) {
      loadMoreRecords();
    }
  }

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <img className="brand-mark" src="/logo.svg" alt="" />
          <div>
            <strong>SparkSpeech</strong>
            <span>personal dictation</span>
          </div>
        </div>

        <nav className="nav-list" aria-label="主导航">
          <button className={activeTab === "home" ? "active" : ""} onClick={() => setActiveTab("home")}>
            <Home size={18} />
            首页
          </button>
          <button className={activeTab === "models" ? "active" : ""} onClick={() => setActiveTab("models")}>
            <SlidersHorizontal size={18} />
            模型
          </button>
          <button
            className={activeTab === "preferences" ? "active" : ""}
            onClick={() => setActiveTab("preferences")}
          >
            <FileText size={18} />
            Preference
          </button>
          <button className={activeTab === "settings" ? "active" : ""} onClick={() => setActiveTab("settings")}>
            <Settings size={18} />
            设置
          </button>
        </nav>

        <div className="shortcut-panel">
          <div className="shortcut-row">
            <Keyboard size={17} />
            <span>快捷键</span>
            <strong>{shortcutLabel(settings?.global_shortcut ?? "RightAlt")}</strong>
          </div>
          <div className="shortcut-row">
            <CheckCircle2 size={17} />
            <span>录音服务</span>
            <strong>{recording.active ? statusLabel[recording.status] ?? recording.status : "可用"}</strong>
          </div>
          <div className="shortcut-row">
            <Clipboard size={17} />
            <span>自动粘贴</span>
            <strong>{settings?.auto_paste ? "开启" : "关闭"}</strong>
          </div>
        </div>
      </aside>

      <main className="workspace" onScroll={handleWorkspaceScroll}>
        <header className="topbar">
          <div>
            <p>{selectedTitle}</p>
            <h1>{activeTab === "home" ? "语音输入历史" : selectedTitle}</h1>
          </div>
          <div className="topbar-actions">
            <span className="status-dot">{notice}</span>
            <button className="primary-button" onClick={toggleRecording}>
              <Mic size={18} />
              {recording.active ? "结束录音" : "开始录音"}
            </button>
          </div>
        </header>

        <div className="page-panel" key={activeTab}>
          {activeTab === "home" && (
            <HomeView
              hasMore={hasMoreRecords}
              loading={recordsLoading}
              records={records}
              stats={stats}
              onCopy={copyRecord}
              onDelete={(id) => setDeleteRecordId(id)}
              onLoadMore={loadMoreRecords}
              onOpenDetails={(record) => setSelectedRecordId(record.id)}
              onRetryAsr={retryAsr}
              onRetryOptimize={retryOptimize}
            />
          )}

          {activeTab === "models" && settings && (
            <ModelSettings settings={settings} onSave={saveSettings} />
          )}

          {activeTab === "preferences" && prompts && (
            <PreferenceSettings prompts={prompts} onSave={savePrompts} />
          )}

          {activeTab === "settings" && settings && (
            <AppSettingsView settings={settings} onSave={saveSettings} />
          )}
        </div>
      </main>

      {selectedRecord && (
        <RecordDetailsModal
          record={selectedRecord}
          onClose={() => setSelectedRecordId(null)}
          onCopy={copyRecord}
          onDelete={(id) => setDeleteRecordId(id)}
          onRetryAsr={retryAsr}
          onRetryOptimize={retryOptimize}
        />
      )}
      {deleteTarget && (
        <ConfirmDeleteModal
          record={deleteTarget}
          onCancel={() => setDeleteRecordId(null)}
          onConfirm={confirmDeleteRecord}
        />
      )}
    </div>
  );
}

function HomeView({
  hasMore,
  loading,
  records,
  stats,
  onCopy,
  onDelete,
  onLoadMore,
  onOpenDetails,
  onRetryAsr,
  onRetryOptimize,
}: {
  hasMore: boolean;
  loading: boolean;
  records: SpeechRecord[];
  stats: HomeStats;
  onCopy: (record: SpeechRecord) => Promise<void>;
  onDelete: (id: string) => void | Promise<void>;
  onLoadMore: () => Promise<void>;
  onOpenDetails: (record: SpeechRecord) => void;
  onRetryAsr: (record: SpeechRecord) => Promise<void>;
  onRetryOptimize: (record: SpeechRecord) => Promise<void>;
}) {
  if (loading && records.length === 0) {
    return <RecordSkeleton />;
  }

  if (records.length === 0) {
    return (
      <>
        <StatsStrip stats={stats} />
        <section className="empty-state">
          <Mic size={30} />
          <h2>还没有语音记录</h2>
          <p>按右 Alt 开始录音。</p>
        </section>
      </>
    );
  }

  return (
    <>
      <StatsStrip stats={stats} />
      <section className="record-list" aria-label="识别历史">
        {records.map((record) => (
          <article className="record-item compact" key={record.id} onClick={() => onOpenDetails(record)}>
            <div className="record-main">
              <p className="record-text">{record.final_text || record.raw_asr_text || "没有录音"}</p>
              <div className="record-meta-line">
                <span>{formatDate(record.created_at)}</span>
                <span>{statusLabel[record.asr_status] ?? record.asr_status}</span>
                {record.duration_ms && <span>{formatDuration(record.duration_ms)}</span>}
                {record.error_message && <span className="record-error">{record.error_message}</span>}
              </div>
            </div>
            <div className="record-actions" onClick={(event) => event.stopPropagation()}>
              <IconButton label="复制" onClick={() => onCopy(record)}>
                <Clipboard size={16} />
              </IconButton>
              <IconButton label="重新转写" disabled={!record.audio_path} onClick={() => onRetryAsr(record)}>
                <FileAudio size={16} />
              </IconButton>
              <IconButton label="重新优化" disabled={!record.raw_asr_text} onClick={() => onRetryOptimize(record)}>
                <RefreshCw size={16} />
              </IconButton>
              <IconButton label="删除" onClick={() => onDelete(record.id)}>
                <Trash2 size={16} />
              </IconButton>
              <IconButton label="查看详情" onClick={() => onOpenDetails(record)}>
                <ChevronRight size={16} />
              </IconButton>
            </div>
          </article>
        ))}
        {loading && <RecordSkeleton />}
        {hasMore && (
          <button className="load-more-button" onClick={onLoadMore}>
            加载更多
          </button>
        )}
      </section>
    </>
  );
}

type HomeStats = {
  totalHours: number;
  totalChars: number;
  charsPerMinute: number;
};

function StatsStrip({ stats }: { stats: HomeStats }) {
  return (
    <section className="stats-strip" aria-label="语音输入统计">
      <div>
        <span>累计说话</span>
        <strong>{formatHours(stats.totalHours)}</strong>
      </div>
      <div>
        <span>累计文字</span>
        <strong>{stats.totalChars.toLocaleString("zh-CN")} 字</strong>
      </div>
      <div>
        <span>平均速度</span>
        <strong>{stats.charsPerMinute.toLocaleString("zh-CN")} 字/分</strong>
      </div>
    </section>
  );
}

function ConfirmDeleteModal({
  onCancel,
  onConfirm,
  record,
}: {
  onCancel: () => void;
  onConfirm: () => Promise<void>;
  record: SpeechRecord;
}) {
  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={onCancel}>
      <section className="modal-panel confirm-modal" role="dialog" aria-modal="true" onMouseDown={(event) => event.stopPropagation()}>
        <header className="modal-header">
          <div>
            <p>{formatDate(record.created_at)}</p>
            <h2>删除这条记录？</h2>
          </div>
          <IconButton label="关闭" onClick={onCancel}>
            <X size={17} />
          </IconButton>
        </header>
        <p className="confirm-copy">删除后会从历史记录中移除。录音文件如果还在本地，也会失去这条记录入口。</p>
        <div className="confirm-actions">
          <button className="secondary-button" type="button" onClick={onCancel}>
            取消
          </button>
          <button className="danger-button" type="button" onClick={onConfirm}>
            删除
          </button>
        </div>
      </section>
    </div>
  );
}

function RecordDetailsModal({
  record,
  onClose,
  onCopy,
  onDelete,
  onRetryAsr,
  onRetryOptimize,
}: {
  record: SpeechRecord;
  onClose: () => void;
  onCopy: (record: SpeechRecord) => Promise<void>;
  onDelete: (id: string) => void | Promise<void>;
  onRetryAsr: (record: SpeechRecord) => Promise<void>;
  onRetryOptimize: (record: SpeechRecord) => Promise<void>;
}) {
  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={onClose}>
      <section className="modal-panel record-modal" role="dialog" aria-modal="true" onMouseDown={(event) => event.stopPropagation()}>
        <header className="modal-header">
          <div>
            <p>{formatDate(record.created_at)}</p>
            <h2>转写详情</h2>
          </div>
          <div className="modal-actions">
            <IconButton label="复制优化文本" onClick={() => onCopy(record)}>
              <Clipboard size={17} />
            </IconButton>
            <IconButton label="重新转写" disabled={!record.audio_path} onClick={() => onRetryAsr(record)}>
              <FileAudio size={17} />
            </IconButton>
            <IconButton label="重新优化" disabled={!record.raw_asr_text} onClick={() => onRetryOptimize(record)}>
              <RefreshCw size={17} />
            </IconButton>
            <IconButton label="删除" onClick={() => onDelete(record.id)}>
              <Trash2 size={17} />
            </IconButton>
            <IconButton label="关闭" onClick={onClose}>
              <X size={17} />
            </IconButton>
          </div>
        </header>

        <div className="detail-status">
          <span>ASR：{statusLabel[record.asr_status] ?? record.asr_status}</span>
          <span>优化：{statusLabel[record.optimize_status] ?? record.optimize_status}</span>
          {record.duration_ms && <span>时长：{formatDuration(record.duration_ms)}</span>}
          {record.copied_at && <span>已复制 {formatDate(record.copied_at)}</span>}
          {record.pasted_at && <span>已粘贴 {formatDate(record.pasted_at)}</span>}
        </div>

        {record.error_message && (
          <div className="inline-alert">
            <AlertCircle size={16} />
            {record.error_message}
          </div>
        )}

        <div className="detail-grid">
          <TextBlock title="原始转录文字" text={record.raw_asr_text || "暂无 ASR 文本"} onCopy={() => call<boolean>("copy_text", { text: record.raw_asr_text })} />
          <TextBlock title="优化后的文字" text={record.final_text || "暂无优化文本"} onCopy={() => call<boolean>("copy_text", { text: record.final_text })} />
        </div>
      </section>
    </div>
  );
}

function TextBlock({ title, text, onCopy }: { title: string; text: string; onCopy: () => Promise<unknown> }) {
  return (
    <section className="text-block">
      <header>
        <h3>{title}</h3>
        <IconButton label="复制这一段" onClick={onCopy}>
          <Clipboard size={15} />
        </IconButton>
      </header>
      <p>{text}</p>
    </section>
  );
}

function RecordSkeleton() {
  return (
    <div className="skeleton-list" aria-label="正在加载">
      {Array.from({ length: 4 }).map((_, index) => (
        <div className="skeleton-card" key={index}>
          <span />
          <span />
          <span />
        </div>
      ))}
    </div>
  );
}

function ModelSettings({
  settings,
  onSave,
}: {
  settings: AppSettings;
  onSave: (settings: AppSettings) => Promise<void>;
}) {
  const [draft, setDraft] = useState(settings);
  const [editingProvider, setEditingProvider] = useState<"doubao" | "openrouter" | null>(null);
  const [testStatus, setTestStatus] = useState("");

  async function testDoubao() {
    setTestStatus("正在检查豆包配置");
    try {
      const message = await call<string>("test_doubao_config", { settings: draft });
      setTestStatus(message);
    } catch (error) {
      setTestStatus(String(error));
    }
  }

  async function testOpenRouter() {
    setTestStatus("正在测试 OpenRouter");
    try {
      const message = await call<string>("test_openrouter", { settings: draft });
      setTestStatus(message);
    } catch (error) {
      setTestStatus(String(error));
    }
  }

  return (
    <section className="settings-page provider-page">
      <div className="provider-grid">
        <ProviderCard
          icon={<Headphones size={20} />}
          title="豆包流式 ASR"
          eyebrow="语音识别 Provider"
          description="只负责把录音转成原始文字，不走系统代理。"
          status={draft.doubao_auth_mode === "app_access_key" ? "App Key 鉴权" : "API Key 鉴权"}
          onEdit={() => setEditingProvider("doubao")}
          onTest={testDoubao}
        />
        <ProviderCard
          icon={<Bot size={20} />}
          title="OpenRouter"
          eyebrow="文本优化 Provider"
          description="OpenAI compatible chat completions，默认走系统代理。"
          status={draft.openrouter_model || "未设置模型"}
          onEdit={() => setEditingProvider("openrouter")}
          onTest={testOpenRouter}
        />
        <button className="provider-card add-provider" type="button" disabled>
          <Plus size={20} />
          <span>新增 Provider</span>
          <small>后续扩展多 provider 时启用</small>
        </button>
      </div>

      {testStatus && <div className="inline-alert neutral"><CheckCircle2 size={16} />{testStatus}</div>}

      <button className="primary-button save-button" onClick={() => onSave(draft)}>
        <Save size={18} />
        保存模型配置
      </button>

      {editingProvider === "doubao" && (
        <ProviderModal title="豆包流式 ASR" onClose={() => setEditingProvider(null)} onTest={testDoubao}>
          <label className="text-field">
            <span>鉴权方式</span>
            <select
              value={draft.doubao_auth_mode}
              onChange={(event) => setDraft({ ...draft, doubao_auth_mode: event.currentTarget.value })}
            >
              <option value="api_key">新版控制台 API Key</option>
              <option value="app_access_key">旧版 App Key + Access Key</option>
            </select>
          </label>
          <TextField label="API Key" value={draft.doubao_api_key} type="password" onChange={(doubao_api_key) => setDraft({ ...draft, doubao_api_key })} />
          <TextField label="App Key" value={draft.doubao_app_key} type="password" onChange={(doubao_app_key) => setDraft({ ...draft, doubao_app_key })} />
          <TextField label="Access Key" value={draft.doubao_access_key} type="password" onChange={(doubao_access_key) => setDraft({ ...draft, doubao_access_key })} />
          <TextField label="Resource ID" value={draft.doubao_resource_id} onChange={(doubao_resource_id) => setDraft({ ...draft, doubao_resource_id })} />
          <TextField label="Endpoint" value={draft.doubao_endpoint} onChange={(doubao_endpoint) => setDraft({ ...draft, doubao_endpoint })} />
          <TextField label="语言" value={draft.doubao_language} onChange={(doubao_language) => setDraft({ ...draft, doubao_language })} />
        </ProviderModal>
      )}

      {editingProvider === "openrouter" && (
        <ProviderModal title="OpenRouter" onClose={() => setEditingProvider(null)} onTest={testOpenRouter}>
          <TextField label="API Key" value={draft.openrouter_api_key} type="password" onChange={(openrouter_api_key) => setDraft({ ...draft, openrouter_api_key })} />
          <TextField label="Base URL" value={draft.openrouter_base_url} onChange={(openrouter_base_url) => setDraft({ ...draft, openrouter_base_url })} />
          <TextField label="Model" value={draft.openrouter_model} onChange={(openrouter_model) => setDraft({ ...draft, openrouter_model })} />
          <TextField label="HTTP-Referer" value={draft.openrouter_http_referer} onChange={(openrouter_http_referer) => setDraft({ ...draft, openrouter_http_referer })} />
          <TextField label="X-OpenRouter-Title" value={draft.openrouter_title} onChange={(openrouter_title) => setDraft({ ...draft, openrouter_title })} />
          <label className="toggle-field">
            <input
              checked={draft.use_system_proxy_for_openrouter}
              type="checkbox"
              onChange={(event) => setDraft({ ...draft, use_system_proxy_for_openrouter: event.currentTarget.checked })}
            />
            OpenRouter 走系统代理
          </label>
        </ProviderModal>
      )}
    </section>
  );
}

function ProviderCard({
  description,
  eyebrow,
  icon,
  onEdit,
  onTest,
  status,
  title,
}: {
  description: string;
  eyebrow: string;
  icon: ReactNode;
  onEdit: () => void;
  onTest: () => void | Promise<void>;
  status: string;
  title: string;
}) {
  return (
    <article className="provider-card">
      <div className="provider-icon">{icon}</div>
      <div>
        <small>{eyebrow}</small>
        <h2>{title}</h2>
        <p>{description}</p>
        <span>{status}</span>
      </div>
      <div className="provider-actions">
        <IconButton label="测试" onClick={onTest}>
          <TestTube2 size={16} />
        </IconButton>
        <IconButton label="编辑" onClick={onEdit}>
          <ChevronRight size={16} />
        </IconButton>
      </div>
    </article>
  );
}

function ProviderModal({
  children,
  onClose,
  onTest,
  title,
}: {
  children: ReactNode;
  onClose: () => void;
  onTest: () => void | Promise<void>;
  title: string;
}) {
  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={onClose}>
      <section className="modal-panel provider-modal" role="dialog" aria-modal="true" onMouseDown={(event) => event.stopPropagation()}>
        <header className="modal-header">
          <div>
            <p>Provider</p>
            <h2>{title}</h2>
          </div>
          <div className="modal-actions">
            <IconButton label="测试 Provider" onClick={onTest}>
              <TestTube2 size={17} />
            </IconButton>
            <IconButton label="关闭" onClick={onClose}>
              <X size={17} />
            </IconButton>
          </div>
        </header>
        <div className="field-grid">{children}</div>
      </section>
    </div>
  );
}

function AppSettingsView({
  settings,
  onSave,
}: {
  settings: AppSettings;
  onSave: (settings: AppSettings) => Promise<void>;
}) {
  const [draft, setDraft] = useState(settings);
  const [microphones, setMicrophones] = useState<string[]>([]);
  const [logs, setLogs] = useState("");
  const [capturingShortcut, setCapturingShortcut] = useState(false);
  const [micTestStatus, setMicTestStatus] = useState("");
  const [micSampleSrc, setMicSampleSrc] = useState("");

  useEffect(() => {
    call<string[]>("list_microphones")
      .then(setMicrophones)
      .catch(() => setMicrophones([]));
  }, []);

  useEffect(() => {
    if (!capturingShortcut) return;

    function handleKeyDown(event: KeyboardEvent) {
      event.preventDefault();
      event.stopPropagation();
      setDraft((current) => ({ ...current, global_shortcut: normalizeShortcutCode(event.code) }));
      setCapturingShortcut(false);
    }

    window.addEventListener("keydown", handleKeyDown, true);
    return () => window.removeEventListener("keydown", handleKeyDown, true);
  }, [capturingShortcut]);

  async function loadLogs() {
    const content = await call<string>("read_logs");
    setLogs(content || "暂无日志。");
  }

  async function recordMicrophoneSample() {
    setMicTestStatus("正在录制试听片段");
    try {
      const path = await call<string>("record_microphone_sample", { microphoneName: draft.microphone_name });
      setMicSampleSrc(convertFileSrc(path));
      setMicTestStatus("试听片段已录好");
    } catch (error) {
      setMicTestStatus(String(error));
      setMicSampleSrc("");
    }
  }

  return (
    <section className="settings-page">
      <div className="settings-section">
        <div className="section-heading">
          <h2>录音</h2>
          <p>选择麦克风和全局快捷键。</p>
        </div>
        <div className="field-grid">
          <label className="text-field">
            <span>麦克风</span>
            <select
              value={draft.microphone_name}
              onChange={(event) => setDraft({ ...draft, microphone_name: event.currentTarget.value })}
            >
              <option value="">系统默认麦克风</option>
              {microphones.map((name) => (
                <option value={name} key={name}>
                  {name}
                </option>
              ))}
            </select>
          </label>
          <label className="text-field">
            <span>麦克风测试</span>
            <button className="capture-button" type="button" onClick={recordMicrophoneSample}>
              <TestTube2 size={16} />
              录一段试听
            </button>
          </label>
          <label className="text-field">
            <span>全局快捷键</span>
            <button
              className={capturingShortcut ? "capture-button capturing" : "capture-button"}
              type="button"
              onClick={() => setCapturingShortcut(true)}
            >
              {capturingShortcut ? "请按下一个键" : shortcutLabel(draft.global_shortcut)}
            </button>
          </label>
          <TextField
            label="录音保留天数"
            value={String(draft.recording_retention_days)}
            type="number"
            onChange={(value) => setDraft({ ...draft, recording_retention_days: Number(value) || 1 })}
          />
          <label className="toggle-field">
            <input
              checked={draft.auto_paste}
              type="checkbox"
              onChange={(event) => setDraft({ ...draft, auto_paste: event.currentTarget.checked })}
            />
            整理成功后自动粘贴
          </label>
        </div>
        {micTestStatus && <div className="inline-alert neutral"><CheckCircle2 size={16} />{micTestStatus}</div>}
        {micSampleSrc && <audio className="audio-preview" controls src={micSampleSrc} />}
      </div>

      <div className="settings-section">
        <div className="section-heading">
          <h2>外观</h2>
          <p>选择界面主题。</p>
        </div>
        <div className="field-grid">
          <label className="text-field">
            <span>主题</span>
            <select
              value={draft.theme}
              onChange={(event) => setDraft({ ...draft, theme: event.currentTarget.value })}
            >
              <option value="system">跟随系统</option>
              <option value="light">浅色</option>
              <option value="dark">深色</option>
            </select>
          </label>
        </div>
      </div>

      <div className="settings-section">
        <div className="section-heading">
          <h2>日志</h2>
          <p>保存和查看本地运行日志。</p>
        </div>
        <label className="toggle-field">
          <input
            checked={draft.save_logs}
            type="checkbox"
            onChange={(event) => setDraft({ ...draft, save_logs: event.currentTarget.checked })}
          />
          保存日志
        </label>
        <div className="button-row">
          <button className="secondary-button" onClick={loadLogs}>
            查看日志
          </button>
        </div>
        {logs && <pre className="log-viewer">{logs}</pre>}
      </div>

      <button className="primary-button save-button" onClick={() => onSave(draft)}>
        <Save size={18} />
        保存设置
      </button>
    </section>
  );
}

function PreferenceSettings({
  prompts,
  onSave,
}: {
  prompts: PromptSettings;
  onSave: (prompts: PromptSettings) => Promise<void>;
}) {
  const [draft, setDraft] = useState(prompts);

  return (
    <section className="settings-page preference-page">
      <div className="settings-section">
        <div className="section-heading">
          <h2>系统提示词</h2>
          <p>决定文本整理器的任务边界。</p>
        </div>
        <textarea
          value={draft.system_prompt}
          onChange={(event) => setDraft({ ...draft, system_prompt: event.currentTarget.value })}
        />
      </div>

      <div className="settings-section">
        <div className="section-heading">
          <h2>个性化偏好</h2>
          <p>控制分段、标点、空格、公式和表达习惯。</p>
        </div>
        <textarea
          value={draft.writing_preferences}
          onChange={(event) => setDraft({ ...draft, writing_preferences: event.currentTarget.value })}
        />
      </div>

      <div className="settings-section">
        <div className="section-heading">
          <h2>词条替换</h2>
          <p>一行一个词，或使用 A -&gt; B 表达明确替换。</p>
        </div>
        <textarea
          className="dictionary-box"
          value={draft.replacements}
          onChange={(event) => setDraft({ ...draft, replacements: event.currentTarget.value })}
        />
      </div>

      <button className="primary-button save-button" onClick={() => onSave(draft)}>
        <Wand2 size={18} />
        保存 Preference
      </button>
    </section>
  );
}

function TextField({
  label,
  value,
  onChange,
  type = "text",
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  type?: string;
}) {
  return (
    <label className="text-field">
      <span>{label}</span>
      <input value={value} type={type} onChange={(event) => onChange(event.currentTarget.value)} />
    </label>
  );
}

function IconButton({
  children,
  disabled,
  label,
  onClick,
}: {
  children: ReactNode;
  disabled?: boolean;
  label: string;
  onClick: () => void | Promise<unknown>;
}) {
  return (
    <button
      aria-label={label}
      className="icon-button"
      data-tooltip={label}
      disabled={disabled}
      type="button"
      onClick={onClick}
    >
      {children}
    </button>
  );
}

function buildStats(records: SpeechRecord[]): HomeStats {
  const totalMs = records.reduce((sum, record) => sum + (record.duration_ms ?? 0), 0);
  const totalChars = records.reduce((sum, record) => sum + countTextChars(record.final_text || record.raw_asr_text), 0);
  const minutes = totalMs / 60_000;
  return {
    totalHours: totalMs / 3_600_000,
    totalChars,
    charsPerMinute: minutes > 0 ? Math.round(totalChars / minutes) : 0,
  };
}

function countTextChars(text: string) {
  return Array.from(text.replace(/\s/g, "")).length;
}

function formatHours(hours: number) {
  if (hours < 1) {
    return `${Math.round(hours * 60)} 分钟`;
  }
  return `${Math.floor(hours)} 时 ${Math.round((hours % 1) * 60)} 分`;
}

function formatDuration(ms: number) {
  const totalSeconds = Math.max(1, Math.round(ms / 1000));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  if (minutes === 0) return `${seconds} 秒`;
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}

function formatDate(value: string) {
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value));
}

function applyTheme(theme: string) {
  document.documentElement.dataset.theme = theme;
}

function normalizeShortcutCode(code: string) {
  if (code === "AltRight") return "RightAlt";
  if (code === "AltLeft") return "LeftAlt";
  if (code === "ControlRight") return "RightControl";
  if (code === "ControlLeft") return "LeftControl";
  if (code === "ShiftRight") return "RightShift";
  if (code === "ShiftLeft") return "LeftShift";
  return code;
}

function shortcutLabel(value: string) {
  const labels: Record<string, string> = {
    RightAlt: "右 Alt",
    LeftAlt: "左 Alt",
    RightControl: "右 Ctrl",
    LeftControl: "左 Ctrl",
    RightShift: "右 Shift",
    LeftShift: "左 Shift",
    Space: "Space",
    Enter: "Enter",
    Escape: "Esc",
    CapsLock: "Caps Lock",
  };
  return labels[value] ?? value;
}
