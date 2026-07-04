use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use chrono::{Duration, Utc};
use flate2::{write::GzEncoder, Compression};
use serde::{de::DeserializeOwned, Serialize};
use tauri::{AppHandle, Manager};

use crate::{
    models::{
        AppSettings, PromptSettings, RealtimeFailedRange, RealtimeTranscriptSegment, RecordPage,
        RecordingFileSegment, RecordingFileSession, SpeechRecord,
    },
    recorder::{read_wav_pcm_16k, RecordedAudio},
};

pub fn get_settings(app: &AppHandle) -> Result<AppSettings, String> {
    read_json(&settings_path(app)?)
}

pub fn save_settings(app: &AppHandle, settings: &AppSettings) -> Result<(), String> {
    write_json(&settings_path(app)?, settings)
}

pub fn get_prompts(app: &AppHandle) -> Result<PromptSettings, String> {
    read_json(&prompts_path(app)?)
}

pub fn save_prompts(app: &AppHandle, prompts: &PromptSettings) -> Result<(), String> {
    write_json(&prompts_path(app)?, prompts)
}

pub fn read_records(app: &AppHandle) -> Result<Vec<SpeechRecord>, String> {
    let mut records = read_json::<Vec<SpeechRecord>>(&records_path(app)?)?;
    records.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(records)
}

pub fn read_record_page(
    app: &AppHandle,
    offset: usize,
    limit: usize,
) -> Result<RecordPage, String> {
    let records = read_records(app)?;
    let total = records.len();
    let page = records
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();

    Ok(RecordPage {
        has_more: offset + page.len() < total,
        records: page,
        total,
        offset,
        limit,
    })
}

pub fn save_records(app: &AppHandle, records: &[SpeechRecord]) -> Result<(), String> {
    write_json(&records_path(app)?, records)
}

pub fn upsert_record(app: &AppHandle, record: SpeechRecord) -> Result<SpeechRecord, String> {
    let mut records = read_records(app)?;
    if let Some(existing) = records.iter_mut().find(|item| item.id == record.id) {
        *existing = record.clone();
    } else {
        records.insert(0, record.clone());
    }
    save_records(app, &records)?;
    Ok(record)
}

pub fn find_record(app: &AppHandle, id: &str) -> Result<SpeechRecord, String> {
    read_records(app)?
        .into_iter()
        .find(|record| record.id == id)
        .ok_or_else(|| "找不到这条记录".to_string())
}

pub fn delete_record(app: &AppHandle, id: &str) -> Result<Vec<SpeechRecord>, String> {
    let mut records = read_records(app)?;
    if let Some(record) = records.iter().find(|record| record.id == id) {
        if let Some(audio_path) = record.audio_path.as_deref() {
            remove_file_if_exists(Path::new(audio_path))?;
            append_log(
                app,
                &format!(
                    "删除历史记录时已删除录音：record_id={}，文件={}。",
                    id, audio_path
                ),
            );
        }
    }
    records.retain(|record| record.id != id);
    save_records(app, &records)?;
    append_log(app, &format!("历史记录已删除：record_id={}。", id));
    Ok(records)
}

pub fn recording_path(app: &AppHandle, record_id: &str) -> Result<PathBuf, String> {
    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let dir = data_dir(app)?.join("recordings").join(date);
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir.join(format!("{record_id}.wav")))
}

pub fn create_recording_file_session(
    app: &AppHandle,
    id: &str,
    started_at: &str,
) -> Result<RecordingFileSession, String> {
    let session = RecordingFileSession {
        id: id.to_string(),
        started_at: started_at.to_string(),
        updated_at: started_at.to_string(),
        status: "active".into(),
        final_audio_path: None,
        segments: Vec::new(),
        realtime_segments: Vec::new(),
        failed_ranges: Vec::new(),
    };
    write_recording_file_session(app, &session)?;
    Ok(session)
}

