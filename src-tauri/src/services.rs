use std::{io::Write, path::Path};

use flate2::{write::GzEncoder, Compression};
use futures_util::{future::try_join, SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
};
use uuid::Uuid;

use crate::{
    models::{AppSettings, PromptSettings},
    recorder::read_wav_pcm_16k,
};

const DOUBAO_AUDIO_CHUNK_MS: usize = 200;
const DOUBAO_FAST_SEND_DELAY_MS: u64 = 10;

pub struct RealtimeAudioChunk {
    pub pcm_16k: Vec<i16>,
    pub next_sample_index: usize,
    pub end_ms: u64,
    pub send_delay_ms: u64,
}

#[derive(Debug, Clone)]
pub struct DoubaoUtterance {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub definite: bool,
}

pub async fn transcribe_audio(
    settings: &AppSettings,
    audio_path: &Path,
) -> Result<(String, Option<String>, String), String> {
    transcribe_audio_with_progress(settings, audio_path, |_current_ms, _total_ms| {}).await
}

pub async fn transcribe_audio_with_progress<F>(
    settings: &AppSettings,
    audio_path: &Path,
    mut on_progress: F,
) -> Result<(String, Option<String>, String), String>
where
    F: FnMut(u64, u64) + Send,
{
    let pcm = read_wav_pcm_16k(audio_path)?;
    if pcm.is_empty() {
        return Err("录音文件为空".to_string());
    }
    let total_ms = (pcm.len() as u64 * 1000) / 16_000;

    let request_id = Uuid::new_v4().to_string();
    let request = build_doubao_request(settings, &request_id)?;

    let (ws, response) = connect_async(request).await.map_err(|error| {
        let message = error.to_string();
        if message.contains("401") || message.contains("Unauthorized") {
            format!(
                "豆包鉴权失败：HTTP 401 Unauthorized。请检查鉴权方式、API Key/App Key/Access Key 和 Resource ID 是否来自同一个控制台项目。原始错误：{message}"
            )
        } else {
            message
        }
    })?;
    let log_id = response
        .headers()
        .get("X-Tt-Logid")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string());

    let init_payload = doubao_init_payload(settings);

    let (mut write, mut read) = ws.split();
    let reader = async move {
        let mut latest_text = String::new();
        while let Some(message) = read.next().await {
            let message = message.map_err(|error| error.to_string())?;
            let Message::Binary(data) = message else {
                continue;
            };
            let response = parse_server_frame(data.as_ref())?;
            if let Some(text) = response.text {
                if !text.trim().is_empty() {
                    latest_text = text;
                }
            }
            if response.is_last {
                break;
            }
        }
        Ok::<String, String>(latest_text)
    };

    let writer = async move {
        write
            .send(Message::Binary(
                build_frame(
                    MessageKind::FullClientRequest,
                    false,
                    Serialization::Json,
                    CompressionFlag::Gzip,
                    &gzip(serde_json::to_vec(&init_payload).map_err(|error| error.to_string())?)?,
                )
                .into(),
            ))
            .await
            .map_err(|error| error.to_string())?;

        let bytes = pcm
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect::<Vec<_>>();
        let chunk_size = 16_000 * 2 * DOUBAO_AUDIO_CHUNK_MS / 1000;
        let mut chunks = bytes.chunks(chunk_size).peekable();
        let mut sent_bytes = 0_usize;
        while let Some(chunk) = chunks.next() {
            let is_last = chunks.peek().is_none();
            write
                .send(Message::Binary(
                    build_frame(
                        MessageKind::AudioOnlyRequest,
                        is_last,
                        Serialization::None,
                        CompressionFlag::Gzip,
                        &gzip(chunk.to_vec())?,
                    )
                    .into(),
                ))
                .await
                .map_err(|error| error.to_string())?;
            sent_bytes += chunk.len();
            let current_ms = ((sent_bytes / 2) as u64 * 1000 / 16_000).min(total_ms);
            on_progress(current_ms, total_ms);
            if !is_last {
                sleep(Duration::from_millis(DOUBAO_FAST_SEND_DELAY_MS)).await;
            }
        }
        Ok::<(), String>(())
    };

    let (_, latest_text) = try_join(writer, reader).await?;

    Ok((latest_text, log_id, request_id))
}

