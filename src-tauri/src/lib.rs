#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            get_bootstrap,
            save_settings,
            save_prompt_settings,
            list_records,
            list_records_page,
            delete_record,
            copy_text,
            start_recording,
            stop_recording,
            retry_asr,
            retry_optimize,
            get_overlay_state,
            list_microphones,
            read_logs,
            test_microphone,
            record_microphone_sample,
            test_doubao_config,
            test_openrouter,
            debug_transcribe_file
        ])
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            let handle = app.handle().clone();
            register_shortcut(&handle, app.state::<AppState>())?;
            create_tray(app)?;

            if let Some(overlay) = app.get_webview_window("overlay") {
                position_overlay(&overlay)?;
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

pub mod models;
pub mod recorder;
pub mod services;
mod shortcut;
mod storage;

use std::{path::PathBuf, sync::Mutex};

use arboard::Clipboard;
use chrono::{Duration, Utc};
use models::{
    AppSettings, BootstrapData, OverlayState, PromptSettings, RecordPage, RecordingSession,
    SpeechRecord,
};
use recorder::AudioRecorder;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    App, AppHandle, Emitter, Manager, State, WebviewWindow,
};
use uuid::Uuid;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_CONTROL, VK_V,
};

#[derive(Default)]
struct AppState {
    recording: Mutex<RecordingSession>,
    recorder: Mutex<Option<AudioRecorder>>,
    shortcut: Mutex<Option<shortcut::ShortcutHandle>>,
    overlay: Mutex<OverlayState>,
}

fn create_tray(app: &App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open-main", "打开主界面", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出应用", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut builder = TrayIconBuilder::with_id("sparkspeech")
        .tooltip("SparkSpeech")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open-main" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app)?;
    Ok(())
}

#[tauri::command]
fn get_bootstrap(app: AppHandle, state: State<'_, AppState>) -> Result<BootstrapData, String> {
    let settings = storage::get_settings(&app)?;
    let prompts = storage::get_prompts(&app)?;
    let page = storage::read_record_page(&app, 0, 60)?;
    let recording = state
        .recording
        .lock()
        .map_err(|_| "无法读取录音状态".to_string())?
        .clone();

    Ok(BootstrapData {
        settings,
        prompts,
        records: page.records,
        recording,
    })
}

#[tauri::command]
fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: AppSettings,
) -> Result<AppSettings, String> {
    storage::save_settings(&app, &settings)?;
    register_shortcut(&app, state)?;
    Ok(settings)
}

#[tauri::command]
fn save_prompt_settings(app: AppHandle, prompts: PromptSettings) -> Result<PromptSettings, String> {
    storage::save_prompts(&app, &prompts)?;
    Ok(prompts)
}

#[tauri::command]
fn list_records(app: AppHandle) -> Result<Vec<SpeechRecord>, String> {
    storage::read_records(&app)
}

#[tauri::command]
fn list_records_page(app: AppHandle, offset: usize, limit: usize) -> Result<RecordPage, String> {
    storage::read_record_page(&app, offset, limit)
}

#[tauri::command]
fn delete_record(app: AppHandle, id: String) -> Result<Vec<SpeechRecord>, String> {
    storage::delete_record(&app, &id)
}

#[tauri::command]
fn copy_text(text: String) -> Result<bool, String> {
    copy_to_clipboard(&text)?;
    Ok(true)
}

#[tauri::command]
fn list_microphones() -> Result<Vec<String>, String> {
    recorder::list_input_devices()
}

#[tauri::command]
fn read_logs(app: AppHandle) -> Result<String, String> {
    storage::read_log(&app)
}

#[tauri::command]
fn test_microphone(microphone_name: String) -> Result<f32, String> {
    recorder::test_input_level(Some(&microphone_name))
}

#[tauri::command]
fn record_microphone_sample(app: AppHandle, microphone_name: String) -> Result<String, String> {
    let recorder = AudioRecorder::start(Some(&microphone_name))?;
    std::thread::sleep(std::time::Duration::from_millis(1800));
    let audio = recorder.stop()?;
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("tests");
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let path = dir.join("microphone-test.wav");
    audio.save_wav(&path)?;
    Ok(path_to_string(&path))
}

