#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            get_bootstrap,
            get_app_version,
            save_settings,
            save_prompt_settings,
            list_records,
            list_records_page,
            delete_record,
            copy_text,
            start_recording,
            stop_recording,
            import_audio_file,
            retry_asr,
            retry_optimize,
            read_audio_data_url,
            open_audio_folder,
            open_main_window,
            reconnect_realtime_asr,
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
            if let Err(error) = storage::ensure_daily_backup(&handle) {
                storage::append_log(&handle, &format!("每日备份失败：{error}"));
            }
            match storage::recover_interrupted_recording_sessions(&handle) {
                Ok(records) => {
                    if !records.is_empty() {
                        storage::append_log(
                            &handle,
                            &format!(
                                "启动时恢复了 {} 条未完成录音，已生成历史记录。",
                                records.len()
                            ),
                        );
                    }
                }
                Err(error) => {
                    storage::append_log(&handle, &format!("启动时恢复未完成录音失败：{error}"));
                }
            }
            match storage::cleanup_expired_recording_files(&handle) {
                Ok(count) => {
                    if count > 0 {
                        storage::append_log(
                            &handle,
                            &format!("启动时清理了 {count} 个已过期录音文件，文字历史已保留。"),
                        );
                    }
                }
                Err(error) => {
                    storage::append_log(&handle, &format!("启动时清理过期录音失败：{error}"));
                }
            }
            register_shortcut(&handle, app.state::<AppState>())?;
            create_tray(app)?;

            if let Some(overlay) = app.get_webview_window("overlay") {
                position_overlay(&overlay, false)?;
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
use base64::{engine::general_purpose, Engine as _};
use chrono::{DateTime, Duration, Utc};
use models::{
    AppSettings, BootstrapData, OverlayState, PromptSettings, RealtimeTranscriptSegment,
    RecordPage, RecordingSession, SpeechRecord,
};
use recorder::AudioRecorder;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Emitter, Manager, State, WebviewWindow,
};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_CONTROL, VK_V,
};

#[derive(Default)]
struct AppState {
    recording: Mutex<RecordingSession>,
    recorder: Mutex<Option<AudioRecorder>>,
    active_session_id: Mutex<Option<String>>,
    realtime_transcript: Mutex<Vec<String>>,
    realtime_text: Mutex<String>,
    realtime_running: Mutex<bool>,
    realtime_failed: Mutex<bool>,
    realtime_sender: Mutex<Option<mpsc::Sender<services::RealtimeAudioChunk>>>,
    realtime_completion:
        Mutex<Option<oneshot::Receiver<Result<services::RealtimeTranscriptionResult, String>>>>,
    realtime_checkpoint: Mutex<usize>,
    realtime_enqueued_checkpoint: Mutex<usize>,
    realtime_sent_ms: Mutex<u64>,
    shortcut: Mutex<Option<shortcut::ShortcutHandle>>,
    overlay: Mutex<OverlayState>,
}

enum FastAsrOutcome {
    Completed(SpeechRecord),
    NoSpeech(SpeechRecord),
    Fallback(SpeechRecord),
}

const FAST_ASR_FINAL_WAIT_MS: u64 = 3000;
const OVERLAY_WINDOW_WIDTH: f64 = 520.0;
const OVERLAY_WINDOW_HEIGHT: f64 = 132.0;
const OVERLAY_BOTTOM_MARGIN: f64 = 72.0;

fn create_tray(app: &App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open-main", "打开主界面", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出应用", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut builder = TrayIconBuilder::with_id("sparkspeech")
        .tooltip("SparkSpeech")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = show_main_window(tray.app_handle());
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open-main" => {
                let _ = show_main_window(app);
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
fn get_app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
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
    sync_launch_at_startup(&app, settings.launch_at_startup)?;
    storage::save_settings(&app, &settings)?;
    storage::append_log(
        &app,
        &format!(
            "设置已保存：自动粘贴={}，保存日志={}，开机自启动={}，录音保留={} 天，片段间隔={} 秒，快速转写={}。",
            if settings.auto_paste {
                "开启"
            } else {
                "关闭"
            },
            if settings.save_logs {
                "开启"
            } else {
                "关闭"
            },
            if settings.launch_at_startup {
                "开启"
            } else {
                "关闭"
            },
            settings.recording_retention_days,
            recording_segment_seconds(&settings),
            if settings.fast_asr_finalize {
                "开启"
            } else {
                "关闭"
            }
        ),
    );
    register_shortcut(&app, state)?;
    Ok(settings)
}

#[cfg(target_os = "windows")]
fn sync_launch_at_startup(app: &AppHandle, enabled: bool) -> Result<(), String> {
    use std::io::ErrorKind;
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE_NAME: &str = "SparkSpeech";

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (run_key, _) = hkcu
        .create_subkey(RUN_KEY)
        .map_err(|error| format!("无法打开 Windows 开机自启动设置：{error}"))?;

    if enabled {
        let exe_path =
            std::env::current_exe().map_err(|error| format!("无法读取当前程序路径：{error}"))?;
        let command = format!("\"{}\"", exe_path.display());
        run_key
            .set_value(VALUE_NAME, &command)
            .map_err(|error| format!("无法写入开机自启动设置：{error}"))?;
        storage::append_log(app, &format!("开机自启动已开启：命令={}。", command));
        return Ok(());
    }

    match run_key.delete_value(VALUE_NAME) {
        Ok(_) => {
            storage::append_log(app, "开机自启动已关闭。");
            Ok(())
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("无法关闭开机自启动：{error}")),
    }
}

