# SparkSpeech 0.1.2 Release Notes

## Highlights

- Improved Doubao streaming ASR reliability for longer recordings.
- SparkSpeech now reads Doubao WebSocket responses while sending audio, instead of waiting until every audio chunk has been sent.
- Audio chunks are still sent through the realtime streaming API, with a short 10ms interval to avoid server-side packet wait timeouts.
- Added drag-and-drop WAV import on the main window. Imported files are copied into the recordings directory and processed like normal recordings.
- Recordings are still saved locally before transcription starts.

## Update Notes

This release fixes failures like:

```text
豆包返回错误：45000081 {"error":"[Timeout waiting next packet] waiting next packet timeout: 8.000000 seconds, session has ended"}
```

The fix keeps Doubao streaming ASR as the only ASR provider and does not switch to file recognition or delayed asynchronous transcription.

Drag-and-drop import currently supports WAV files because SparkSpeech's local transcription pipeline reads WAV/PCM audio before sending it to Doubao.

## Verification

Recommended checks before tagging:

```powershell
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
npm run tauri:build -- --no-bundle
```