#[tauri::command]
fn test_doubao_config(settings: AppSettings) -> Result<String, String> {
    if settings.doubao_endpoint.trim().is_empty() {
        return Err("豆包 Endpoint 为空".into());
    }
    if settings.doubao_resource_id.trim().is_empty() {
        return Err("豆包 Resource ID 为空".into());
    }
    if settings.doubao_auth_mode == "app_access_key" {
        if settings.doubao_app_key.trim().is_empty() || settings.doubao_access_key.trim().is_empty()
        {
            return Err("旧版鉴权需要 App Key 和 Access Key".into());
        }
    } else if settings.doubao_api_key.trim().is_empty() {
        return Err("新版鉴权需要 API Key".into());
    }
    Ok("豆包配置字段完整，可用历史记录里的重新转写做真实 ASR 测试".into())
}

#[tauri::command]
async fn test_openrouter(settings: AppSettings) -> Result<String, String> {
    services::test_openrouter(&settings).await
}

#[tauri::command]
async fn debug_transcribe_file(app: AppHandle, path: String) -> Result<String, String> {
    let settings = storage::get_settings(&app)?;
    let (text, _, _) = services::transcribe_audio(&settings, &PathBuf::from(path)).await?;
    Ok(text)
}

#[tauri::command]
fn get_overlay_state(state: State<'_, AppState>) -> Result<OverlayState, String> {
    state
        .overlay
        .lock()
        .map_err(|_| "无法读取 overlay 状态".to_string())
        .map(|state| state.clone())
}

#[tauri::command]
fn start_recording(app: AppHandle, state: State<'_, AppState>) -> Result<RecordingSession, String> {
    let settings = storage::get_settings(&app)?;
    let recorder = AudioRecorder::start(Some(&settings.microphone_name))?;
    let session = RecordingSession {
        active: true,
        started_at: Some(Utc::now().to_rfc3339()),
        status: "recording".into(),
        elapsed_ms: 0,
    };

    *state
        .recorder
        .lock()
        .map_err(|_| "无法更新录音器状态".to_string())? = Some(recorder);
    *state
        .recording
        .lock()
        .map_err(|_| "无法更新录音状态".to_string())? = session.clone();

    show_overlay(&app, "recording", "直接说", 0)?;
    start_overlay_level_loop(app.clone());
    storage::append_log(&app, "recording started");
    app.emit("recording-state", &session)
        .map_err(|error| error.to_string())?;

    Ok(session)
}