#[cfg(not(target_os = "windows"))]
fn sync_launch_at_startup(_app: &AppHandle, _enabled: bool) -> Result<(), String> {
    Ok(())
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
fn read_audio_data_url(path: String) -> Result<String, String> {
    let path = PathBuf::from(path);
    if !path.exists() {
        return Err("录音文件不存在".into());
    }
    let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
    Ok(format!(
        "data:audio/wav;base64,{}",
        general_purpose::STANDARD.encode(bytes)
    ))
}

#[tauri::command]
fn open_audio_folder(path: String) -> Result<bool, String> {
    let path = PathBuf::from(path);
    if !path.exists() {
        return Err("录音文件不存在".into());
    }
    std::process::Command::new("explorer.exe")
        .arg(format!("/select,{}", path.display()))
        .spawn()
        .map_err(|error| error.to_string())?;
    Ok(true)
}

#[tauri::command]
fn open_main_window(app: AppHandle) -> Result<bool, String> {
    show_main_window(&app)?;
    Ok(true)
}

#[tauri::command]
fn reconnect_realtime_asr(app: AppHandle, state: State<'_, AppState>) -> Result<bool, String> {
    let recording_status = state
        .recording
        .lock()
        .map_err(|_| "无法读取录音状态".to_string())?
        .status
        .clone();
    if recording_status != "recording" {
        return Err("当前没有正在录音".into());
    }
    if *state
        .realtime_running
        .lock()
        .map_err(|_| "无法读取实时转写状态".to_string())?
    {
        return Ok(true);
    }

    let checkpoint = *state
        .realtime_checkpoint
        .lock()
        .map_err(|_| "无法读取实时转写位置".to_string())?;
    let overlap = state
        .recorder
        .lock()
        .ok()
        .and_then(|recorder| {
            recorder
                .as_ref()
                .map(|recorder| recorder.sample_count_for_ms(1200))
        })
        .unwrap_or(0);
    let start_index = checkpoint.saturating_sub(overlap);
    start_realtime_asr_loop(app, start_index, true)?;
    Ok(true)
}

fn show_main_window(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.show().map_err(|error| error.to_string())?;
        window.set_focus().map_err(|error| error.to_string())?;
        let overlay_state = OverlayState::default();
        if let Some(app_state) = app.try_state::<AppState>() {
            if let Ok(mut state) = app_state.overlay.lock() {
                *state = overlay_state.clone();
            }
        }
        if let Some(overlay) = app.get_webview_window("overlay") {
            let _ = overlay.emit("overlay-state", overlay_state);
            let _ = overlay.hide();
        }
        return Ok(());
    }
    Err("主窗口不可用".into())
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
    let session_id = Uuid::new_v4().to_string();
    let started_at = Utc::now().to_rfc3339();
    storage::create_recording_file_session(&app, &session_id, &started_at)?;
    let session = RecordingSession {
        active: true,
        started_at: Some(started_at),
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
    *state
        .active_session_id
        .lock()
        .map_err(|_| "无法更新录音文件会话".to_string())? = Some(session_id.clone());
    state
        .realtime_transcript
        .lock()
        .map_err(|_| "无法更新实时转写缓存".to_string())?
        .clear();
    state
        .realtime_text
        .lock()
        .map_err(|_| "无法更新实时转写文本".to_string())?
        .clear();
    *state
        .realtime_failed
        .lock()
        .map_err(|_| "无法更新实时转写状态".to_string())? = false;
    *state
        .realtime_sender
        .lock()
        .map_err(|_| "无法更新实时转写通道".to_string())? = None;
    *state
        .realtime_completion
        .lock()
        .map_err(|_| "无法更新实时转写完成状态".to_string())? = None;
    *state
        .realtime_checkpoint
        .lock()
        .map_err(|_| "无法更新实时转写位置".to_string())? = 0;
    *state
        .realtime_enqueued_checkpoint
        .lock()
        .map_err(|_| "无法更新实时转写队列位置".to_string())? = 0;
    *state
        .realtime_sent_ms
        .lock()
        .map_err(|_| "无法更新实时转写进度".to_string())? = 0;
    *state
        .realtime_running
        .lock()
        .map_err(|_| "无法更新实时转写状态".to_string())? = false;

    show_overlay(&app, "recording", "直接说", 0)?;
    start_overlay_level_loop(app.clone());
    start_recording_segment_loop(app.clone(), session_id.clone());
    if settings.fast_asr_finalize || settings.show_realtime_transcript {
        if let Err(error) = start_realtime_asr_loop(app.clone(), 0, false) {
            storage::append_log(
                &app,
                &format!("实时转写启动失败：session={}，错误={}。", session_id, error),
            );
        }
    }
    storage::append_log(
        &app,
        &format!(
            "开始录音：session={}，麦克风={}。",
            session_id,
            if settings.microphone_name.trim().is_empty() {
                "系统默认"
            } else {
                settings.microphone_name.as_str()
            }
        ),
    );
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

    let settings = storage::get_settings(&app)?;
    let fast_asr_upload_started = if settings.fast_asr_finalize {
        finish_realtime_audio_upload(&app, &state).await
    } else {
        false
    };

    let recorder = state
        .recorder
        .lock()
        .map_err(|_| "无法读取录音器状态".to_string())?
        .take()
        .ok_or_else(|| "当前没有正在进行的录音".to_string())?;
    let captured_duration_ms = recorder.captured_duration_ms().ok();

    let active_session_id = state
        .active_session_id
        .lock()
        .map_err(|_| "无法读取录音文件会话".to_string())?
        .clone();
    let record_id = active_session_id
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let audio_path = storage::recording_path(&app, &record_id)?;
    *state
        .active_session_id
        .lock()
        .map_err(|_| "无法更新录音文件会话".to_string())? = None;

    let now = Utc::now();
    let mut record = SpeechRecord {
        id: record_id.clone(),
        created_at: now.to_rfc3339(),
        updated_at: now.to_rfc3339(),
        raw_asr_text: String::new(),
        final_text: String::new(),
        audio_path: None,
        duration_ms: captured_duration_ms,
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
        openrouter_model: Some(active_text_model_name(&settings)),
    };
    record = storage::upsert_record(&app, record)?;
    app.emit("record-updated", &record)
        .map_err(|error| error.to_string())?;

    let save_audio_path = audio_path.clone();
    let save_task = tauri::async_runtime::spawn_blocking(move || -> Result<u64, String> {
        let audio = recorder.stop()?;
        let duration_ms = audio.duration_ms();
        audio.save_wav(&save_audio_path)?;
        Ok(duration_ms)
    });

    set_session_status(&app, &state, "transcribing")?;
    show_overlay(&app, "transcribing", "文字转写中", 0)?;
    record =
        await_audio_save_for_transcription(&app, record, active_session_id, &audio_path, save_task)
            .await;
    if record.audio_path.is_none() {
        reset_recording_state(&app, &state)?;
        finish_overlay_after_record(app.clone(), &record);
        return Ok(record);
    }

    let mut needs_full_audio_transcription = !settings.fast_asr_finalize;
    if settings.fast_asr_finalize {
        match apply_fast_asr_result(app.clone(), record, fast_asr_upload_started).await {
            FastAsrOutcome::Completed(next) => {
                record = next;
            }
            FastAsrOutcome::NoSpeech(next) => {
                reset_recording_state(&app, &state)?;
                finish_overlay_after_record(app.clone(), &next);
                return Ok(next);
            }
            FastAsrOutcome::Fallback(next) => {
                record = next;
                needs_full_audio_transcription = true;
            }
        }
    }

    if needs_full_audio_transcription {
        record = match transcribe_record(app.clone(), record).await {
            Ok(record) => record,
            Err(record) => {
                reset_recording_state(&app, &state)?;
                finish_overlay_after_record(app.clone(), &record);
                return Ok(record);
            }
        }
    }

    set_session_status(&app, &state, "optimizing")?;
    show_overlay(&app, "optimizing", "内容优化中", 0)?;
    record = optimize_record(app.clone(), record).await;

    reset_recording_state(&app, &state)?;
    finish_overlay_after_record(app.clone(), &record);
    Ok(record)
}

async fn await_audio_save_for_transcription(
    app: &AppHandle,
    record: SpeechRecord,
    active_session_id: Option<String>,
    audio_path: &PathBuf,
    save_task: tauri::async_runtime::JoinHandle<Result<u64, String>>,
) -> SpeechRecord {
    match save_task.await {
        Ok(Ok(duration_ms)) => match apply_audio_save_success(
            app,
            &record.id,
            active_session_id,
            audio_path,
            duration_ms,
        ) {
            Ok(record) => record,
            Err(error) => apply_audio_save_failure(app, record, error),
        },
        Ok(Err(error)) => apply_audio_save_failure(app, record, error),
        Err(error) => apply_audio_save_failure(app, record, error.to_string()),
    }
}

fn apply_audio_save_success(
    app: &AppHandle,
    record_id: &str,
    active_session_id: Option<String>,
    audio_path: &PathBuf,
    duration_ms: u64,
) -> Result<SpeechRecord, String> {
    storage::append_log(
        app,
        &format!(
            "录音已保存：record_id={}，时长={}，文件={}",
            record_id,
            format_duration_for_log(duration_ms),
            audio_path.display()
        ),
    );

    let mut record = storage::find_record(app, record_id)?;
    record.audio_path = Some(path_to_string(audio_path));
    record.duration_ms = Some(duration_ms);
    record.updated_at = Utc::now().to_rfc3339();
    let record = storage::upsert_record(app, record)?;
    let _ = app.emit("record-updated", &record);

    if let Some(session_id) = active_session_id {
        match storage::finish_recording_file_session(app, &session_id, audio_path) {
            Ok(()) => {
                if let Err(error) = storage::remove_recording_file_session(app, &session_id) {
                    storage::append_log(
                        app,
                        &format!("录音分段清理失败：session={session_id}，错误={error}"),
                    );
                } else {
                    storage::append_log(app, &format!("录音分段已清理：session={session_id}。"));
                }
            }
            Err(error) => storage::append_log(
                app,
                &format!("录音分段完成标记失败：session={session_id}，错误={error}"),
            ),
        }
    }

    Ok(record)
}

fn apply_audio_save_failure(
    app: &AppHandle,
    mut record: SpeechRecord,
    error: String,
) -> SpeechRecord {
    storage::append_log(
        app,
        &format!("录音保存失败：record_id={}，错误={}。", record.id, error),
    );
    record.audio_path = None;
    record.duration_ms = None;
    record.asr_status = "failed".into();
    record.optimize_status = "blocked".into();
    record.error_message = Some(format!("录音保存失败：{error}"));
    record.updated_at = Utc::now().to_rfc3339();
    let record = storage::upsert_record(app, record.clone()).unwrap_or(record);
    let _ = app.emit("record-updated", &record);
    record
}

async fn finish_realtime_audio_upload(app: &AppHandle, state: &State<'_, AppState>) -> bool {
    let failed = state
        .realtime_failed
        .lock()
        .map(|value| *value)
        .unwrap_or(true);
    if failed {
        return false;
    }

    let sender = state
        .realtime_sender
        .lock()
        .ok()
        .and_then(|mut sender| sender.take());
    let Some(sender) = sender else {
        return false;
    };

    let checkpoint = state
        .realtime_checkpoint
        .lock()
        .map(|value| *value)
        .unwrap_or(0);
    let enqueued_checkpoint = state
        .realtime_enqueued_checkpoint
        .lock()
        .map(|value| *value)
        .unwrap_or(0);
    let tail_start = checkpoint.max(enqueued_checkpoint);
    let final_segment = state
        .recorder
        .lock()
        .ok()
        .and_then(|recorder| {
            recorder
                .as_ref()
                .and_then(|recorder| recorder.segment_since(tail_start).ok())
        })
        .flatten();

    if let Some(segment) = final_segment {
        if let Err(error) = send_realtime_segment_chunks(&sender, segment, tail_start, 10).await {
            mark_realtime_asr_failed(app, &error);
            return false;
        }
    }

    drop(sender);
    storage::append_log(app, "快速转写已发送最后一段音频，等待豆包最终结果。");
    true
}

async fn send_realtime_segment_chunks(
    sender: &mpsc::Sender<services::RealtimeAudioChunk>,
    segment: recorder::RecordedSegment,
    base_sample_index: usize,
    send_delay_ms: u64,
) -> Result<(), String> {
    let chunk_len = 16_000 * 200 / 1000;
    let total_samples = segment.audio.pcm_16k.len();
    if total_samples == 0 {
        return Ok(());
    }

    let mut sent_samples = 0_usize;
    for chunk in segment.audio.pcm_16k.chunks(chunk_len) {
        sent_samples += chunk.len();
        let is_last_chunk = sent_samples >= total_samples;
        let end_ms = if is_last_chunk {
            segment.end_ms
        } else {
            segment.start_ms + (sent_samples as u64 * 1000) / 16_000
        };
        let next_sample_index = if is_last_chunk {
            segment.next_sample_index
        } else {
            base_sample_index
        };
        sender
            .send(services::RealtimeAudioChunk {
                pcm_16k: chunk.to_vec(),
                next_sample_index,
                end_ms,
                send_delay_ms,
            })
            .await
            .map_err(|_| "快速转写通道已关闭".to_string())?;
    }
    Ok(())
}

async fn apply_fast_asr_result(
    app: AppHandle,
    record: SpeechRecord,
    upload_started: bool,
) -> FastAsrOutcome {
    if !upload_started {
        storage::append_log(
            &app,
            &format!(
                "快速转写未完成上传：record_id={}，改用完整音频转写。",
                record.id
            ),
        );
        return FastAsrOutcome::Fallback(record);
    }

    let Some(app_state) = app.try_state::<AppState>() else {
        return FastAsrOutcome::Fallback(record);
    };
    let completion = app_state
        .realtime_completion
        .lock()
        .ok()
        .and_then(|mut completion| completion.take());
    let Some(completion) = completion else {
        storage::append_log(
            &app,
            &format!(
                "快速转写没有完成通知：record_id={}，改用完整音频转写。",
                record.id
            ),
        );
        return FastAsrOutcome::Fallback(record);
    };

    if let Some(total_ms) = record.duration_ms {
        let sent_ms = app_state
            .realtime_sent_ms
            .lock()
            .map(|value| *value)
            .unwrap_or(0);
        update_overlay_progress(&app, "transcribing", sent_ms.min(total_ms), total_ms);
    }

    let result = tokio::time::timeout(
        std::time::Duration::from_millis(FAST_ASR_FINAL_WAIT_MS),
        completion,
    )
    .await;
    let result = match result {
        Ok(Ok(Ok(result))) => result,
        Ok(Ok(Err(error))) => {
            storage::append_log(
                &app,
                &format!(
                    "快速转写最终结果失败：record_id={}，改用完整音频转写。错误={}。",
                    record.id, error
                ),
            );
            return FastAsrOutcome::Fallback(record);
        }
        Ok(Err(_)) => {
            storage::append_log(
                &app,
                &format!(
                    "快速转写任务提前结束：record_id={}，改用完整音频转写。",
                    record.id
                ),
            );
            return FastAsrOutcome::Fallback(record);
        }
        Err(_) => {
            storage::append_log(
                &app,
                &format!(
                    "快速转写等待超时：record_id={}，改用完整音频转写。",
                    record.id
                ),
            );
            return FastAsrOutcome::Fallback(record);
        }
    };

    if let Some(total_ms) = record.duration_ms {
        update_overlay_progress(&app, "transcribing", total_ms, total_ms);
    }

    let text = result.text.trim().to_string();
    complete_fast_asr_record(app, record, text, result.log_id, Some(result.request_id))
}

fn complete_fast_asr_record(
    app: AppHandle,
    mut record: SpeechRecord,
    text: String,
    log_id: Option<String>,
    request_id: Option<String>,
) -> FastAsrOutcome {
    if let Some(total_ms) = record.duration_ms {
        update_overlay_progress(&app, "transcribing", total_ms, total_ms);
    }

    record.doubao_log_id = log_id;
    record.doubao_request_id = request_id;
    if text.is_empty() {
        record.raw_asr_text = String::new();
        record.final_text = "没有录音".into();
        record.asr_status = "no_speech".into();
        record.optimize_status = "blocked".into();
        record.error_message = None;
        record.updated_at = Utc::now().to_rfc3339();
        let _ = storage::upsert_record(&app, record.clone());
        let _ = app.emit("record-updated", &record);
        storage::append_log(
            &app,
            &format!(
                "快速转写完成：record_id={}，结果为空，按 no_speech 保存。",
                record.id
            ),
        );
        return FastAsrOutcome::NoSpeech(record);
    }

    let asr_chars = count_text_chars(&text);
    record.raw_asr_text = text;
    record.asr_status = "completed".into();
    record.optimize_status = "pending".into();
    record.error_message = None;
    record.updated_at = Utc::now().to_rfc3339();
    let record = storage::upsert_record(&app, record.clone()).unwrap_or(record);
    let _ = app.emit("record-updated", &record);
    storage::append_log(
        &app,
        &format!(
            "快速转写完成：record_id={}，原始文本 {} 字。",
            record.id, asr_chars
        ),
    );
    FastAsrOutcome::Completed(record)
}

#[tauri::command]
async fn import_audio_file(app: AppHandle, path: String) -> Result<SpeechRecord, String> {
    let source_path = PathBuf::from(path);
    if !source_path.exists() || !source_path.is_file() {
        return Err("音频文件不存在".into());
    }
    let extension = source_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension != "wav" {
        return Err("目前拖拽导入支持 WAV 音频".into());
    }

    let settings = storage::get_settings(&app)?;
    let record_id = Uuid::new_v4().to_string();
    let audio_path = storage::recording_path(&app, &record_id)?;
    std::fs::copy(&source_path, &audio_path).map_err(|error| error.to_string())?;
    let duration_ms = recorder::read_wav_pcm_16k(&audio_path)
        .map(|pcm| (pcm.len() as u64 * 1000) / 16_000)
        .map_err(|error| {
            let _ = std::fs::remove_file(&audio_path);
            format!("无法读取 WAV 音频：{error}")
        })?;
    storage::append_log(
        &app,
        &format!(
            "audio imported: {} -> {}",
            source_path.display(),
            audio_path.display()
        ),
    );

    let now = Utc::now();
    let mut record = SpeechRecord {
        id: record_id,
        created_at: now.to_rfc3339(),
        updated_at: now.to_rfc3339(),
        raw_asr_text: String::new(),
        final_text: String::new(),
        audio_path: Some(path_to_string(&audio_path)),
        duration_ms: Some(duration_ms),
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
        openrouter_model: Some(active_text_model_name(&settings)),
    };
    record = storage::upsert_record(&app, record)?;
    app.emit("record-updated", &record)
        .map_err(|error| error.to_string())?;

    show_overlay(&app, "transcribing", "文字转写中", 0)?;
    record = match transcribe_record(app.clone(), record).await {
        Ok(record) => record,
        Err(record) => {
            finish_overlay_after_record(app.clone(), &record);
            return Ok(record);
        }
    };

    show_overlay(&app, "optimizing", "内容优化中", 0)?;
    record = optimize_record(app.clone(), record).await;
    finish_overlay_after_record(app.clone(), &record);
    Ok(record)
}

#[tauri::command]
async fn retry_asr(app: AppHandle, id: String) -> Result<SpeechRecord, String> {
    let record = storage::find_record(&app, &id)?;
    show_overlay(&app, "transcribing", "文字转写中", 0)?;
    let record = match transcribe_record(app.clone(), record).await {
        Ok(record) | Err(record) => record,
    };
    finish_overlay_after_record(app.clone(), &record);
    Ok(record)
}

#[tauri::command]
async fn retry_optimize(app: AppHandle, id: String) -> Result<SpeechRecord, String> {
    let record = storage::find_record(&app, &id)?;
    show_overlay(&app, "optimizing", "内容优化中", 0)?;
    let record = optimize_record(app.clone(), record).await;
    finish_overlay_after_record(app.clone(), &record);
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
        storage::append_log(
            &app,
            &format!("准备转写失败：record_id={} 没有可用录音文件。", record.id),
        );
        let _ = storage::upsert_record(&app, record.clone());
        let _ = app.emit("record-updated", &record);
        return Err(record);
    };

    storage::append_log(
        &app,
        &format!(
            "准备调用豆包 ASR：record_id={}，音频时长={}，文件={}。",
            record.id,
            record
                .duration_ms
                .map(format_duration_for_log)
                .unwrap_or_else(|| "未知".into()),
            audio_path
        ),
    );

    let app_for_progress = app.clone();
    match services::transcribe_audio_with_progress(
        &settings,
        &PathBuf::from(audio_path),
        move |current_ms, total_ms| {
            let progress_cap = (total_ms.saturating_mul(96) / 100).max(1);
            update_overlay_progress(
                &app_for_progress,
                "transcribing",
                current_ms.min(progress_cap),
                total_ms,
            );
        },
    )
    .await
    {
        Ok((text, log_id, request_id)) => {
            if let Some(total_ms) = record.duration_ms {
                update_overlay_progress(&app, "transcribing", total_ms, total_ms);
            }
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
                storage::append_log(
                    &app,
                    &format!(
                        "豆包 ASR 完成：record_id={}，结果为空，按 no_speech 保存。request_id={}，log_id={}。",
                        record.id,
                        record.doubao_request_id.clone().unwrap_or_else(|| "无".into()),
                        record.doubao_log_id.clone().unwrap_or_else(|| "无".into())
                    ),
                );
                return Err(record);
            }
            let asr_chars = count_text_chars(&text);
            record.raw_asr_text = text;
            record.asr_status = "completed".into();
            record.optimize_status = "pending".into();
            record.error_message = None;
            record.doubao_log_id = log_id;
            record.doubao_request_id = Some(request_id);
            storage::append_log(
                &app,
                &format!(
                    "豆包 ASR 完成：record_id={}，原始文本 {} 字，request_id={}，log_id={}。",
                    record.id,
                    asr_chars,
                    record
                        .doubao_request_id
                        .clone()
                        .unwrap_or_else(|| "无".into()),
                    record.doubao_log_id.clone().unwrap_or_else(|| "无".into())
                ),
            );
        }
        Err(error) => {
            record.asr_status = "failed".into();
            record.optimize_status = "blocked".into();
            record.error_message = Some(format!("录音已保存，转写失败：{error}"));
            storage::append_log(
                &app,
                &format!(
                    "豆包 ASR 失败：record_id={}，录音已保留，错误={}。",
                    record.id, error
                ),
            );
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
        storage::append_log(
            &app,
            &format!("跳过文本优化：record_id={} 没有可用 ASR 文本。", record.id),
        );
    } else {
        let progress_total = count_text_chars(&record.raw_asr_text).max(1) as u64;
        storage::append_log(
            &app,
            &format!(
                "准备调用文本优化模型：record_id={}，provider={}，model={}，输入 {} 字。",
                record.id,
                active_text_provider_label(&settings),
                active_text_model_name(&settings),
                progress_total
            ),
        );
        let app_for_progress = app.clone();
        match services::optimize_text_streaming(
            &settings,
            &prompts,
            &record.raw_asr_text,
            move |text| {
                let current = count_text_chars(&text) as u64;
                let progress_cap = (progress_total.saturating_mul(95) / 100).max(1);
                let capped_current = current.min(progress_cap);
                update_overlay_progress(
                    &app_for_progress,
                    "optimizing",
                    capped_current,
                    progress_total,
                );
            },
        )
        .await
        {
            Ok(text) => {
                update_overlay_progress(&app, "optimizing", progress_total, progress_total);
                let output_chars = count_text_chars(&text);
                record.final_text = text;
                record.optimize_status = "completed".into();
                record.error_message = None;
                record.openrouter_model = Some(active_text_model_name(&settings));
                let copied = if copy_to_clipboard(&record.final_text).is_ok() {
                    record.copied_at = Some(Utc::now().to_rfc3339());
                    true
                } else {
                    false
                };
                let pasted = if copied && settings.auto_paste && paste_from_clipboard().is_ok() {
                    record.pasted_at = Some(Utc::now().to_rfc3339());
                    true
                } else {
                    false
                };
                storage::append_log(
                    &app,
                    &format!(
                        "文本优化完成：record_id={}，输出 {} 字，已复制={}，已自动粘贴={}。",
                        record.id,
                        output_chars,
                        if copied { "是" } else { "否" },
                        if pasted { "是" } else { "否" }
                    ),
                );
            }
            Err(error) => {
                record.optimize_status = "failed".into();
                record.error_message = Some(format!("转写已保存，内容优化失败：{error}"));
                storage::append_log(
                    &app,
                    &format!(
                        "文本优化失败：record_id={}，原始 ASR 已保存，错误={}。",
                        record.id, error
                    ),
                );
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
    if let Ok(mut sender) = state.realtime_sender.lock() {
        *sender = None;
    }
    if let Ok(mut completion) = state.realtime_completion.lock() {
        *completion = None;
    }
    if let Ok(mut failed) = state.realtime_failed.lock() {
        *failed = false;
    }
    if let Ok(mut sent_ms) = state.realtime_sent_ms.lock() {
        *sent_ms = 0;
    }
    if let Ok(mut enqueued_checkpoint) = state.realtime_enqueued_checkpoint.lock() {
        *enqueued_checkpoint = 0;
    }
    app.emit("recording-state", session)
        .map_err(|error| error.to_string())
}

fn show_overlay(app: &AppHandle, phase: &str, label: &str, elapsed_ms: u64) -> Result<(), String> {
    if should_defer_overlay_to_active_recording(app, phase) {
        return Ok(());
    }

    let state = OverlayState {
        visible: true,
        phase: phase.into(),
        label: label.into(),
        elapsed_ms,
        input_level: 0.0,
        action_label: None,
        status_kind: None,
        transcript_lines: Vec::new(),
        progress_current: None,
        progress_total: None,
        reconnect_available: false,
    };
    if let Some(app_state) = app.try_state::<AppState>() {
        if let Ok(mut overlay_state) = app_state.overlay.lock() {
            *overlay_state = state.clone();
        }
    }

    if let Some(overlay) = app.get_webview_window("overlay") {
        position_overlay(&overlay, should_expand_overlay(app, phase))?;
        overlay.show().map_err(|error| error.to_string())?;
        overlay
            .emit("overlay-state", state.clone())
            .map_err(|error| error.to_string())?;

        let overlay_for_retry = overlay.clone();
        let app_for_retry = app.clone();
        let state_for_retry = state;
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            let should_emit = app_for_retry
                .try_state::<AppState>()
                .and_then(|app_state| {
                    app_state.overlay.lock().ok().map(|current| {
                        current.phase == state_for_retry.phase
                            && current.label == state_for_retry.label
                            && current.status_kind == state_for_retry.status_kind
                            && current.progress_current == state_for_retry.progress_current
                            && current.progress_total == state_for_retry.progress_total
                    })
                })
                .unwrap_or(false);
            if should_emit {
                let _ = overlay_for_retry.emit("overlay-state", state_for_retry);
            }
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

            let recording_session = app_state
                .recording
                .lock()
                .map(|session| session.clone())
                .unwrap_or_default();
            if recording_session.status != "recording" {
                break;
            }
            let elapsed_ms = recording_session
                .started_at
                .as_deref()
                .and_then(|started_at| DateTime::parse_from_rfc3339(started_at).ok())
                .map(|started_at| {
                    Utc::now()
                        .signed_duration_since(started_at.with_timezone(&Utc))
                        .num_milliseconds()
                        .max(0) as u64
                })
                .unwrap_or(0);

            let input_level = app_state
                .recorder
                .lock()
                .ok()
                .and_then(|recorder| recorder.as_ref().map(|recorder| recorder.input_level()))
                .unwrap_or(0.0);
            let show_realtime_transcript = storage::get_settings(&app)
                .map(|settings| settings.show_realtime_transcript)
                .unwrap_or(true);
            let (status_kind, reconnect_available) = app_state
                .overlay
                .lock()
                .map(|state| {
                    (
                        if state.reconnect_available
                            || matches!(
                                state.status_kind.as_deref(),
                                Some("saved") | Some("network_error")
                            )
                        {
                            state.status_kind.clone()
                        } else {
                            Some("recording".into())
                        },
                        state.reconnect_available,
                    )
                })
                .unwrap_or_else(|_| (Some("recording".into()), false));

            let state = OverlayState {
                visible: true,
                phase: "recording".into(),
                label: "直接说".into(),
                elapsed_ms,
                input_level,
                action_label: None,
                status_kind,
                transcript_lines: if show_realtime_transcript {
                    overlay_state_transcript_lines(&app_state)
                } else {
                    Vec::new()
                },
                progress_current: None,
                progress_total: None,
                reconnect_available,
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

fn start_recording_segment_loop(app: AppHandle, session_id: String) {
    tauri::async_runtime::spawn(async move {
        let mut next_sample_index = 0_usize;
        let mut segment_index = 0_u32;
        let segment_seconds = storage::get_settings(&app)
            .map(|settings| recording_segment_seconds(&settings))
            .unwrap_or(10);

        loop {
            tokio::time::sleep(std::time::Duration::from_secs(segment_seconds)).await;
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

            let segment = app_state
                .recorder
                .lock()
                .ok()
                .and_then(|recorder| {
                    recorder
                        .as_ref()
                        .and_then(|recorder| recorder.segment_since(next_sample_index).ok())
                })
                .flatten();

            let Some(segment) = segment else {
                continue;
            };
            if segment.audio.duration_ms() < 1000 {
                continue;
            }

            let next_segment_index = segment_index + 1;
            match storage::append_recording_file_segment(
                &app,
                &session_id,
                next_segment_index,
                segment.start_ms,
                segment.end_ms,
                &segment.audio,
            ) {
                Ok(session_manifest) => {
                    segment_index = next_segment_index;
                    next_sample_index = segment.next_sample_index;
                    show_saved_tick(&app);
                    let segment_path = session_manifest
                        .segments
                        .iter()
                        .find(|item| item.index == next_segment_index)
                        .map(|item| item.path.as_str())
                        .unwrap_or("未知");
                    storage::append_log(
                        &app,
                        &format!(
                            "录音分段已保存：session={}，片段 #{}，范围 {}-{}，文件={}。",
                            session_id,
                            next_segment_index,
                            format_duration_for_log(segment.start_ms),
                            format_duration_for_log(segment.end_ms),
                            segment_path
                        ),
                    );
                }
                Err(error) => {
                    storage::append_log(
                        &app,
                        &format!("录音分段保存失败：session={}，错误={}。", session_id, error),
                    );
                }
            }
        }
    });
}

fn recording_segment_seconds(settings: &AppSettings) -> u64 {
    match settings.recording_segment_seconds {
        5 | 10 | 15 | 20 | 25 | 30 => settings.recording_segment_seconds,
        _ => 10,
    }
}

fn start_realtime_asr_loop(
    app: AppHandle,
    start_sample_index: usize,
    catchup: bool,
) -> Result<(), String> {
    let settings = storage::get_settings(&app)?;
    let Some(app_state) = app.try_state::<AppState>() else {
        return Err("应用状态不可用".into());
    };
    {
        let mut running = app_state
            .realtime_running
            .lock()
            .map_err(|_| "无法更新实时转写状态".to_string())?;
        if *running {
            return Ok(());
        }
        *running = true;
    }
    reset_realtime_overlay_status(&app);

    let (sender, receiver) = mpsc::channel::<services::RealtimeAudioChunk>(256);
    let (completion_sender, completion_receiver) =
        oneshot::channel::<Result<services::RealtimeTranscriptionResult, String>>();
    if let Ok(mut realtime_sender) = app_state.realtime_sender.lock() {
        *realtime_sender = Some(sender.clone());
    }
    if let Ok(mut realtime_completion) = app_state.realtime_completion.lock() {
        *realtime_completion = Some(completion_receiver);
    }
    start_realtime_audio_producer(app.clone(), sender, start_sample_index, catchup);

    tauri::async_runtime::spawn(async move {
        let send_delay = if catchup { 10 } else { 200 };
        let app_for_text = app.clone();
        let app_for_sent = app.clone();
        let result = services::transcribe_audio_stream(
            settings,
            receiver,
            send_delay,
            move |text, utterances| update_realtime_transcript(&app_for_text, &text, utterances),
            move |next_sample_index, end_ms| {
                if let Some(state) = app_for_sent.try_state::<AppState>() {
                    if let Ok(mut checkpoint) = state.realtime_checkpoint.lock() {
                        *checkpoint = next_sample_index;
                    }
                    if let Ok(mut sent_ms) = state.realtime_sent_ms.lock() {
                        *sent_ms = end_ms;
                    }
                }
            },
        )
        .await;

        if let Some(state) = app.try_state::<AppState>() {
            if let Ok(mut running) = state.realtime_running.lock() {
                *running = false;
            }
            if let Ok(mut sender) = state.realtime_sender.lock() {
                *sender = None;
            }
        }

        if let Err(error) = &result {
            storage::append_log(&app, &format!("realtime asr disconnected: {error}"));
            mark_realtime_asr_failed(&app, error.as_str());
            if is_recording_active(&app) {
                record_realtime_failure(&app, error.as_str());
                let show_realtime_transcript = storage::get_settings(&app)
                    .map(|settings| settings.show_realtime_transcript)
                    .unwrap_or(false);
                if show_realtime_transcript {
                    show_realtime_error_overlay(&app);
                }
            }
        }
        let _ = completion_sender.send(result);
    });

    Ok(())
}

fn start_realtime_audio_producer(
    app: AppHandle,
    sender: tokio::sync::mpsc::Sender<services::RealtimeAudioChunk>,
    start_sample_index: usize,
    catchup: bool,
) {
    tauri::async_runtime::spawn(async move {
        let mut next_sample_index = start_sample_index;
        let poll_ms = if catchup { 40 } else { 200 };

        loop {
            tokio::time::sleep(std::time::Duration::from_millis(poll_ms)).await;
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

            let segment = app_state
                .recorder
                .lock()
                .ok()
                .and_then(|recorder| {
                    recorder
                        .as_ref()
                        .and_then(|recorder| recorder.segment_since(next_sample_index).ok())
                })
                .flatten();
            let Some(segment) = segment else {
                continue;
            };
            if segment.audio.duration_ms() < 80 {
                continue;
            }

            let send_delay_ms = if catchup && segment.audio.duration_ms() > 600 {
                10
            } else {
                200
            };
            let chunk_len = 16_000 * 200 / 1000;
            let total_samples = segment.audio.pcm_16k.len();
            let mut sent_samples = 0_usize;
            for chunk in segment.audio.pcm_16k.chunks(chunk_len) {
                sent_samples += chunk.len();
                let is_last_chunk = sent_samples >= total_samples;
                let end_ms = if is_last_chunk {
                    segment.end_ms
                } else {
                    segment.start_ms + (sent_samples as u64 * 1000) / 16_000
                };
                let next_index = if is_last_chunk {
                    segment.next_sample_index
                } else {
                    next_sample_index
                };
                if sender
                    .send(services::RealtimeAudioChunk {
                        pcm_16k: chunk.to_vec(),
                        next_sample_index: next_index,
                        end_ms,
                        send_delay_ms,
                    })
                    .await
                    .is_err()
                {
                    return;
                }
            }
            next_sample_index = segment.next_sample_index;
            if let Ok(mut enqueued_checkpoint) = app_state.realtime_enqueued_checkpoint.lock() {
                *enqueued_checkpoint = next_sample_index;
            };
        }
    });
}

fn update_realtime_transcript(
    app: &AppHandle,
    text: &str,
    utterances: Vec<services::DoubaoUtterance>,
) {
    if !is_recording_active(app) {
        return;
    }
    let settings = storage::get_settings(app).unwrap_or_default();
    if let Some(state) = app.try_state::<AppState>() {
        let transcript_segments = utterances
            .into_iter()
            .map(|utterance| RealtimeTranscriptSegment {
                start_ms: utterance.start_ms,
                end_ms: utterance.end_ms,
                text: utterance.text,
                definite: utterance.definite,
            })
            .collect::<Vec<_>>();
        let incoming_text = realtime_preview_text(text, &transcript_segments);
        let preview_text = if let Ok(mut realtime_text) = state.realtime_text.lock() {
            let merged = merge_realtime_text(&realtime_text, &incoming_text);
            *realtime_text = merged.clone();
            merged
        } else {
            incoming_text
        };
        let lines = if settings.show_realtime_transcript {
            transcript_preview_lines(&preview_text)
        } else {
            Vec::new()
        };
        if let Ok(session_id) = state.active_session_id.lock().map(|value| value.clone()) {
            if let Some(session_id) = session_id {
                let _ = storage::append_realtime_transcript_segments(
                    app,
                    &session_id,
                    &transcript_segments,
                );
            }
        }
        if let Ok(mut transcript) = state.realtime_transcript.lock() {
            *transcript = lines.clone();
        }
        if let Ok(mut overlay) = state.overlay.lock() {
            overlay.transcript_lines = lines;
            overlay.status_kind = Some("recording".into());
            overlay.reconnect_available = false;
            if let Some(window) = app.get_webview_window("overlay") {
                let _ = window.emit("overlay-state", overlay.clone());
            }
        }
    }
}

fn show_saved_tick(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut overlay) = state.overlay.lock() {
            overlay.status_kind = Some("saved".into());
            if let Some(window) = app.get_webview_window("overlay") {
                let _ = window.emit("overlay-state", overlay.clone());
            }
        }
    }

    let app_for_reset = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(900)).await;
        if let Some(state) = app_for_reset.try_state::<AppState>() {
            if let Ok(mut overlay) = state.overlay.lock() {
                if overlay.status_kind.as_deref() == Some("saved") {
                    overlay.status_kind = Some("recording".into());
                    if let Some(window) = app_for_reset.get_webview_window("overlay") {
                        let _ = window.emit("overlay-state", overlay.clone());
                    }
                }
            }
        }
    });
}

fn is_recording_active(app: &AppHandle) -> bool {
    app.try_state::<AppState>()
        .and_then(|state| {
            state
                .recording
                .lock()
                .ok()
                .map(|session| session.status == "recording")
        })
        .unwrap_or(false)
}

fn should_defer_overlay_to_active_recording(app: &AppHandle, phase: &str) -> bool {
    phase != "recording" && is_recording_active(app)
}

fn update_overlay_progress(app: &AppHandle, phase: &str, current: u64, total: u64) {
    if should_defer_overlay_to_active_recording(app, phase) {
        return;
    }

    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut overlay) = state.overlay.lock() {
            overlay.visible = true;
            overlay.phase = phase.into();
            overlay.status_kind = Some("processing".into());
            overlay.progress_current = Some(current);
            overlay.progress_total = Some(total);
            if let Some(window) = app.get_webview_window("overlay") {
                let _ = window.emit("overlay-state", overlay.clone());
            }
        }
    }
}