pub fn append_realtime_transcript_segments(
    app: &AppHandle,
    session_id: &str,
    segments: &[RealtimeTranscriptSegment],
) -> Result<(), String> {
    if segments.is_empty() {
        return Ok(());
    }

    let mut session = read_recording_file_session(app, session_id)?;
    session.updated_at = Utc::now().to_rfc3339();
    for segment in segments {
        session
            .realtime_segments
            .retain(|item| item.start_ms != segment.start_ms || item.end_ms != segment.end_ms);
        session.realtime_segments.push(segment.clone());
    }
    session
        .realtime_segments
        .sort_by_key(|segment| (segment.start_ms, segment.end_ms));
    write_recording_file_session(app, &session)
}

pub fn append_realtime_failed_range(
    app: &AppHandle,
    session_id: &str,
    start_ms: u64,
    end_ms: u64,
    reason: &str,
) -> Result<(), String> {
    let mut session = read_recording_file_session(app, session_id)?;
    session.updated_at = Utc::now().to_rfc3339();
    session.failed_ranges.push(RealtimeFailedRange {
        start_ms,
        end_ms,
        reason: reason.to_string(),
    });
    write_recording_file_session(app, &session)
}

pub fn append_recording_file_segment(
    app: &AppHandle,
    session_id: &str,
    index: u32,
    start_ms: u64,
    end_ms: u64,
    audio: &RecordedAudio,
) -> Result<RecordingFileSession, String> {
    let path = recording_segment_path(app, session_id, index)?;
    audio.save_wav(&path)?;

    let mut session = read_recording_file_session(app, session_id)?;
    session.updated_at = Utc::now().to_rfc3339();
    session.segments.retain(|segment| segment.index != index);
    session.segments.push(RecordingFileSegment {
        index,
        start_ms,
        end_ms,
        path: path_to_string(&path),
        status: "saved".into(),
    });
    session.segments.sort_by_key(|segment| segment.index);
    write_recording_file_session(app, &session)?;
    Ok(session)
}

pub fn finish_recording_file_session(
    app: &AppHandle,
    session_id: &str,
    final_audio_path: &Path,
) -> Result<(), String> {
    let mut session = read_recording_file_session(app, session_id)?;
    session.updated_at = Utc::now().to_rfc3339();
    session.status = "completed".into();
    session.final_audio_path = Some(path_to_string(final_audio_path));
    write_recording_file_session(app, &session)
}

pub fn remove_recording_file_session(app: &AppHandle, session_id: &str) -> Result<(), String> {
    let dir = recording_sessions_dir(app)?.join(session_id);
    remove_dir_if_exists(&dir)
}