#[tauri::command]
async fn stop_recording(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<SpeechRecord, String> {
    set_session_status(&app, &state, "saving")?;
    show_overlay(&app, "saving", "正在保存录音", 0)?;

    let recorder = state
        .recorder
        .lock()
        .map_err(|_| "无法读取录音器状态".to_string())?
        .take()
        .ok_or_else(|| "当前没有正在进行的录音".to_string())?;

    let settings = storage::get_settings(&app)?;
    let record_id = Uuid::new_v4().to_string();
    let audio_path = storage::recording_path(&app, &record_id)?;
    let audio = recorder.stop()?;
    audio.save_wav(&audio_path)?;
    storage::append_log(&app, &format!("recording saved: {}", audio_path.display()));

    let now = Utc::now();
    let mut record = SpeechRecord {
        id: record_id,
        created_at: now.to_rfc3339(),
        updated_at: now.to_rfc3339(),
        raw_asr_text: String::new(),
        final_text: String::new(),
        audio_path: Some(path_to_string(&audio_path)),
        duration_ms: Some(audio.duration_ms()),
        audio_expires_at: Some(
            (now + Duration::days(settings.recording_retention_days)).to_rfc3339(),
        ),
        asr_status: "pending".into(),
        optimize_status: "pending".into(),
        copied_at: None,
        pasted_at: None,
        error_message: None,
        doubao_request_id: None,
        doubao_log_id: None,
        openrouter_model: Some(settings.openrouter_model.clone()),
    };
    record = storage::upsert_record(&app, record)?;
    app.emit("record-updated", &record)
        .map_err(|error| error.to_string())?;

    set_session_status(&app, &state, "transcribing")?;
    show_overlay(&app, "transcribing", "文字转写中", 0)?;
    record = match transcribe_record(app.clone(), record).await {
        Ok(record) => record,
        Err(record) => {
            reset_recording_state(&app, &state)?;
            hide_overlay_later(app.clone());
            return Ok(record);
        }
    };

    set_session_status(&app, &state, "optimizing")?;
    show_overlay(&app, "optimizing", "内容优化中", 0)?;
    record = optimize_record(app.clone(), record).await;

    reset_recording_state(&app, &state)?;
    hide_overlay_later(app.clone());
    Ok(record)
}

#[tauri::command]
async fn retry_asr(app: AppHandle, id: String) -> Result<SpeechRecord, String> {
    let record = storage::find_record(&app, &id)?;
    show_overlay(&app, "transcribing", "文字转写中", 0)?;
    let record = match transcribe_record(app.clone(), record).await {
        Ok(record) | Err(record) => record,
    };
    hide_overlay_later(app.clone());
    Ok(record)
}

#[tauri::command]
async fn retry_optimize(app: AppHandle, id: String) -> Result<SpeechRecord, String> {
    let record = storage::find_record(&app, &id)?;
    show_overlay(&app, "optimizing", "内容优化中", 0)?;
    let record = optimize_record(app.clone(), record).await;
    hide_overlay_later(app.clone());
    Ok(record)
}

async fn transcribe_record(
    app: AppHandle,
    mut record: SpeechRecord,
) -> Result<SpeechRecord, SpeechRecord> {
    let settings = storage::get_settings(&app).unwrap_or_default();
    let Some(audio_path) = record.audio_path.clone() else {
        record.asr_status = "failed".into();
        record.error_message = Some("这条记录没有可用录音文件".into());
        let _ = storage::upsert_record(&app, record.clone());
        let _ = app.emit("record-updated", &record);
        return Err(record);
    };

    match services::transcribe_audio(&settings, &PathBuf::from(audio_path)).await {
        Ok((text, log_id, request_id)) => {
            if text.trim().is_empty() {
                record.raw_asr_text = String::new();
                record.final_text = "没有录音".into();
                record.asr_status = "no_speech".into();
                record.optimize_status = "blocked".into();
                record.error_message = None;
                record.doubao_log_id = log_id;
                record.doubao_request_id = Some(request_id);
                record.updated_at = Utc::now().to_rfc3339();
                let _ = storage::upsert_record(&app, record.clone());
                let _ = app.emit("record-updated", &record);
                return Err(record);
            }
            record.raw_asr_text = text;
            record.asr_status = "completed".into();
            record.optimize_status = "pending".into();
            record.error_message = None;
            record.doubao_log_id = log_id;
            record.doubao_request_id = Some(request_id);
        }
        Err(error) => {
            record.asr_status = "failed".into();
            record.optimize_status = "blocked".into();
            record.error_message = Some(format!("录音已保存，转写失败：{error}"));
            storage::append_log(&app, &format!("asr failed: {error}"));
            record.updated_at = Utc::now().to_rfc3339();
            let _ = storage::upsert_record(&app, record.clone());
            let _ = app.emit("record-updated", &record);
            return Err(record);
        }
    }

    record.updated_at = Utc::now().to_rfc3339();
    let record = storage::upsert_record(&app, record.clone()).unwrap_or(record);
    let _ = app.emit("record-updated", &record);
    Ok(record)
}

async fn optimize_record(app: AppHandle, mut record: SpeechRecord) -> SpeechRecord {
    let settings = storage::get_settings(&app).unwrap_or_default();
    let prompts = storage::get_prompts(&app).unwrap_or_default();

    if record.raw_asr_text.trim().is_empty() {
        record.optimize_status = "blocked".into();
        record.error_message = Some("没有可用于优化的 ASR 文本".into());
    } else {
        match services::optimize_text(&settings, &prompts, &record.raw_asr_text).await {
            Ok(text) => {
                record.final_text = text;
                record.optimize_status = "completed".into();
                record.error_message = None;
                record.openrouter_model = Some(settings.openrouter_model.clone());
                if copy_to_clipboard(&record.final_text).is_ok() {
                    record.copied_at = Some(Utc::now().to_rfc3339());
                    if settings.auto_paste && paste_from_clipboard().is_ok() {
                        record.pasted_at = Some(Utc::now().to_rfc3339());
                    }
                }
            }
            Err(error) => {
                record.optimize_status = "failed".into();
                record.error_message = Some(format!("转写已保存，内容优化失败：{error}"));
                storage::append_log(&app, &format!("optimize failed: {error}"));
            }
        }
    }

    record.updated_at = Utc::now().to_rfc3339();
    let record = storage::upsert_record(&app, record.clone()).unwrap_or(record);
    let _ = app.emit("record-updated", &record);
    record
}

fn register_shortcut(app: &AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let settings = storage::get_settings(app)?;
    let mut current = state
        .shortcut
        .lock()
        .map_err(|_| "无法更新快捷键状态".to_string())?;
    if let Some(existing) = current.take() {
        existing.stop();
    }
    *current = Some(shortcut::register(app.clone(), &settings.global_shortcut)?);
    Ok(())
}

fn set_session_status(
    app: &AppHandle,
    state: &State<'_, AppState>,
    status: &str,
) -> Result<(), String> {
    let mut session = state
        .recording
        .lock()
        .map_err(|_| "无法更新录音状态".to_string())?;
    session.status = status.to_string();
    app.emit("recording-state", session.clone())
        .map_err(|error| error.to_string())
}

fn reset_recording_state(app: &AppHandle, state: &State<'_, AppState>) -> Result<(), String> {
    let session = RecordingSession::default();
    *state
        .recording
        .lock()
        .map_err(|_| "无法更新录音状态".to_string())? = session.clone();
    app.emit("recording-state", session)
        .map_err(|error| error.to_string())
}

fn show_overlay(app: &AppHandle, phase: &str, label: &str, elapsed_ms: u64) -> Result<(), String> {
    let state = OverlayState {
        visible: true,
        phase: phase.into(),
        label: label.into(),
        elapsed_ms,
        input_level: 0.0,
    };
    if let Some(app_state) = app.try_state::<AppState>() {
        if let Ok(mut overlay_state) = app_state.overlay.lock() {
            *overlay_state = state.clone();
        }
    }

    if let Some(overlay) = app.get_webview_window("overlay") {
        position_overlay(&overlay)?;
        overlay.show().map_err(|error| error.to_string())?;
        overlay
            .emit("overlay-state", state.clone())
            .map_err(|error| error.to_string())?;

        let overlay_for_retry = overlay.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            let _ = overlay_for_retry.emit("overlay-state", state);
        });
    }
    Ok(())
}

