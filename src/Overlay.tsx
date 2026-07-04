import { useEffect, useState } from "react";
import type { CSSProperties } from "react";
import { listen } from "@tauri-apps/api/event";
import { Check, Mic, RotateCw, WifiOff } from "lucide-react";
import { call } from "./tauri";
import type { OverlayState } from "./tauri";

export function Overlay() {
  const [state, setState] = useState<OverlayState>({
    visible: false,
    phase: "idle",
    label: "",
    elapsed_ms: 0,
    input_level: 0,
    action_label: null,
    status_kind: null,
    transcript_lines: [],
    progress_current: null,
    progress_total: null,
    reconnect_available: false,
  });
  const [speechHoldUntil, setSpeechHoldUntil] = useState(0);
  const [now, setNow] = useState(Date.now());

  useEffect(() => {
    document.documentElement.classList.add("overlay-html");
    document.body.classList.add("overlay-body");
    call<OverlayState>("get_overlay_state")
      .then(setState)
      .catch(() => undefined);
    const unlisten = listen<OverlayState>("overlay-state", (event) => {
      setState(event.payload);
    });

    return () => {
      document.documentElement.classList.remove("overlay-html");
      document.body.classList.remove("overlay-body");
      unlisten.then((dispose) => dispose());
    };
  }, []);

  useEffect(() => {
    if (state.phase !== "recording") return;
    if ((state.input_level ?? 0) > 0.08) {
      setSpeechHoldUntil(Date.now() + 1400);
    }
  }, [state.input_level, state.phase]);

  useEffect(() => {
    if (state.phase !== "recording") return;
    const timer = window.setInterval(() => setNow(Date.now()), 120);
    return () => window.clearInterval(timer);
  }, [state.phase]);

  if (!state.visible) return <div className="overlay-root" />;
  const level = Math.max(0, Math.min(1, state.input_level ?? 0));
  const speaking = level > 0.08 || now < speechHoldUntil;
  const waveStyle = { "--level": speaking ? "1" : "0" } as CSSProperties;
  const [attentionTitle, ...attentionDetail] = state.label.split("：");
  const progressPercent = getProgressPercent(state);
  const statusText = getStatusText(state);

  async function handleAction() {
    if (state.reconnect_available) {
      await call<boolean>("reconnect_realtime_asr");
      return;
    }
    await call<boolean>("open_main_window");
  }

  return (
    <div className="overlay-root">
      {(state.transcript_lines ?? []).length > 0 && (
        <div className="overlay-transcript">
          {state.transcript_lines.map((line, index) => (
            <span className={index === state.transcript_lines.length - 1 ? "latest" : ""} key={`${line}-${index}`}>
              {line}
            </span>
          ))}
        </div>
      )}
      <div className={`recording-pill overlay-pill ${state.phase}`}>
        {state.phase === "attention" ? (
          <>
            <div className="overlay-message">
              <strong>{attentionTitle}</strong>
              <span>{attentionDetail.join("：")}</span>
            </div>
            <button className="overlay-action" type="button" onClick={handleAction}>
              {state.action_label ?? "打开主界面"}
            </button>
          </>
        ) : (
          <>
            <OverlayStatusIcon kind={state.status_kind ?? state.phase} />
            <i />
            {state.phase === "recording" ? (
              <div className={speaking ? "wave speaking" : "wave quiet"} style={waveStyle} aria-hidden="true">
                <b style={{ "--bar": "0.55" } as CSSProperties} />
                <b style={{ "--bar": "0.9" } as CSSProperties} />
                <b style={{ "--bar": "0.75" } as CSSProperties} />
                <b style={{ "--bar": "0.45" } as CSSProperties} />
              </div>
            ) : (
              progressPercent !== null ? (
                <div
                  className="overlay-progress-ring"
                  style={{ "--progress": `${progressPercent}%` } as CSSProperties}
                  aria-hidden="true"
                />
              ) : (
                <div className="spinner" aria-hidden="true" />
              )
            )}
            {statusText && <small className="overlay-progress-text">{statusText}</small>}
            {state.reconnect_available && (
              <button className="overlay-icon-action" type="button" onClick={handleAction} aria-label="重连实时转写">
                <RotateCw size={14} />
              </button>
            )}
          </>
        )}
      </div>
    </div>
  );
}

function OverlayStatusIcon({ kind }: { kind: string }) {
  if (kind === "saved") return <Check className="overlay-status-icon saved" size={16} aria-label="已保存" />;
  if (kind === "network_error") return <WifiOff className="overlay-status-icon error" size={16} aria-label="实时转写断开" />;
  return <Mic className="overlay-status-icon" size={16} aria-label="录音中" />;
}

function getProgressPercent(state: OverlayState) {
  if (typeof state.progress_current !== "number" || typeof state.progress_total !== "number" || state.progress_total <= 0) {
    return null;
  }
  const percent = Math.min(100, Math.max(0, (state.progress_current / state.progress_total) * 100));
  if (state.phase === "optimizing") {
    if (state.progress_current >= state.progress_total) return 100;
    return Math.min(95, percent);
  }
  return percent;
}

function getStatusText(state: OverlayState) {
  if (state.phase === "recording") {
    if (state.status_kind === "network_error") return "断线";
    if (state.status_kind === "saved") return "已保存";
    return formatDuration(state.elapsed_ms);
  }
  if (state.phase === "saving") return "保存中";
  if (typeof state.progress_current !== "number" || typeof state.progress_total !== "number" || state.progress_total <= 0) {
    if (state.phase === "transcribing") return "转写中";
    if (state.phase === "optimizing") return "优化中";
    return "";
  }
  if (state.phase === "optimizing") {
    return `${formatCount(state.progress_current)} / 约 ${formatCount(state.progress_total)}`;
  }
  return `${formatDuration(state.progress_current)} / ${formatDuration(state.progress_total)}`;
}

function formatDuration(ms: number) {
  const totalSeconds = Math.max(0, Math.floor(ms / 1000));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
}

function formatCount(value: number) {
  if (value < 1000) return String(Math.max(0, Math.round(value)));
  return `${(value / 1000).toFixed(1)}k`;
}
