use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use serde::{de::DeserializeOwned, Serialize};
use tauri::{AppHandle, Manager};

use crate::models::{AppSettings, PromptSettings, RecordPage, SpeechRecord};

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
    records.retain(|record| record.id != id);
    save_records(app, &records)?;
    Ok(records)
}

pub fn recording_path(app: &AppHandle, record_id: &str) -> Result<PathBuf, String> {
    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let dir = data_dir(app)?.join("recordings").join(date);
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir.join(format!("{record_id}.wav")))
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