pub fn recover_interrupted_recording_sessions(
    app: &AppHandle,
) -> Result<Vec<SpeechRecord>, String> {
    let sessions_dir = recording_sessions_dir(app)?;
    if !sessions_dir.exists() {
        return Ok(Vec::new());
    }

    let settings = get_settings(app).unwrap_or_default();
    let mut recovered = Vec::new();
    let entries = fs::read_dir(&sessions_dir).map_err(|error| error.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        if !entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_dir()
        {
            continue;
        }
        let session_id = entry.file_name().to_string_lossy().to_string();
        let mut session = match read_recording_file_session(app, &session_id) {
            Ok(session) => session,
            Err(error) => {
                append_log(
                    app,
                    &format!("跳过一个录音恢复任务：读取 session 失败，错误={error}。"),
                );
                continue;
            }
        };
        if session.status != "active" {
            continue;
        }

        let mut pcm = Vec::new();
        session.segments.sort_by_key(|segment| segment.index);
        for segment in &session.segments {
            let path = PathBuf::from(&segment.path);
            if path.exists() {
                match read_wav_pcm_16k(&path) {
                    Ok(mut segment_pcm) => pcm.append(&mut segment_pcm),
                    Err(error) => {
                        append_log(
                            app,
                            &format!(
                                "恢复录音时跳过一个分段：文件={}，错误={}。",
                                path.display(),
                                error
                            ),
                        );
                    }
                }
            }
        }

        if pcm.is_empty() {
            session.status = "empty".into();
            session.updated_at = Utc::now().to_rfc3339();
            write_recording_file_session(app, &session)?;
            continue;
        }

        let audio = RecordedAudio::from_pcm_16k(pcm);
        let audio_path = recording_path(app, &session.id)?;
        audio.save_wav(&audio_path)?;
        let now = Utc::now();
        let record = SpeechRecord {
            id: session.id.clone(),
            created_at: if session.started_at.trim().is_empty() {
                now.to_rfc3339()
            } else {
                session.started_at.clone()
            },
            updated_at: now.to_rfc3339(),
            raw_asr_text: String::new(),
            final_text: String::new(),
            audio_path: Some(path_to_string(&audio_path)),
            duration_ms: Some(audio.duration_ms()),
            audio_expires_at: Some(
                (now + Duration::days(settings.recording_retention_days)).to_rfc3339(),
            ),
            asr_status: "recovered".into(),
            optimize_status: "blocked".into(),
            copied_at: None,
            pasted_at: None,
            error_message: Some("未完成录音已恢复，可在详情中试听或重新转写。".into()),
            doubao_request_id: None,
            doubao_log_id: None,
            openrouter_model: Some(active_text_model_name(&settings)),
        };
        let record = upsert_record(app, record)?;

        session.status = "recovered".into();
        session.updated_at = now.to_rfc3339();
        session.final_audio_path = Some(path_to_string(&audio_path));
        write_recording_file_session(app, &session)?;
        remove_recording_file_session(app, &session.id)?;
        append_log(
            app,
            &format!(
                "未完成录音已恢复：session={}，时长={}，文件={}。",
                session.id,
                format_duration_for_log(audio.duration_ms()),
                audio_path.display()
            ),
        );
        recovered.push(record);
    }

    Ok(recovered)
}

pub fn cleanup_expired_recording_files(app: &AppHandle) -> Result<usize, String> {
    let now = Utc::now();
    let mut records = read_records(app)?;
    let mut removed = 0_usize;
    for record in &mut records {
        let Some(expires_at) = record.audio_expires_at.clone() else {
            continue;
        };
        let Ok(expires_at) = chrono::DateTime::parse_from_rfc3339(&expires_at) else {
            continue;
        };
        if expires_at.with_timezone(&Utc) > now {
            continue;
        }
        let Some(audio_path) = record.audio_path.clone() else {
            continue;
        };
        remove_file_if_exists(Path::new(&audio_path))?;
        append_log(
            app,
            &format!(
                "过期录音已删除：record_id={}，文件={}。",
                record.id, audio_path
            ),
        );
        record.audio_path = None;
        record.audio_expires_at = None;
        record.updated_at = now.to_rfc3339();
        removed += 1;
    }
    if removed > 0 {
        save_records(app, &records)?;
    }
    Ok(removed)
}

pub fn ensure_daily_backup(app: &AppHandle) -> Result<(), String> {
    let date = Utc::now().format("%Y-%m-%d").to_string();
    let dir = data_dir(app)?.join("backups");
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let backup_path = dir.join(format!("{date}.tar.gz"));
    if backup_path.exists() {
        cleanup_old_backups(&dir, 7)?;
        return Ok(());
    }

    let file = File::create(&backup_path).map_err(|error| error.to_string())?;
    let encoder = GzEncoder::new(file, Compression::default());
    let mut archive = tar::Builder::new(encoder);

    append_backup_file(&mut archive, settings_path(app)?, "settings.json")?;
    append_backup_file(&mut archive, prompts_path(app)?, "prompts.json")?;
    append_backup_file(&mut archive, records_path(app)?, "records.json")?;
    append_backup_file(&mut archive, log_path(app)?, "app.log")?;
    append_recording_session_manifests(app, &mut archive)?;

    archive.finish().map_err(|error| error.to_string())?;
    cleanup_old_backups(&dir, 7)?;
    append_log(
        app,
        &format!("每日备份已创建：文件={}。", backup_path.display()),
    );
    Ok(())
}