fn record_realtime_failure(app: &AppHandle, reason: &str) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let Ok(session_id) = state.active_session_id.lock().map(|value| value.clone()) else {
        return;
    };
    let Some(session_id) = session_id else {
        return;
    };
    let checkpoint = state
        .realtime_checkpoint
        .lock()
        .map(|value| *value)
        .unwrap_or(0);
    let range = state
        .recorder
        .lock()
        .ok()
        .and_then(|recorder| {
            recorder
                .as_ref()
                .and_then(|recorder| recorder.segment_since(checkpoint).ok())
        })
        .flatten();
    let (start_ms, end_ms) = range
        .map(|segment| (segment.start_ms, segment.end_ms))
        .unwrap_or((0, 0));
    let _ = storage::append_realtime_failed_range(app, &session_id, start_ms, end_ms, reason);
}

fn mark_realtime_asr_failed(app: &AppHandle, reason: &str) {
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut failed) = state.realtime_failed.lock() {
            *failed = true;
        }
        if let Ok(mut sender) = state.realtime_sender.lock() {
            *sender = None;
        }
    }
    storage::append_log(app, &format!("快速转写已停用本次录音：{reason}"));
}

fn reset_realtime_overlay_status(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(mut overlay) = state.overlay.lock() {
            overlay.status_kind = Some("recording".into());
            overlay.reconnect_available = false;
            if let Some(window) = app.get_webview_window("overlay") {
                let _ = window.emit("overlay-state", overlay.clone());
            }
        }
    }
}