pub async fn transcribe_audio_stream<F, G>(
    settings: AppSettings,
    mut receiver: mpsc::Receiver<RealtimeAudioChunk>,
    send_delay_ms: u64,
    mut on_text: F,
    mut on_sent: G,
) -> Result<(), String>
where
    F: FnMut(String, Vec<DoubaoUtterance>) + Send + 'static,
    G: FnMut(usize, u64) + Send + 'static,
{
    let request_id = Uuid::new_v4().to_string();
    let request = build_doubao_request(&settings, &request_id)?;
    let (ws, _) = connect_async(request).await.map_err(|error| {
        let message = error.to_string();
        if message.contains("401") || message.contains("Unauthorized") {
            format!(
                "豆包鉴权失败：HTTP 401 Unauthorized。请检查鉴权方式、API Key/App Key/Access Key 和 Resource ID 是否来自同一个控制台项目。原始错误：{message}"
            )
        } else {
            message
        }
    })?;
    let init_payload = doubao_init_payload(&settings);
    let (mut write, mut read) = ws.split();

    let reader = async move {
        while let Some(message) = read.next().await {
            let message = message.map_err(|error| error.to_string())?;
            let Message::Binary(data) = message else {
                continue;
            };
            let response = parse_server_frame(data.as_ref())?;
            if let Some(text) = response.text {
                if !text.trim().is_empty() {
                    on_text(text, response.utterances);
                }
            }
            if response.is_last {
                break;
            }
        }
        Ok::<(), String>(())
    };

    let writer = async move {
        write
            .send(Message::Binary(
                build_frame(
                    MessageKind::FullClientRequest,
                    false,
                    Serialization::Json,
                    CompressionFlag::Gzip,
                    &gzip(serde_json::to_vec(&init_payload).map_err(|error| error.to_string())?)?,
                )
                .into(),
            ))
            .await
            .map_err(|error| error.to_string())?;

        let Some(mut current) = receiver.recv().await else {
            return Ok::<(), String>(());
        };
        while let Some(next) = receiver.recv().await {
            write
                .send(build_audio_message_from_pcm(&current.pcm_16k, false)?)
                .await
                .map_err(|error| error.to_string())?;
            on_sent(current.next_sample_index, current.end_ms);
            let delay_ms = if current.send_delay_ms == 0 {
                send_delay_ms
            } else {
                current.send_delay_ms
            };
            sleep(Duration::from_millis(delay_ms)).await;
            current = next;
        }
        write
            .send(build_audio_message_from_pcm(&current.pcm_16k, true)?)
            .await
            .map_err(|error| error.to_string())?;
        on_sent(current.next_sample_index, current.end_ms);
        Ok::<(), String>(())
    };

    try_join(writer, reader).await?;
    Ok(())
}

pub async fn optimize_text(
    settings: &AppSettings,
    prompts: &PromptSettings,
    raw_asr_text: &str,
) -> Result<String, String> {
    let provider = active_text_provider(settings)?;

    let mut client_builder = reqwest::Client::builder();
    if !provider.use_system_proxy {
        client_builder = client_builder.no_proxy();
    }
    let client = client_builder.build().map_err(|error| error.to_string())?;

    let url = format!(
        "{}/chat/completions",
        provider.base_url.trim_end_matches('/')
    );
    let system_prompt = build_system_prompt(prompts);

    let mut request = client
        .post(url)
        .bearer_auth(&provider.api_key)
        .json(&json!({
            "model": provider.model,
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": format!("ASR 原文：\n{raw_asr_text}") }
            ]
        }));

    if let Some(title) = provider.title.filter(|value| !value.trim().is_empty()) {
        request = request.header("X-OpenRouter-Title", title);
    }
    if let Some(referer) = provider.referer.filter(|value| !value.trim().is_empty()) {
        request = request.header("HTTP-Referer", referer);
    } else if !settings.openrouter_http_referer.trim().is_empty() {
        request = request.header("HTTP-Referer", &settings.openrouter_http_referer);
    }

    let response = request.send().await.map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("{} 请求失败：{status} {text}", provider.name));
    }

    let body = response
        .json::<OpenRouterResponse>()
        .await
        .map_err(|error| error.to_string())?;
    body.choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content.trim().to_string())
        .filter(|text| !text.is_empty())
        .ok_or_else(|| format!("{} 没有返回可用文本", provider.name))
}