pub fn append_log(app: &AppHandle, message: &str) {
    let Ok(settings) = get_settings(app) else {
        return;
    };
    if !settings.save_logs {
        return;
    }
    let Ok(path) = log_path(app) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let timestamp = chrono::Utc::now().to_rfc3339();
        let _ = writeln!(file, "[{timestamp}] {message}");
    }
}

pub fn read_log(app: &AppHandle) -> Result<String, String> {
    let path = log_path(app)?;
    if !path.exists() {
        return Ok(String::new());
    }
    fs::read_to_string(path).map_err(|error| error.to_string())
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("settings.json"))
}

fn prompts_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("prompts.json"))
}

fn records_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("records.json"))
}

fn log_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("app.log"))
}

fn recording_sessions_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = data_dir(app)?.join("recording-sessions");
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

fn recording_session_dir(app: &AppHandle, session_id: &str) -> Result<PathBuf, String> {
    let dir = recording_sessions_dir(app)?.join(session_id);
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

fn recording_session_manifest_path(app: &AppHandle, session_id: &str) -> Result<PathBuf, String> {
    Ok(recording_session_dir(app, session_id)?.join("manifest.json"))
}

fn recording_segment_path(
    app: &AppHandle,
    session_id: &str,
    index: u32,
) -> Result<PathBuf, String> {
    let dir = recording_session_dir(app, session_id)?.join("segments");
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir.join(format!("{index:04}.wav")))
}

fn read_recording_file_session(
    app: &AppHandle,
    session_id: &str,
) -> Result<RecordingFileSession, String> {
    read_json(&recording_session_manifest_path(app, session_id)?)
}

fn write_recording_file_session(
    app: &AppHandle,
    session: &RecordingFileSession,
) -> Result<(), String> {
    write_json(&recording_session_manifest_path(app, &session.id)?, session)
}

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

fn read_json<T>(path: &Path) -> Result<T, String>
where
    T: DeserializeOwned + Default,
{
    if !path.exists() {
        return Ok(T::default());
    }

    let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&content).map_err(|error| error.to_string())
}

fn write_json<T>(path: &Path, value: &T) -> Result<(), String>
where
    T: Serialize + ?Sized,
{
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let content = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    fs::write(path, content).map_err(|error| error.to_string())
}

fn append_backup_file<W: Write>(
    archive: &mut tar::Builder<W>,
    path: PathBuf,
    name: &str,
) -> Result<(), String> {
    if path.exists() {
        archive
            .append_path_with_name(&path, name)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn append_recording_session_manifests<W: Write>(
    app: &AppHandle,
    archive: &mut tar::Builder<W>,
) -> Result<(), String> {
    let sessions_dir = recording_sessions_dir(app)?;
    if !sessions_dir.exists() {
        return Ok(());
    }

    let entries = fs::read_dir(sessions_dir).map_err(|error| error.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        if !entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_dir()
        {
            continue;
        }
        let session_id = entry.file_name().to_string_lossy().to_string();
        let manifest = entry.path().join("manifest.json");
        if manifest.exists() {
            let name = format!("recording-sessions/{session_id}/manifest.json");
            archive
                .append_path_with_name(&manifest, name)
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn cleanup_old_backups(dir: &Path, keep: usize) -> Result<(), String> {
    let mut backups = fs::read_dir(dir)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".tar.gz"))
        })
        .collect::<Vec<_>>();
    backups.sort_by_key(|entry| entry.file_name());
    while backups.len() > keep {
        let entry = backups.remove(0);
        fs::remove_file(entry.path()).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn remove_file_if_exists(path: &Path) -> Result<(), String> {
    if path.exists() {
        fs::remove_file(path).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn remove_dir_if_exists(path: &Path) -> Result<(), String> {
    if path.exists() {
        fs::remove_dir_all(path).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn format_duration_for_log(ms: u64) -> String {
    let total_seconds = ms / 1000;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes:02}:{seconds:02}")
}

fn active_text_model_name(settings: &AppSettings) -> String {
    match settings.optimize_provider.as_str() {
        "deepseek" => settings.deepseek_model.clone(),
        "custom_openai" => settings.custom_openai_model.clone(),
        _ => settings.openrouter_model.clone(),
    }
}