fn show_realtime_error_overlay(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        let show_realtime_transcript = storage::get_settings(app)
            .map(|settings| settings.show_realtime_transcript)
            .unwrap_or(true);
        let transcript_lines = if show_realtime_transcript {
            overlay_state_transcript_lines(&state)
        } else {
            Vec::new()
        };
        let overlay_state = OverlayState {
            visible: true,
            phase: "recording".into(),
            label: "直接说".into(),
            elapsed_ms: 0,
            input_level: 0.0,
            action_label: None,
            status_kind: Some("network_error".into()),
            transcript_lines,
            progress_current: None,
            progress_total: None,
            reconnect_available: false,
        };
        if let Ok(mut overlay) = state.overlay.lock() {
            *overlay = overlay_state.clone();
        }
        if let Some(window) = app.get_webview_window("overlay") {
            let _ = window.emit("overlay-state", overlay_state);
        }
    }
}

fn count_text_chars(text: &str) -> usize {
    text.chars().filter(|ch| !ch.is_whitespace()).count()
}

fn format_duration_for_log(ms: u64) -> String {
    let total_seconds = ms / 1000;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes:02}:{seconds:02}")
}

fn active_text_provider_label(settings: &AppSettings) -> String {
    match settings.optimize_provider.as_str() {
        "deepseek" => "DeepSeek".into(),
        "custom_openai" => settings.custom_openai_provider_name.clone(),
        _ => "OpenRouter".into(),
    }
}