pub async fn optimize_text_streaming<F>(
    settings: &AppSettings,
    prompts: &PromptSettings,
    raw_asr_text: &str,
    mut on_progress: F,
) -> Result<String, String>
where
    F: FnMut(String) + Send,
{
    let provider = active_text_provider(settings)?;

    let mut client_builder = reqwest::Client::builder();
    if !provider.use_system_proxy {
        client_builder = client_builder.no_proxy();
    }
    let client = client_builder.build().map_err(|error| error.to_string())?;

    let url = format!(
        "{}/chat/completions",
        provider.base_url.trim_end_matches('/')
    );
    let system_prompt = build_system_prompt(prompts);

    let mut request = client
        .post(url)
        .bearer_auth(&provider.api_key)
        .json(&json!({
            "model": provider.model,
            "stream": true,
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": format!("ASR 原文：\n{raw_asr_text}") }
            ]
        }));

    if let Some(title) = provider.title.filter(|value| !value.trim().is_empty()) {
        request = request.header("X-OpenRouter-Title", title);
    }
    if let Some(referer) = provider.referer.filter(|value| !value.trim().is_empty()) {
        request = request.header("HTTP-Referer", referer);
    }

    let response = request.send().await.map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("{} 请求失败：{status} {text}", provider.name));
    }

    let mut output = String::new();
    let mut pending = String::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| error.to_string())?;
        pending.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(index) = pending.find('\n') {
            let line = pending[..index].trim().to_string();
            pending = pending[index + 1..].to_string();
            if !line.starts_with("data:") {
                continue;
            }
            let data = line.trim_start_matches("data:").trim();
            if data == "[DONE]" {
                return Ok(output.trim().to_string());
            }
            let parsed: OpenRouterStreamResponse =
                serde_json::from_str(data).map_err(|error| error.to_string())?;
            for choice in parsed.choices {
                if let Some(content) = choice.delta.content {
                    output.push_str(&content);
                    on_progress(output.clone());
                }
            }
        }
    }

    let output = output.trim().to_string();
    if output.is_empty() {
        Err(format!("{} 没有返回可用文本", provider.name))
    } else {
        Ok(output)
    }
}

pub async fn test_openrouter(settings: &AppSettings) -> Result<String, String> {
    let provider = active_text_provider(settings)?;

    let mut client_builder = reqwest::Client::builder();
    if !provider.use_system_proxy {
        client_builder = client_builder.no_proxy();
    }
    let client = client_builder.build().map_err(|error| error.to_string())?;
    let url = format!(
        "{}/chat/completions",
        provider.base_url.trim_end_matches('/')
    );

    let mut request = client
        .post(url)
        .bearer_auth(&provider.api_key)
        .json(&json!({
            "model": provider.model,
            "messages": [
                { "role": "user", "content": "ping" }
            ],
            "max_tokens": 4
        }));

    if let Some(title) = provider.title.filter(|value| !value.trim().is_empty()) {
        request = request.header("X-OpenRouter-Title", title);
    }
    if let Some(referer) = provider.referer.filter(|value| !value.trim().is_empty()) {
        request = request.header("HTTP-Referer", referer);
    }

    let response = request.send().await.map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("{} 测试失败：{status} {text}", provider.name));
    }

    Ok(format!("{} 连接可用", provider.name))
}

struct TextProvider {
    name: String,
    api_key: String,
    base_url: String,
    model: String,
    title: Option<String>,
    referer: Option<String>,
    use_system_proxy: bool,
}

