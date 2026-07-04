import { useEffect, useState } from "react";
import type { CSSProperties } from "react";
import { listen } from "@tauri-apps/api/event";
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

  async function handleAction() {
    await call<boolean>("open_main_window");
  }

  return (
    <div className="overlay-root">
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
            <span>{state.label}</span>
            <i />
            {state.phase === "recording" ? (
              <div className={speaking ? "wave speaking" : "wave quiet"} style={waveStyle} aria-hidden="true">
                <b style={{ "--bar": "0.55" } as CSSProperties} />
                <b style={{ "--bar": "0.9" } as CSSProperties} />
                <b style={{ "--bar": "0.75" } as CSSProperties} />
                <b style={{ "--bar": "0.45" } as CSSProperties} />
              </div>
            ) : (
              <div className="spinner" aria-hidden="true" />
            )}
          </>
        )}
      </div>
    </div>
  );
}