fn active_text_model_name(settings: &AppSettings) -> String {
    match settings.optimize_provider.as_str() {
        "deepseek" => settings.deepseek_model.clone(),
        "custom_openai" => settings.custom_openai_model.clone(),
        _ => settings.openrouter_model.clone(),
    }
}

fn transcript_preview_lines(text: &str) -> Vec<String> {
    let chars = text.trim().chars().collect::<Vec<_>>();
    if chars.is_empty() {
        return Vec::new();
    }
    let start = chars.len().saturating_sub(140);
    vec![chars[start..].iter().collect::<String>()]
}

fn realtime_preview_text(text: &str, segments: &[RealtimeTranscriptSegment]) -> String {
    let mut ordered_segments = segments.iter().collect::<Vec<_>>();
    ordered_segments.sort_by_key(|segment| (segment.start_ms, segment.end_ms));
    let mut preview = String::new();
    for segment in ordered_segments {
        append_text_fragment(&mut preview, &segment.text);
    }
    if preview.trim().is_empty() {
        text.trim().to_string()
    } else {
        preview
    }
}

fn merge_realtime_text(previous: &str, incoming: &str) -> String {
    let previous = previous.trim();
    let incoming = incoming.trim();
    if incoming.is_empty() {
        return previous.to_string();
    }
    if previous.is_empty() || incoming.starts_with(previous) {
        return incoming.to_string();
    }
    if previous.ends_with(incoming) {
        return previous.to_string();
    }
    let mut merged = previous.to_string();
    append_text_fragment(&mut merged, incoming);
    merged
}