fn active_text_provider(settings: &AppSettings) -> Result<TextProvider, String> {
    let provider = match settings.optimize_provider.as_str() {
        "deepseek" => TextProvider {
            name: "DeepSeek".into(),
            api_key: settings.deepseek_api_key.clone(),
            base_url: settings.deepseek_base_url.clone(),
            model: settings.deepseek_model.clone(),
            title: None,
            referer: None,
            use_system_proxy: true,
        },
        "custom_openai" => TextProvider {
            name: if settings.custom_openai_provider_name.trim().is_empty() {
                "Custom OpenAI-compatible".into()
            } else {
                settings.custom_openai_provider_name.clone()
            },
            api_key: settings.custom_openai_api_key.clone(),
            base_url: settings.custom_openai_base_url.clone(),
            model: settings.custom_openai_model.clone(),
            title: None,
            referer: None,
            use_system_proxy: true,
        },
        _ => TextProvider {
            name: "OpenRouter".into(),
            api_key: settings.openrouter_api_key.clone(),
            base_url: settings.openrouter_base_url.clone(),
            model: settings.openrouter_model.clone(),
            title: Some(settings.openrouter_title.clone()),
            referer: Some(settings.openrouter_http_referer.clone()),
            use_system_proxy: settings.use_system_proxy_for_openrouter,
        },
    };

    if provider.api_key.trim().is_empty() {
        return Err(format!("{} API Key 为空", provider.name));
    }
    if provider.base_url.trim().is_empty() {
        return Err(format!("{} Base URL 为空", provider.name));
    }
    if provider.model.trim().is_empty() {
        return Err(format!("{} 模型为空", provider.name));
    }
    Ok(provider)
}

fn build_system_prompt(prompts: &PromptSettings) -> String {
    let cleanup_prompt = cleanup_mode_prompt(&prompts.cleanup_mode);
    let mut parts = vec![prompts.system_prompt.trim().to_string()];
    if !cleanup_prompt.is_empty() {
        parts.push(format!("# 整理强度\n{cleanup_prompt}"));
    }
    parts.push(format!("# 用户词典\n{}", prompts.replacements.trim()));
    parts.push(format!(
        "# 个性化偏好\n{}",
        prompts.writing_preferences.trim()
    ));
    parts.join("\n\n")
}

fn cleanup_mode_prompt(mode: &str) -> &'static str {
    match mode {
        "light" => {
            "【轻度整理】\n保留原表达逻辑，只清理明显口语噪音、紧邻重复、明确自我修正和明显 ASR 错误。允许按自然语义轻度换行，但不总结、不扩写、不重排结构。"
        }
        "deep" => {
            "【深度整理】\n提取中心意思，删除口癖和重复表达，重组语序和结构；遇到明确列举、步骤、条件或任务时优先使用编号列表。不得新增事实、回答问题或给建议。"
        }
        _ => "",
    }
}

fn build_doubao_request(
    settings: &AppSettings,
    request_id: &str,
) -> Result<http::Request<()>, String> {
    let mut request = settings
        .doubao_endpoint
        .as_str()
        .into_client_request()
        .map_err(|error| error.to_string())?;
    let headers = request.headers_mut();

    if settings.doubao_auth_mode == "app_access_key" {
        if settings.doubao_app_key.trim().is_empty() || settings.doubao_access_key.trim().is_empty()
        {
            return Err("豆包旧版鉴权需要填写 App Key 和 Access Key".into());
        }
        headers.insert(
            "X-Api-App-Key",
            settings
                .doubao_app_key
                .parse()
                .map_err(|error| format!("豆包 App Key 请求头无效：{error}"))?,
        );
        headers.insert(
            "X-Api-Access-Key",
            settings
                .doubao_access_key
                .parse()
                .map_err(|error| format!("豆包 Access Key 请求头无效：{error}"))?,
        );
    } else {
        if settings.doubao_api_key.trim().is_empty() {
            return Err("豆包新版鉴权需要填写 API Key".into());
        }
        headers.insert(
            "X-Api-Key",
            settings
                .doubao_api_key
                .parse()
                .map_err(|error| format!("豆包 API Key 请求头无效：{error}"))?,
        );
    }
    headers.insert(
        "X-Api-Resource-Id",
        settings
            .doubao_resource_id
            .parse()
            .map_err(|error| format!("豆包 Resource ID 请求头无效：{error}"))?,
    );
    headers.insert(
        "X-Api-Request-Id",
        request_id
            .parse()
            .map_err(|error| format!("豆包 Request ID 请求头无效：{error}"))?,
    );
    headers.insert(
        "X-Api-Connect-Id",
        request_id
            .parse()
            .map_err(|error| format!("豆包 Connect ID 请求头无效：{error}"))?,
    );
    headers.insert("X-Api-Sequence", "-1".parse().unwrap());
    Ok(request)
}

