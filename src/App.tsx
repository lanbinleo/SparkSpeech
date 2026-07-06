import {
  AlertCircle,
  Bot,
  CheckCircle2,
  ChevronRight,
  Clipboard,
  FileAudio,
  FileText,
  FolderOpen,
  Home,
  Headphones,
  Keyboard,
  Mic,
  Monitor,
  Moon,
  Plus,
  Play,
  RefreshCw,
  Save,
  Settings,
  SlidersHorizontal,
  Sun,
  TestTube2,
  Trash2,
  Wand2,
  X,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
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
type TextProviderKey = "openrouter" | "deepseek" | "custom_openai";
type Toast = {
  id: number;
  message: string;
  tone?: "info" | "error" | "success";
};
type AutoSaveStatus = "idle" | "saving" | "saved" | "error";

const statusLabel: Record<string, string> = {
  idle: "待命",
  recording: "正在录音",
  processing: "整理中",
  pending: "等待处理",
  transcribing: "文字转写中",
  optimizing: "内容优化中",
  completed: "完成",
  mocked: "本地模拟",
  failed: "失败",
  blocked: "无法继续",
  recovered: "已恢复录音",
  no_speech: "没有录音",
};

const audioStatusLabel: Record<string, string> = {
  pending: "正在保存",
  saved: "已保存",
  save_failed: "保存失败",
  expired: "已过期",
  missing: "文件缺失",
};

const pageSize = 60;

export function App() {
  const [activeTab, setActiveTab] = useState<Tab>("home");
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [settingsDraft, setSettingsDraft] = useState<AppSettings | null>(null);
  const [promptsDraft, setPromptsDraft] = useState<PromptSettings | null>(null);
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
  const [draggingFile, setDraggingFile] = useState(false);
  const [importingAudio, setImportingAudio] = useState(false);
  const importingAudioRef = useRef(false);
  const recordingRef = useRef(recording);
  const settingsRef = useRef<AppSettings | null>(settings);
  const shortcutHandledAtRef = useRef(0);
  const [toasts, setToasts] = useState<Toast[]>([]);

  useEffect(() => {
    recordingRef.current = recording;
  }, [recording]);

  useEffect(() => {
    settingsRef.current = settings;
  }, [settings]);

  useEffect(() => {
    call<BootstrapData>("get_bootstrap")
      .then((data) => {
        setSettings(data.settings);
        setSettingsDraft(data.settings);
        applyTheme(data.settings.theme);
        setPromptsDraft(data.prompts);
        setRecords(data.records);
        setHasMoreRecords(data.records.length >= pageSize);
        setRecording(data.recording);
        setRecordsLoading(false);
      })
      .catch((error) => {
        showToast(`读取配置失败：${String(error)}`, "error");
        setRecordsLoading(false);
      });
  }, []);

  useEffect(() => {
    function triggerShortcut() {
      const now = Date.now();
      if (now - shortcutHandledAtRef.current < 300) return;
      shortcutHandledAtRef.current = now;
      toggleRecording();
    }

    function handleFocusedKeyDown(event: KeyboardEvent) {
      if (event.repeat) return;
      const activeShortcut = settingsRef.current?.global_shortcut ?? "RightAlt";
      if (normalizeShortcutCode(event.code) !== activeShortcut) return;
      event.preventDefault();
      event.stopPropagation();
      triggerShortcut();
    }

    window.addEventListener("keydown", handleFocusedKeyDown);
    const disposers = Promise.all([
      listen<string>("global-shortcut", () => {
        triggerShortcut();
      }),
      listen<RecordingSession>("recording-state", (event) => {
        recordingRef.current = event.payload;
        setRecording(event.payload);
      }),
      listen<SpeechRecord>("record-updated", (event) => {
        mergeRecord(event.payload);
      }),
      listen<string>("shortcut-error", (event) => {
        showToast(event.payload, "error");
      }),
    ]);

    return () => {
      window.removeEventListener("keydown", handleFocusedKeyDown);
      disposers.then((items) => items.forEach((dispose) => dispose()));
    };
  }, []);

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

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type === "enter" || event.payload.type === "over") {
          setDraggingFile(true);
          return;
        }
        if (event.payload.type === "leave") {
          setDraggingFile(false);
          return;
        }
        setDraggingFile(false);
        const [path] = event.payload.paths;
        if (path) importAudioFile(path);
      })
      .then((dispose) => {
        if (disposed) {
          dispose();
        } else {
          unlisten = dispose;
        }
      })
      .catch((error) => showToast(`拖拽监听不可用：${String(error)}`, "error"));

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const selectedTitle = useMemo(() => {
    if (activeTab === "models") return "模型配置";
    if (activeTab === "preferences") return "偏好";
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

  function showToast(message: string, tone: Toast["tone"] = "info") {
    const id = Date.now() + Math.random();
    setToasts((current) => [...current, { id, message, tone }]);
    window.setTimeout(() => {
      setToasts((current) => current.filter((toast) => toast.id !== id));
    }, 3200);
  }

  async function toggleRecording() {
    const currentRecording = recordingRef.current;
    if (!currentRecording.active) {
      const next = await call<RecordingSession>("start_recording");
      setRecording(next);
      recordingRef.current = next;
      showToast("录音已开始", "success");
      return;
    }

    if (currentRecording.status !== "recording") {
      showToast("上一段录音还在处理，暂不支持同时录制", "info");
      return;
    }

    const record = await call<SpeechRecord>("stop_recording");
    const idleRecording: RecordingSession = { active: false, started_at: null, status: "idle", elapsed_ms: 0 };
    setRecording(idleRecording);
    recordingRef.current = idleRecording;
    mergeRecord(record);
    showToast(record.error_message ?? "录音处理完成", record.error_message ? "error" : "success");
  }

  async function saveSettings(
    next: AppSettings,
    options: { notify?: boolean; syncDraft?: boolean } = {},
  ) {
    const saved = await call<AppSettings>("save_settings", { settings: next });
    setSettings(saved);
    if (options.syncDraft !== false) {
      setSettingsDraft(saved);
    }
    applyTheme(saved.theme);
    if (options.notify !== false) {
      showToast("设置已保存", "success");
    }
    return saved;
  }

  async function savePrompts(next: PromptSettings) {
    const saved = await call<PromptSettings>("save_prompt_settings", { prompts: next });
    setPromptsDraft(saved);
    showToast("偏好已保存", "success");
  }

  async function copyRecord(record: SpeechRecord) {
    await call<boolean>("copy_text", { text: record.final_text });
    showToast("已复制到剪贴板", "success");
  }

  async function deleteRecord(id: string) {
    const next = await call<SpeechRecord[]>("delete_record", { id });
    setRecords(next);
    showToast("记录已删除", "success");
  }

  async function confirmDeleteRecord() {
    if (!deleteRecordId) return;
    await deleteRecord(deleteRecordId);
    if (selectedRecordId === deleteRecordId) setSelectedRecordId(null);
    setDeleteRecordId(null);
  }

  async function retryAsr(record: SpeechRecord) {
    showToast("文字转写中");
    const next = await call<SpeechRecord>("retry_asr", { id: record.id });
    mergeRecord(next);
    showToast(
      next.error_message ?? (next.asr_status === "no_speech" ? "没有录音" : "文字转写完成"),
      next.error_message ? "error" : "success",
    );
  }

  async function retryOptimize(record: SpeechRecord) {
    showToast("内容优化中");
    const next = await call<SpeechRecord>("retry_optimize", { id: record.id });
    mergeRecord(next);
    showToast(next.error_message ?? "内容优化完成", next.error_message ? "error" : "success");
  }

  async function importAudioFile(path: string) {
    if (importingAudioRef.current) return;
    importingAudioRef.current = true;
    setImportingAudio(true);
    showToast("音频导入中");
    try {
      const record = await call<SpeechRecord>("import_audio_file", { path });
      setActiveTab("home");
      mergeRecord(record);
      showToast(record.error_message ?? "音频处理完成", record.error_message ? "error" : "success");
    } catch (error) {
      showToast(String(error), "error");
    } finally {
      importingAudioRef.current = false;
      setImportingAudio(false);
    }
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
    <div className={`app-shell${draggingFile ? " dragging-file" : ""}`}>
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
            偏好
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
            {activeTab === "settings" ? null : activeTab === "preferences" && promptsDraft ? (
              <button className="primary-button" onClick={() => savePrompts(promptsDraft)}>
                <Save size={18} />
                保存偏好
              </button>
            ) : (
              <button className="primary-button" onClick={toggleRecording}>
                <Mic size={18} />
                {recording.active ? "结束录音" : "开始录音"}
              </button>
            )}
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
            <ModelSettings settings={settings} onSave={(next) => saveSettings(next, { notify: false, syncDraft: false })} />
          )}

          {activeTab === "preferences" && promptsDraft && (
            <PreferenceSettings prompts={promptsDraft} onChange={setPromptsDraft} onSave={savePrompts} />
          )}

          {activeTab === "settings" && settings && (
            <AppSettingsView
              settings={settingsDraft ?? settings}
              onChange={setSettingsDraft}
              onSave={(next) => saveSettings(next, { notify: false, syncDraft: false })}
            />
          )}
        </div>
      </main>

      <ToastViewport toasts={toasts} />

      {draggingFile && (
        <div className="drop-overlay">
          <div>
            <FileAudio size={24} />
            <strong>{importingAudio ? "正在导入" : "释放导入 WAV 音频"}</strong>
          </div>
        </div>
      )}

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
        {records.map((record) => {
          const displayText = record.final_text || record.raw_asr_text;
          const showSkeleton = shouldShowRecordSkeleton(record);
          return (
            <article className="record-item compact" key={record.id} onClick={() => onOpenDetails(record)}>
              <div className="record-main">
                {showSkeleton ? (
                  <InlineRecordSkeleton />
                ) : (
                  <p className="record-text">{displayText || "没有录音"}</p>
                )}
                <div className="record-meta-line">
                  <span>{formatDate(record.created_at)}</span>
                  {record.audio_status !== "saved" && <span>录音：{formatAudioStatus(record)}</span>}
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
          );
        })}
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
        <p className="confirm-copy">删除后会从历史记录中移除，并同时删除这条记录对应的本地录音文件。</p>
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

function ToastViewport({ toasts }: { toasts: Toast[] }) {
  if (toasts.length === 0) return null;

  return (
    <div className="toast-viewport" aria-live="polite" aria-atomic="true">
      {toasts.map((toast) => (
        <div className={`toast ${toast.tone ?? "info"}`} key={toast.id}>
          {toast.message}
        </div>
      ))}
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
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const [audioSrc, setAudioSrc] = useState("");
  const [audioMessage, setAudioMessage] = useState("");

  async function playOriginalAudio() {
    if (!record.audio_path) return;
    try {
      const src = audioSrc || (await call<string>("read_audio_data_url", { path: record.audio_path }));
      setAudioSrc(src);
      setAudioMessage("");
      window.setTimeout(() => audioRef.current?.play(), 0);
    } catch (error) {
      setAudioMessage(String(error));
    }
  }

  async function openAudioFolder() {
    if (!record.audio_path) return;
    try {
      await call<boolean>("open_audio_folder", { path: record.audio_path });
      setAudioMessage("");
    } catch (error) {
      setAudioMessage(String(error));
    }
  }

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
            <IconButton label="播放原音频" disabled={!record.audio_path} onClick={playOriginalAudio}>
              <Play size={17} />
            </IconButton>
            <IconButton label="打开录音文件夹" disabled={!record.audio_path} onClick={openAudioFolder}>
              <FolderOpen size={17} />
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
          <span>录音：{formatAudioStatus(record)}</span>
          <span>ASR：{statusLabel[record.asr_status] ?? record.asr_status}</span>
          <span>优化：{statusLabel[record.optimize_status] ?? record.optimize_status}</span>
          {record.duration_ms && <span>时长：{formatDuration(record.duration_ms)}</span>}
          {record.copied_at && <span>已复制 {formatDate(record.copied_at)}</span>}
          {record.pasted_at && <span>已粘贴 {formatDate(record.pasted_at)}</span>}
        </div>

        {audioSrc && <audio ref={audioRef} className="detail-audio" controls src={audioSrc} />}
        {audioMessage && (
          <div className="inline-alert">
            <AlertCircle size={16} />
            {audioMessage}
          </div>
        )}

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

function InlineRecordSkeleton() {
  return (
    <div className="record-text-skeleton" aria-label="正在处理">
      <span />
      <span />
    </div>
  );
}

function ModelSettings({
  settings,
  onSave,
}: {
  settings: AppSettings;
  onSave: (settings: AppSettings) => Promise<AppSettings>;
}) {
  const [draft, setDraft] = useState(settings);
  const [editingProvider, setEditingProvider] = useState<"doubao" | "openrouter" | "deepseek" | "custom_openai" | null>(null);
  const [testStatus, setTestStatus] = useState("");
  const [newModelName, setNewModelName] = useState("");
  const [saveStatus, setSaveStatus] = useState<AutoSaveStatus>("idle");
  const autoSaveReadyRef = useRef(false);
  const onSaveRef = useRef(onSave);
  const openrouterModels = useMemo(() => normalizeModelList(draft, "openrouter"), [draft]);
  const deepseekModels = useMemo(() => normalizeModelList(draft, "deepseek"), [draft]);
  const customOpenAIModels = useMemo(() => normalizeModelList(draft, "custom_openai"), [draft]);
  const textModelOptions = useMemo(() => buildTextModelOptions(draft), [draft]);

  useEffect(() => {
    onSaveRef.current = onSave;
  }, [onSave]);

  useEffect(() => {
    if (!autoSaveReadyRef.current) {
      autoSaveReadyRef.current = true;
      return;
    }
    const snapshot = {
      ...draft,
      openrouter_models: openrouterModels,
      deepseek_models: deepseekModels,
      custom_openai_models: customOpenAIModels,
    };
    setSaveStatus("saving");
    const timer = window.setTimeout(() => {
      onSaveRef.current(snapshot)
        .then(() => setSaveStatus("saved"))
        .catch(() => setSaveStatus("error"));
    }, 700);
    return () => window.clearTimeout(timer);
  }, [customOpenAIModels, deepseekModels, draft, openrouterModels]);

  async function testDoubao() {
    setTestStatus("正在检查豆包配置");
    try {
      const message = await call<string>("test_doubao_config", { settings: draft });
      setTestStatus(message);
    } catch (error) {
      setTestStatus(String(error));
    }
  }

  async function testTextProvider(provider: TextProviderKey = getTextProviderKey(draft.optimize_provider)) {
    const settingsToTest = { ...draft, optimize_provider: provider };
    setTestStatus(`正在测试 ${getActiveTextProviderLabel(settingsToTest)}`);
    try {
      const message = await call<string>("test_openrouter", { settings: settingsToTest });
      setTestStatus(message);
    } catch (error) {
      setTestStatus(String(error));
    }
  }

  function selectProviderModel(provider: TextProviderKey, model: string) {
    if (provider === "deepseek") {
      setDraft({ ...draft, optimize_provider: provider, deepseek_model: model, deepseek_models: deepseekModels });
      return;
    }
    if (provider === "custom_openai") {
      setDraft({
        ...draft,
        optimize_provider: provider,
        custom_openai_model: model,
        custom_openai_models: customOpenAIModels,
      });
      return;
    }
    setDraft({ ...draft, optimize_provider: provider, openrouter_model: model, openrouter_models: openrouterModels });
  }

  function addProviderModel(provider: TextProviderKey) {
    const model = newModelName.trim();
    if (!model) return;
    if (provider === "deepseek") {
      const deepseek_models = Array.from(new Set([...deepseekModels, model]));
      setDraft({ ...draft, deepseek_model: model, deepseek_models });
    } else if (provider === "custom_openai") {
      const custom_openai_models = Array.from(new Set([...customOpenAIModels, model]));
      setDraft({ ...draft, custom_openai_model: model, custom_openai_models });
    } else {
      const openrouter_models = Array.from(new Set([...openrouterModels, model]));
      setDraft({ ...draft, openrouter_model: model, openrouter_models });
    }
    setNewModelName("");
  }

  function deleteProviderModel(provider: TextProviderKey, model: string) {
    if (provider === "deepseek") {
      const deepseek_models = deepseekModels.filter((item) => item !== model);
      const deepseek_model = draft.deepseek_model === model ? (deepseek_models[0] ?? "") : draft.deepseek_model;
      setDraft({ ...draft, deepseek_model, deepseek_models });
      return;
    }
    if (provider === "custom_openai") {
      const custom_openai_models = customOpenAIModels.filter((item) => item !== model);
      const custom_openai_model =
        draft.custom_openai_model === model ? (custom_openai_models[0] ?? "") : draft.custom_openai_model;
      setDraft({ ...draft, custom_openai_model, custom_openai_models });
      return;
    }
    const openrouter_models = openrouterModels.filter((item) => item !== model);
    const openrouter_model = draft.openrouter_model === model ? (openrouter_models[0] ?? "") : draft.openrouter_model;
    setDraft({ ...draft, openrouter_model, openrouter_models });
  }

  function selectTextModel(value: string) {
    const separator = value.indexOf("|");
    if (separator < 0) return;
    const provider = value.slice(0, separator);
    const model = value.slice(separator + 1);
    if (provider === "deepseek") {
      setDraft({ ...draft, optimize_provider: "deepseek", deepseek_model: model, deepseek_models: deepseekModels });
      return;
    }
    if (provider === "custom_openai") {
      setDraft({
        ...draft,
        optimize_provider: "custom_openai",
        custom_openai_model: model,
        custom_openai_models: customOpenAIModels,
      });
      return;
    }
    setDraft({ ...draft, optimize_provider: "openrouter", openrouter_model: model, openrouter_models: openrouterModels });
  }

  function providerModels(provider: TextProviderKey) {
    if (provider === "deepseek") return deepseekModels;
    if (provider === "custom_openai") return customOpenAIModels;
    return openrouterModels;
  }

  function selectedProviderModel(provider: TextProviderKey) {
    if (provider === "deepseek") return draft.deepseek_model;
    if (provider === "custom_openai") return draft.custom_openai_model;
    return draft.openrouter_model;
  }

  function renderModelManager(provider: TextProviderKey, placeholder: string) {
    const models = providerModels(provider);
    const selected = selectedProviderModel(provider);
    return (
      <section className="model-manager">
        <div className="model-manager-heading">
          <div>
            <h3>模型</h3>
            <p>添加常用模型，然后在上方默认模型里选择。</p>
          </div>
          <div className="model-add-row">
            <input
              value={newModelName}
              placeholder={placeholder}
              onChange={(event) => setNewModelName(event.currentTarget.value)}
            />
            <button className="secondary-button" type="button" onClick={() => addProviderModel(provider)}>
              <Plus size={16} />
              添加模型
            </button>
          </div>
        </div>
        <div className="model-list">
          {models.length === 0 && <p className="empty-model-list">还没有模型。</p>}
          {models.map((model) => (
            <article className={model === selected ? "model-row active" : "model-row"} key={model}>
              <div>
                <strong>{model}</strong>
                <span>{model === selected ? "当前文本模型" : "自定义"}</span>
              </div>
              <div className="model-row-actions">
                <IconButton label="删除模型" onClick={() => deleteProviderModel(provider, model)}>
                  <Trash2 size={16} />
                </IconButton>
                <button className="secondary-button" type="button" onClick={() => selectProviderModel(provider, model)}>
                  <CheckCircle2 size={16} />
                  设为当前
                </button>
              </div>
            </article>
          ))}
        </div>
      </section>
    );
  }

  const activeTextModel = getActiveTextModel(draft);
  const activeTextProvider = getActiveTextProviderLabel(draft);
  const activeTextModelKey = `${draft.optimize_provider}|${activeTextModel}`;

  return (
    <section className="settings-page provider-page">
      <AutoSaveNotice status={saveStatus} />

      <section className="model-defaults">
        <header className="model-defaults-header">
          <Settings size={24} />
          <div>
            <p>默认模型</p>
            <h2>选择模型</h2>
          </div>
        </header>
        <div className="model-select-grid">
          <label className="model-select-card">
            <span>语音识别模型</span>
            <select value="doubao-streaming" onChange={() => undefined}>
              <option value="doubao-streaming">豆包流式语音识别模型 2.0</option>
            </select>
            <small>流式音频转文本模型</small>
          </label>
          <label className="model-select-card">
            <span>文本优化模型</span>
            <select
              value={activeTextModelKey}
              onChange={(event) => selectTextModel(event.currentTarget.value)}
            >
              {textModelOptions.length === 0 && <option value="">未设置模型</option>}
              {textModelOptions.map((option) => (
                <option value={option.value} key={option.value}>
                  {option.label}
                </option>
              ))}
            </select>
            <small>{activeTextProvider} · {activeTextModel || "未设置模型"}</small>
          </label>
        </div>
      </section>

      <div className="provider-grid">
        <ProviderCard
          icon={<Headphones size={20} />}
          title="豆包"
          eyebrow="ASR Provider"
          description="当前仅支持豆包流式识别，负责把录音转成原始文字。"
          status={draft.doubao_auth_mode === "app_access_key" ? "App Key 鉴权" : "API Key 鉴权"}
          onEdit={() => setEditingProvider("doubao")}
          onTest={testDoubao}
        />
        <ProviderCard
          icon={<Bot size={20} />}
          title="OpenRouter"
          eyebrow="文本模型 Provider"
          description="管理 API、Endpoint 和可选文本模型。"
          status={draft.optimize_provider === "openrouter" ? `当前：${draft.openrouter_model || "未设置模型"}` : draft.openrouter_model || "未设置模型"}
          onEdit={() => setEditingProvider("openrouter")}
          onTest={() => testTextProvider("openrouter")}
        />
        <ProviderCard
          icon={<Bot size={20} />}
          title="DeepSeek"
          eyebrow="OpenAI-compatible"
          description="使用 DeepSeek API 做 ASR 文本优化。"
          status={draft.optimize_provider === "deepseek" ? `当前：${draft.deepseek_model || "未设置模型"}` : draft.deepseek_model || "未设置模型"}
          onEdit={() => setEditingProvider("deepseek")}
          onTest={() => testTextProvider("deepseek")}
        />
        <ProviderCard
          icon={<Bot size={20} />}
          title={draft.custom_openai_provider_name || "Custom"}
          eyebrow="OpenAI-compatible"
          description="自定义 Base URL、API Key 和模型名。"
          status={draft.optimize_provider === "custom_openai" ? `当前：${draft.custom_openai_model || "未设置模型"}` : draft.custom_openai_model || "未设置模型"}
          onEdit={() => setEditingProvider("custom_openai")}
          onTest={() => testTextProvider("custom_openai")}
        />
      </div>

      {testStatus && <div className="inline-alert neutral"><CheckCircle2 size={16} />{testStatus}</div>}

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
        <ProviderModal title="OpenRouter" onClose={() => setEditingProvider(null)} onTest={() => testTextProvider("openrouter")}>
          <TextField label="API Key" value={draft.openrouter_api_key} type="password" onChange={(openrouter_api_key) => setDraft({ ...draft, openrouter_api_key })} />
          <TextField label="Base URL" value={draft.openrouter_base_url} onChange={(openrouter_base_url) => setDraft({ ...draft, openrouter_base_url })} />
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
          {renderModelManager("openrouter", "例如 openai/gpt-4.1-mini")}
        </ProviderModal>
      )}

      {editingProvider === "deepseek" && (
        <ProviderModal title="DeepSeek" onClose={() => setEditingProvider(null)} onTest={() => testTextProvider("deepseek")}>
          <TextField label="API Key" value={draft.deepseek_api_key} type="password" onChange={(deepseek_api_key) => setDraft({ ...draft, deepseek_api_key })} />
          <TextField label="Base URL" value={draft.deepseek_base_url} onChange={(deepseek_base_url) => setDraft({ ...draft, deepseek_base_url })} />
          {renderModelManager("deepseek", "例如 deepseek-chat")}
          <button className="secondary-button" type="button" onClick={() => setDraft({ ...draft, optimize_provider: "deepseek" })}>
            <CheckCircle2 size={16} />
            设为当前模型
          </button>
        </ProviderModal>
      )}

      {editingProvider === "custom_openai" && (
        <ProviderModal title="自定义 OpenAI-compatible" onClose={() => setEditingProvider(null)} onTest={() => testTextProvider("custom_openai")}>
          <TextField label="名称" value={draft.custom_openai_provider_name} onChange={(custom_openai_provider_name) => setDraft({ ...draft, custom_openai_provider_name })} />
          <TextField label="API Key" value={draft.custom_openai_api_key} type="password" onChange={(custom_openai_api_key) => setDraft({ ...draft, custom_openai_api_key })} />
          <TextField label="Base URL" value={draft.custom_openai_base_url} onChange={(custom_openai_base_url) => setDraft({ ...draft, custom_openai_base_url })} />
          {renderModelManager("custom_openai", "例如 gpt-4.1-mini")}
          <button className="secondary-button" type="button" onClick={() => setDraft({ ...draft, optimize_provider: "custom_openai" })}>
            <CheckCircle2 size={16} />
            设为当前模型
          </button>
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
  onChange,
  settings,
  onSave,
}: {
  onChange: (settings: AppSettings) => void;
  settings: AppSettings;
  onSave: (settings: AppSettings) => Promise<AppSettings>;
}) {
  const [activeSettingsTab, setActiveSettingsTab] = useState<"general" | "recording" | "logs" | "about">("general");
  const [microphones, setMicrophones] = useState<string[]>([]);
  const [logs, setLogs] = useState("");
  const [capturingShortcut, setCapturingShortcut] = useState(false);
  const [micTestStatus, setMicTestStatus] = useState("");
  const [micSampleSrc, setMicSampleSrc] = useState("");
  const [appVersion, setAppVersion] = useState("");
  const [updateStatus, setUpdateStatus] = useState("尚未检查更新");
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [saveStatus, setSaveStatus] = useState<AutoSaveStatus>("idle");
  const autoSaveReadyRef = useRef(false);
  const onSaveRef = useRef(onSave);

  useEffect(() => {
    onSaveRef.current = onSave;
  }, [onSave]);

  useEffect(() => {
    if (!autoSaveReadyRef.current) {
      autoSaveReadyRef.current = true;
      return;
    }
    const snapshot = settings;
    setSaveStatus("saving");
    const timer = window.setTimeout(() => {
      onSaveRef.current(snapshot)
        .then(() => setSaveStatus("saved"))
        .catch(() => setSaveStatus("error"));
    }, 700);
    return () => window.clearTimeout(timer);
  }, [settings]);

  useEffect(() => {
    call<string[]>("list_microphones")
      .then(setMicrophones)
      .catch(() => setMicrophones([]));
    call<string>("get_app_version")
      .then(setAppVersion)
      .catch(() => setAppVersion(""));
  }, []);

  useEffect(() => {
    if (!capturingShortcut) return;

    function handleKeyDown(event: KeyboardEvent) {
      event.preventDefault();
      event.stopPropagation();
      onChange({ ...settings, global_shortcut: normalizeShortcutCode(event.code) });
      setCapturingShortcut(false);
    }

    window.addEventListener("keydown", handleKeyDown, true);
    return () => window.removeEventListener("keydown", handleKeyDown, true);
  }, [capturingShortcut, onChange, settings]);

  async function loadLogs() {
    const content = await call<string>("read_logs");
    setLogs(content || "暂无日志。");
  }

  async function recordMicrophoneSample() {
    setMicTestStatus("正在录制试听片段");
    try {
      const path = await call<string>("record_microphone_sample", { microphoneName: settings.microphone_name });
      const src = await call<string>("read_audio_data_url", { path });
      setMicSampleSrc(src);
      setMicTestStatus("试听片段已录好");
    } catch (error) {
      setMicTestStatus(String(error));
      setMicSampleSrc("");
    }
  }

  async function checkForUpdate() {
    setCheckingUpdate(true);
    setUpdateStatus("正在检查更新");
    try {
      const update = await check();
      if (!update) {
        setUpdateStatus("当前已经是最新版本");
        return;
      }
      setUpdateStatus(`发现 ${update.version}，正在下载并安装`);
      await update.downloadAndInstall();
      setUpdateStatus("更新安装完成，正在重启");
      await relaunch();
    } catch (error) {
      setUpdateStatus(`检查更新失败：${String(error)}`);
    } finally {
      setCheckingUpdate(false);
    }
  }

  return (
    <section className="settings-page settings-tabs-page">
      <div className="settings-tab-list" role="tablist" aria-label="设置分类">
        <button className={activeSettingsTab === "general" ? "active" : ""} type="button" onClick={() => setActiveSettingsTab("general")}>
          通用
        </button>
        <button className={activeSettingsTab === "recording" ? "active" : ""} type="button" onClick={() => setActiveSettingsTab("recording")}>
          录音
        </button>
        <button className={activeSettingsTab === "logs" ? "active" : ""} type="button" onClick={() => setActiveSettingsTab("logs")}>
          日志
        </button>
        <button className={activeSettingsTab === "about" ? "active" : ""} type="button" onClick={() => setActiveSettingsTab("about")}>
          关于
        </button>
      </div>

      <AutoSaveNotice status={saveStatus} />

      {activeSettingsTab === "general" && (
        <div className="settings-section">
          <div className="section-heading">
            <h2>通用</h2>
            <p>设置启动、粘贴和界面显示方式。</p>
          </div>
          <div className="field-grid">
            <div className="theme-buttons" role="group" aria-label="主题">
              <button
                className={settings.theme === "system" ? "active" : ""}
                type="button"
                onClick={() => {
                  const next = { ...settings, theme: "system" };
                  onChange(next);
                  applyTheme(next.theme);
                }}
              >
                <Monitor size={16} />
                跟随系统
              </button>
              <button
                className={settings.theme === "light" ? "active" : ""}
                type="button"
                onClick={() => {
                  const next = { ...settings, theme: "light" };
                  onChange(next);
                  applyTheme(next.theme);
                }}
              >
                <Sun size={16} />
                浅色
              </button>
              <button
                className={settings.theme === "dark" ? "active" : ""}
                type="button"
                onClick={() => {
                  const next = { ...settings, theme: "dark" };
                  onChange(next);
                  applyTheme(next.theme);
                }}
              >
                <Moon size={16} />
                深色
              </button>
            </div>
            <SettingsSwitch
              checked={settings.auto_paste}
              title="整理成功后自动粘贴"
              description="整理完成后直接粘贴到当前光标位置。"
              onChange={(checked) => onChange({ ...settings, auto_paste: checked })}
            />
            <SettingsSwitch
              checked={settings.launch_at_startup}
              title="开机自启动"
              description="登录 Windows 后自动启动 SparkSpeech。"
              onChange={(checked) => onChange({ ...settings, launch_at_startup: checked })}
            />
            <SettingsSwitch
              checked={settings.fast_asr_finalize}
              title="快速完成转写（实验）"
              description="录音时提前上传音频；若连接中断，结束后改用完整录音转写。"
              onChange={(checked) => onChange({ ...settings, fast_asr_finalize: checked })}
            />
            <SettingsSwitch
              checked={settings.show_realtime_transcript}
              title="显示实时字幕预览"
              description="录音时在底部状态条显示临时识别文本。"
              onChange={(checked) => onChange({ ...settings, show_realtime_transcript: checked })}
            />
          </div>
        </div>
      )}

      {activeSettingsTab === "recording" && (
        <div className="settings-section">
        <div className="section-heading">
          <h2>录音</h2>
          <p>选择麦克风和全局快捷键。</p>
        </div>
        <div className="field-grid">
          <label className="text-field">
            <span>麦克风</span>
            <select
              value={settings.microphone_name}
              onChange={(event) => onChange({ ...settings, microphone_name: event.currentTarget.value })}
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
              {capturingShortcut ? "请按下一个键" : shortcutLabel(settings.global_shortcut)}
            </button>
          </label>
          <TextField
            label="录音保留天数"
            value={String(settings.recording_retention_days)}
            type="number"
            onChange={(value) => onChange({ ...settings, recording_retention_days: Number(value) || 1 })}
          />
          <label className="text-field">
            <span>片段保存间隔</span>
            <select
              value={String(settings.recording_segment_seconds)}
              onChange={(event) => onChange({ ...settings, recording_segment_seconds: Number(event.currentTarget.value) })}
            >
              {[5, 10, 15, 20, 25, 30].map((seconds) => (
                <option value={seconds} key={seconds}>
                  {seconds} 秒
                </option>
              ))}
            </select>
          </label>
        </div>
        {micTestStatus && <div className="inline-alert neutral"><CheckCircle2 size={16} />{micTestStatus}</div>}
        {micSampleSrc && <audio className="audio-preview" controls src={micSampleSrc} />}
      </div>
      )}

      {activeSettingsTab === "logs" && (
        <div className="settings-section">
        <div className="section-heading">
          <h2>日志</h2>
          <p>保存和查看本地运行日志。</p>
        </div>
        <SettingsSwitch
          checked={settings.save_logs}
          title="保存日志"
          description="记录录音、转写、整理和清理过程，方便排查异常。"
          onChange={(checked) => onChange({ ...settings, save_logs: checked })}
        />
        <div className="button-row">
          <button className="secondary-button" onClick={loadLogs}>
            查看日志
          </button>
        </div>
        {logs && <pre className="log-viewer">{logs}</pre>}
      </div>
      )}

      {activeSettingsTab === "about" && (
        <div className="settings-section about-section">
          <img className="about-logo" src="/logo.svg" alt="" />
          <h2>SparkSpeech</h2>
          <p>简约、大方、开源的类闪电说智能语音输入法。</p>
          <div className="about-version-row">
            <span>v{appVersion || "未知"}</span>
            <button className="secondary-button" type="button" disabled={checkingUpdate} onClick={checkForUpdate}>
              <RefreshCw size={16} />
              {checkingUpdate ? "检查中" : "检查更新"}
            </button>
          </div>
          <div className="about-update-status">{updateStatus}</div>
          <div className="about-links">
            <a href="https://github.com/lanbinleo/SparkSpeech" target="_blank" rel="noreferrer">GitHub</a>
            <span>·</span>
            <a href="https://github.com/lanbinleo/SparkSpeech/releases" target="_blank" rel="noreferrer">Releases</a>
          </div>
        </div>
      )}
    </section>
  );
}

function PreferenceSettings({
  onChange,
  prompts,
  onSave,
}: {
  onChange: (prompts: PromptSettings) => void;
  prompts: PromptSettings;
  onSave: (prompts: PromptSettings) => Promise<void>;
}) {
  const cleanupMode = prompts.cleanup_mode || "plain";

  async function selectCleanupMode(cleanup_mode: string) {
    if (cleanup_mode === cleanupMode) return;
    const next = { ...prompts, cleanup_mode };
    onChange(next);
    await onSave(next);
  }

  return (
    <section className="settings-page preference-page">
      <div className="settings-section">
        <div className="section-heading">
          <h2>整理强度</h2>
          <p>选择额外的整理风格；原话模式只使用基础提示词、词典和个性化偏好。</p>
        </div>
        <div className="cleanup-mode-grid" role="radiogroup" aria-label="整理强度">
          {cleanupModeOptions.map((option) => (
            <button
              className={cleanupMode === option.value ? "cleanup-mode-card active" : "cleanup-mode-card"}
              type="button"
              role="radio"
              aria-checked={cleanupMode === option.value}
              onClick={() => selectCleanupMode(option.value)}
              key={option.value}
            >
              <strong>{option.title}</strong>
              <span>{option.description}</span>
              <em>{option.example}</em>
            </button>
          ))}
        </div>
      </div>

      <div className="settings-section">
        <div className="section-heading">
          <h2>系统提示词</h2>
          <p>决定文本整理器的任务边界。</p>
        </div>
        <textarea
          value={prompts.system_prompt}
          onChange={(event) => onChange({ ...prompts, system_prompt: event.currentTarget.value })}
        />
      </div>

      <div className="settings-section">
        <div className="section-heading">
          <h2>个性化偏好</h2>
          <p>控制分段、标点、空格、公式和表达习惯。</p>
        </div>
        <textarea
          value={prompts.writing_preferences}
          onChange={(event) => onChange({ ...prompts, writing_preferences: event.currentTarget.value })}
        />
      </div>

      <div className="settings-section">
        <div className="section-heading">
          <h2>词条替换</h2>
          <p>一行一个词，或使用 A -&gt; B 表达明确替换。</p>
        </div>
        <textarea
          className="dictionary-box"
          value={prompts.replacements}
          onChange={(event) => onChange({ ...prompts, replacements: event.currentTarget.value })}
        />
      </div>

      <button className="primary-button save-button" onClick={() => onSave(prompts)}>
        <Wand2 size={18} />
        保存偏好
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

function SettingsSwitch({
  checked,
  description,
  title,
  onChange,
}: {
  checked: boolean;
  description: string;
  title: string;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="switch-row">
      <span className="switch-copy">
        <strong>{title}</strong>
        <small>{description}</small>
      </span>
      <input checked={checked} type="checkbox" onChange={(event) => onChange(event.currentTarget.checked)} />
      <i aria-hidden="true" />
    </label>
  );
}

function AutoSaveNotice({ status }: { status: AutoSaveStatus }) {
  if (status === "idle") return null;
  if (status === "saving") {
    return <div className="inline-alert neutral"><RefreshCw size={16} />正在保存</div>;
  }
  if (status === "error") {
    return <div className="inline-alert error"><AlertCircle size={16} />保存失败</div>;
  }
  return <div className="inline-alert neutral"><CheckCircle2 size={16} />已保存</div>;
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
  const optimizedRecords = records.filter(
    (record) => record.optimize_status === "completed" && record.final_text.trim() && record.duration_ms,
  );
  const totalChars = optimizedRecords.reduce((sum, record) => sum + countTextChars(record.final_text), 0);
  const optimizedMs = optimizedRecords.reduce((sum, record) => sum + (record.duration_ms ?? 0), 0);
  const minutes = optimizedMs / 60_000;
  return {
    totalHours: totalMs / 3_600_000,
    totalChars,
    charsPerMinute: minutes > 0 ? Math.round(totalChars / minutes) : 0,
  };
}

function shouldShowRecordSkeleton(record: SpeechRecord) {
  if ((record.final_text || record.raw_asr_text).trim()) return false;
  if (record.error_message) return false;
  if (!record.audio_path && record.audio_status !== "pending") return false;
  if (record.asr_status === "no_speech" || record.asr_status === "failed") return false;
  return record.asr_status === "pending" || record.optimize_status === "pending";
}

function formatAudioStatus(record: SpeechRecord) {
  if (!record.audio_status && record.audio_path) return audioStatusLabel.saved;
  if (!record.audio_status) return audioStatusLabel.expired;
  return audioStatusLabel[record.audio_status] ?? record.audio_status;
}

function normalizeModelList(settings: AppSettings, provider: TextProviderKey) {
  const models =
    provider === "deepseek"
      ? (settings.deepseek_models ?? [])
      : provider === "custom_openai"
        ? (settings.custom_openai_models ?? [])
        : (settings.openrouter_models ?? []);
  const selected =
    provider === "deepseek"
      ? settings.deepseek_model.trim()
      : provider === "custom_openai"
        ? settings.custom_openai_model.trim()
        : settings.openrouter_model.trim();
  return Array.from(new Set([...models, selected].map((model) => model.trim()).filter(Boolean)));
}

const cleanupModeOptions = [
  {
    value: "plain",
    title: "原话",
    description: "尽量保留原表达，只做必要识别修正。",
    example: "保留说话顺序和原本语气。",
  },
  {
    value: "light",
    title: "轻度整理",
    description: "清理口头禅、自我修正，并轻度格式化。",
    example: "适合日常输入和即时发送。",
  },
  {
    value: "deep",
    title: "深度整理",
    description: "提取中心意思，整理成更清楚的书面表达。",
    example: "适合会议、方案和长段想法。",
  },
];

function buildTextModelOptions(settings: AppSettings) {
  const options = normalizeModelList(settings, "openrouter").map((model) => ({
    value: `openrouter|${model}`,
    label: `${model} (OpenRouter)`,
  }));
  for (const model of normalizeModelList(settings, "deepseek")) {
    options.push({
      value: `deepseek|${model}`,
      label: `${model} (DeepSeek)`,
    });
  }
  for (const model of normalizeModelList(settings, "custom_openai")) {
    options.push({
      value: `custom_openai|${model}`,
      label: `${model} (${settings.custom_openai_provider_name || "Custom"})`,
    });
  }
  return options;
}

function getActiveTextProviderLabel(settings: AppSettings) {
  if (settings.optimize_provider === "deepseek") return "DeepSeek";
  if (settings.optimize_provider === "custom_openai") return settings.custom_openai_provider_name || "Custom";
  return "OpenRouter";
}

function getTextProviderKey(provider: string): TextProviderKey {
  if (provider === "deepseek" || provider === "custom_openai") return provider;
  return "openrouter";
}

function getActiveTextModel(settings: AppSettings) {
  if (settings.optimize_provider === "deepseek") return settings.deepseek_model;
  if (settings.optimize_provider === "custom_openai") return settings.custom_openai_model;
  return settings.openrouter_model;
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
  return seconds === 0 ? `${minutes} 分` : `${minutes} 分 ${seconds} 秒`;
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