fn append_text_fragment(target: &mut String, fragment: &str) {
    let fragment = fragment.trim();
    if fragment.is_empty() {
        return;
    }
    if target.ends_with(fragment) {
        return;
    }
    if let (Some(left), Some(right)) = (target.chars().last(), fragment.chars().next()) {
        if left.is_ascii_alphanumeric() && right.is_ascii_alphanumeric() {
            target.push(' ');
        }
    }
    target.push_str(fragment);
}

fn overlay_state_transcript_lines(app_state: &State<'_, AppState>) -> Vec<String> {
    app_state
        .realtime_transcript
        .lock()
        .map(|lines| {
            lines
                .iter()
                .rev()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect()
        })
        .unwrap_or_default()
}

fn finish_overlay_after_record(app: AppHandle, record: &SpeechRecord) {
    if is_recording_active(&app) {
        return;
    }

    if record.error_message.is_some() {
        let _ = show_action_overlay(
            &app,
            "录音已保存",
            "网络恢复后可在主界面重新转写或重新优化",
            "打开主界面",
        );
    } else {
        hide_overlay_later(app);
    }
}

fn show_action_overlay(
    app: &AppHandle,
    title: &str,
    detail: &str,
    action_label: &str,
) -> Result<(), String> {
    if is_recording_active(app) {
        return Ok(());
    }

    let state = OverlayState {
        visible: true,
        phase: "attention".into(),
        label: format!("{title}：{detail}"),
        elapsed_ms: 0,
        input_level: 0.0,
        action_label: Some(action_label.into()),
        status_kind: Some("attention".into()),
        transcript_lines: Vec::new(),
        progress_current: None,
        progress_total: None,
        reconnect_available: false,
    };
    if let Some(app_state) = app.try_state::<AppState>() {
        if let Ok(mut overlay_state) = app_state.overlay.lock() {
            *overlay_state = state.clone();
        }
    }

    if let Some(overlay) = app.get_webview_window("overlay") {
        position_overlay(&overlay, true)?;
        overlay.show().map_err(|error| error.to_string())?;
        overlay
            .emit("overlay-state", state)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
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

fn position_overlay(window: &WebviewWindow, _expanded: bool) -> Result<(), String> {
    if let Some(monitor) = window
        .current_monitor()
        .map_err(|error| error.to_string())?
    {
        let size = monitor.size();
        let scale = monitor.scale_factor();
        let width = OVERLAY_WINDOW_WIDTH;
        let height = OVERLAY_WINDOW_HEIGHT;
        let x = (size.width as f64 / scale - width) / 2.0;
        let y = size.height as f64 / scale - height - OVERLAY_BOTTOM_MARGIN;
        window
            .set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }))
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn should_expand_overlay(app: &AppHandle, phase: &str) -> bool {
    if matches!(phase, "transcribing" | "optimizing" | "attention") {
        return true;
    }
    if phase != "recording" {
        return false;
    }
    storage::get_settings(app)
        .map(|settings| settings.show_realtime_transcript)
        .unwrap_or(true)
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