fn doubao_init_payload(settings: &AppSettings) -> Value {
    json!({
        "user": {
            "uid": "sparkspeech"
        },
        "audio": {
            "format": "pcm",
            "rate": 16000,
            "bits": 16,
            "channel": 1,
            "language": settings.doubao_language
        },
        "request": {
            "model_name": "bigmodel",
            "enable_itn": true,
            "enable_punc": true,
            "enable_ddc": false,
            "enable_nonstream": true,
            "show_utterances": true,
            "result_type": "full"
        }
    })
}

fn build_audio_message_from_pcm(pcm_16k: &[i16], is_last: bool) -> Result<Message, String> {
    let bytes = pcm_16k
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect::<Vec<_>>();
    Ok(Message::Binary(
        build_frame(
            MessageKind::AudioOnlyRequest,
            is_last,
            Serialization::None,
            CompressionFlag::Gzip,
            &gzip(bytes)?,
        )
        .into(),
    ))
}

enum MessageKind {
    FullClientRequest,
    AudioOnlyRequest,
}

enum Serialization {
    None,
    Json,
}

enum CompressionFlag {
    Gzip,
}

struct ParsedServerFrame {
    text: Option<String>,
    utterances: Vec<DoubaoUtterance>,
    is_last: bool,
}

fn build_frame(
    kind: MessageKind,
    is_last: bool,
    serialization: Serialization,
    compression: CompressionFlag,
    payload: &[u8],
) -> Vec<u8> {
    let message_type = match kind {
        MessageKind::FullClientRequest => 0b0001,
        MessageKind::AudioOnlyRequest => 0b0010,
    };
    let flags = if is_last { 0b0010 } else { 0b0000 };
    let serialization = match serialization {
        Serialization::None => 0b0000,
        Serialization::Json => 0b0001,
    };
    let compression = match compression {
        CompressionFlag::Gzip => 0b0001,
    };

    let mut frame = vec![
        0b0001_0001,
        (message_type << 4) | flags,
        (serialization << 4) | compression,
        0,
    ];
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(payload);
    frame
}

fn parse_server_frame(data: &[u8]) -> Result<ParsedServerFrame, String> {
    if data.len() < 8 {
        return Err("豆包返回帧过短".to_string());
    }

    let header_size = ((data[0] & 0x0f) * 4) as usize;
    let message_type = data[1] >> 4;
    let flags = data[1] & 0x0f;
    let compression = data[2] & 0x0f;
    let mut cursor = header_size;

    if message_type == 0b1111 {
        return Err(parse_error_frame(data, cursor));
    }

    let has_sequence = flags == 0b0001 || flags == 0b0011;
    let metadata_size = if has_sequence { 8 } else { 4 };
    if data.len() < cursor + metadata_size {
        return Err("豆包返回帧缺少 payload".to_string());
    }
    if has_sequence {
        cursor += 4;
    }
    let size = u32::from_be_bytes(
        data[cursor..cursor + 4]
            .try_into()
            .map_err(|_| "payload size 解析失败".to_string())?,
    ) as usize;
    cursor += 4;

    if data.len() < cursor + size {
        return Err(format!(
            "豆包返回帧 payload 不完整：frame_len={}, header_size={}, flags={}, has_sequence={}, payload_size={}, payload_start={}",
            data.len(),
            header_size,
            flags,
            has_sequence,
            size,
            cursor
        ));
    }
    let payload = &data[cursor..cursor + size];
    let payload = if compression == 0b0001 {
        let mut decoder = flate2::read::GzDecoder::new(payload);
        let mut output = Vec::new();
        std::io::Read::read_to_end(&mut decoder, &mut output).map_err(|error| error.to_string())?;
        output
    } else {
        payload.to_vec()
    };

    let parsed: Value = serde_json::from_slice(&payload).map_err(|error| error.to_string())?;
    let utterances = extract_doubao_utterances(&parsed);
    Ok(ParsedServerFrame {
        text: extract_doubao_text(&parsed),
        utterances,
        is_last: flags == 0b0010 || flags == 0b0011,
    })
}

