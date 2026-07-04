use std::{io::Write, path::Path};

use flate2::{write::GzEncoder, Compression};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message},
};
use uuid::Uuid;

use crate::{
    models::{AppSettings, PromptSettings},
    recorder::read_wav_pcm_16k,
};

pub async fn transcribe_audio(
    settings: &AppSettings,
    audio_path: &Path,
) -> Result<(String, Option<String>, String), String> {
    let pcm = read_wav_pcm_16k(audio_path)?;
    if pcm.is_empty() {
        return Err("录音文件为空".to_string());
    }

    let request_id = Uuid::new_v4().to_string();
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

    let (mut ws, response) = connect_async(request).await.map_err(|error| {
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

    let init_payload = json!({
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
    });

    ws.send(Message::Binary(
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
    let chunk_size = 16_000 / 5 * 2;
    let mut chunks = bytes.chunks(chunk_size).peekable();
    while let Some(chunk) = chunks.next() {
        let is_last = chunks.peek().is_none();
        ws.send(Message::Binary(
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
    }

    let mut latest_text = String::new();
    while let Some(message) = ws.next().await {
        let message = message.map_err(|error| error.to_string())?;
        let Message::Binary(data) = message else {
            continue;
        };
        let response = parse_server_frame(&data)?;
        if let Some(text) = response.text {
            if !text.trim().is_empty() {
                latest_text = text;
            }
        }
        if response.is_last {
            break;
        }
    }

    Ok((latest_text, log_id, request_id))
}

pub async fn optimize_text(
    settings: &AppSettings,
    prompts: &PromptSettings,
    raw_asr_text: &str,
) -> Result<String, String> {
    if settings.openrouter_api_key.trim().is_empty() {
        return Err("OpenRouter API Key 为空".to_string());
    }

    let mut client_builder = reqwest::Client::builder();
    if !settings.use_system_proxy_for_openrouter {
        client_builder = client_builder.no_proxy();
    }
    let client = client_builder.build().map_err(|error| error.to_string())?;

    let url = format!(
        "{}/chat/completions",
        settings.openrouter_base_url.trim_end_matches('/')
    );
    let system_prompt = format!(
        "{}\n\n# 用户词典\n{}\n\n# 个性化偏好\n{}",
        prompts.system_prompt.trim(),
        prompts.replacements.trim(),
        prompts.writing_preferences.trim()
    );

    let mut request = client
        .post(url)
        .bearer_auth(&settings.openrouter_api_key)
        .header("X-OpenRouter-Title", &settings.openrouter_title)
        .json(&json!({
            "model": settings.openrouter_model,
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": format!("ASR 原文：\n{raw_asr_text}") }
            ]
        }));

    if !settings.openrouter_http_referer.trim().is_empty() {
        request = request.header("HTTP-Referer", &settings.openrouter_http_referer);
    }

    let response = request.send().await.map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("OpenRouter 请求失败：{status} {text}"));
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
        .ok_or_else(|| "OpenRouter 没有返回可用文本".to_string())
}

pub async fn test_openrouter(settings: &AppSettings) -> Result<String, String> {
    if settings.openrouter_api_key.trim().is_empty() {
        return Err("OpenRouter API Key 为空".to_string());
    }

    let mut client_builder = reqwest::Client::builder();
    if !settings.use_system_proxy_for_openrouter {
        client_builder = client_builder.no_proxy();
    }
    let client = client_builder.build().map_err(|error| error.to_string())?;
    let url = format!(
        "{}/chat/completions",
        settings.openrouter_base_url.trim_end_matches('/')
    );

    let mut request = client
        .post(url)
        .bearer_auth(&settings.openrouter_api_key)
        .header("X-OpenRouter-Title", &settings.openrouter_title)
        .json(&json!({
            "model": settings.openrouter_model,
            "messages": [
                { "role": "user", "content": "ping" }
            ],
            "max_tokens": 4
        }));

    if !settings.openrouter_http_referer.trim().is_empty() {
        request = request.header("HTTP-Referer", &settings.openrouter_http_referer);
    }

    let response = request.send().await.map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(format!("OpenRouter 测试失败：{status} {text}"));
    }

    Ok("OpenRouter 连接可用".to_string())
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
    Ok(ParsedServerFrame {
        text: extract_doubao_text(&parsed),
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