fn start_overlay_level_loop(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(90)).await;
            let Some(app_state) = app.try_state::<AppState>() else {
                break;
            };

            let recording_status = app_state
                .recording
                .lock()
                .map(|session| session.status.clone())
                .unwrap_or_default();
            if recording_status != "recording" {
                break;
            }

            let input_level = app_state
                .recorder
                .lock()
                .ok()
                .and_then(|recorder| recorder.as_ref().map(|recorder| recorder.input_level()))
                .unwrap_or(0.0);

            let state = OverlayState {
                visible: true,
                phase: "recording".into(),
                label: "直接说".into(),
                elapsed_ms: 0,
                input_level,
            };

            if let Ok(mut overlay_state) = app_state.overlay.lock() {
                *overlay_state = state.clone();
            }
            if let Some(overlay) = app.get_webview_window("overlay") {
                let _ = overlay.emit("overlay-state", state);
            }
        }
    });
}

fn hide_overlay_later(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(900)).await;
        let state = OverlayState::default();
        if let Some(app_state) = app.try_state::<AppState>() {
            if let Ok(mut overlay_state) = app_state.overlay.lock() {
                *overlay_state = state.clone();
            }
        }
        if let Some(overlay) = app.get_webview_window("overlay") {
            let _ = overlay.emit("overlay-state", state);
            let _ = overlay.hide();
        }
    });
}

fn position_overlay(window: &WebviewWindow) -> Result<(), String> {
    if let Some(monitor) = window
        .current_monitor()
        .map_err(|error| error.to_string())?
    {
        let size = monitor.size();
        let scale = monitor.scale_factor();
        let width = 260.0;
        let height = 60.0;
        let x = (size.width as f64 / scale - width) / 2.0;
        let y = size.height as f64 / scale - height - 36.0;
        window
            .set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }))
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn copy_to_clipboard(text: &str) -> Result<(), String> {
    let mut clipboard = Clipboard::new().map_err(|error| error.to_string())?;
    clipboard
        .set_text(text.to_string())
        .map_err(|error| error.to_string())
}

fn paste_from_clipboard() -> Result<(), String> {
    let inputs = [
        keyboard_input(VK_CONTROL, KEYBD_EVENT_FLAGS(0)),
        keyboard_input(VK_V, KEYBD_EVENT_FLAGS(0)),
        keyboard_input(VK_V, KEYEVENTF_KEYUP),
        keyboard_input(VK_CONTROL, KEYEVENTF_KEYUP),
    ];
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent == inputs.len() as u32 {
        Ok(())
    } else {
        Err("自动粘贴失败".into())
    }
}

fn keyboard_input(key: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn path_to_string(path: &std::path::Path) -> String {
    path.to_string_lossy().to_string()
}