fn extract_doubao_text(value: &Value) -> Option<String> {
    if let Some(text) = value.get("text").and_then(Value::as_str) {
        return non_empty_text(text);
    }

    if let Some(result) = value.get("result") {
        if let Some(text) = result.get("text").and_then(Value::as_str) {
            return non_empty_text(text);
        }

        let mut parts = Vec::new();
        collect_utterance_text(result.get("utterances"), &mut parts);
        if !parts.is_empty() {
            return joined_parts(parts);
        }

        if let Some(items) = result.as_array() {
            for item in items {
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    parts.push(text.to_string());
                } else {
                    collect_utterance_text(item.get("utterances"), &mut parts);
                }
            }
            return joined_parts(parts);
        }
    }

    None
}

fn joined_parts(parts: Vec<String>) -> Option<String> {
    let text = parts
        .into_iter()
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("");

    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn non_empty_text(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

fn collect_utterance_text(value: Option<&Value>, parts: &mut Vec<String>) {
    if let Some(items) = value.and_then(Value::as_array) {
        for item in items {
            if let Some(text) = item.get("text").and_then(Value::as_str) {
                parts.push(text.to_string());
            }
        }
    }
}

fn extract_doubao_utterances(value: &Value) -> Vec<DoubaoUtterance> {
    let mut output = Vec::new();
    if let Some(result) = value.get("result") {
        collect_utterances(result.get("utterances"), &mut output);
        if let Some(items) = result.as_array() {
            for item in items {
                collect_utterances(item.get("utterances"), &mut output);
            }
        }
    }
    output
}

fn collect_utterances(value: Option<&Value>, output: &mut Vec<DoubaoUtterance>) {
    let Some(items) = value.and_then(Value::as_array) else {
        return;
    };

    for item in items {
        let Some(text) = item.get("text").and_then(Value::as_str) else {
            continue;
        };
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        output.push(DoubaoUtterance {
            start_ms: json_u64(item.get("start_time")).unwrap_or(0),
            end_ms: json_u64(item.get("end_time")).unwrap_or(0),
            text: text.to_string(),
            definite: item
                .get("definite")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        });
    }
}

fn json_u64(value: Option<&Value>) -> Option<u64> {
    value.and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_str().and_then(|text| text.parse::<u64>().ok()))
    })
}

fn parse_error_frame(data: &[u8], mut cursor: usize) -> String {
    if data.len() < cursor + 8 {
        return "豆包返回错误，但错误帧不完整".to_string();
    }
    let code = u32::from_be_bytes(data[cursor..cursor + 4].try_into().unwrap_or_default());
    cursor += 4;
    let size = u32::from_be_bytes(data[cursor..cursor + 4].try_into().unwrap_or_default()) as usize;
    cursor += 4;
    let message = data
        .get(cursor..cursor + size)
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .unwrap_or("");
    format!("豆包返回错误：{code} {message}")
}

fn gzip(data: Vec<u8>) -> Result<Vec<u8>, String> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(&data)
        .map_err(|error| error.to_string())?;
    encoder.finish().map_err(|error| error.to_string())
}

#[derive(Debug, Deserialize)]
struct OpenRouterResponse {
    choices: Vec<OpenRouterChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterChoice {
    message: OpenRouterMessage,
}

#[derive(Debug, Deserialize)]
struct OpenRouterMessage {
    content: String,
}

#[derive(Debug, Deserialize)]
struct OpenRouterStreamResponse {
    choices: Vec<OpenRouterStreamChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterStreamChoice {
    delta: OpenRouterDelta,
}

#[derive(Debug, Deserialize)]
struct OpenRouterDelta {
    content: Option<String>,
}
