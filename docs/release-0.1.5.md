# SparkSpeech 0.1.5 Release Notes

## Highlights

- Added model list management for DeepSeek and custom OpenAI-compatible providers.
- The text model selector can now show multiple saved models for OpenRouter, DeepSeek, and custom OpenAI-compatible providers.
- Improved fast ASR finalization fallback timing so SparkSpeech returns to complete-audio transcription sooner when the streaming final result is not available quickly.
- Kept local WAV saving ahead of transcription, and delayed exposing the recording file path in history until the WAV has been written successfully.
- Smoothed transcription overlay progress so the progress ring moves naturally while SparkSpeech waits for the final ASR result.

## Update Notes

DeepSeek and custom OpenAI-compatible providers now use the same model manager pattern as OpenRouter. Add a model in the provider dialog, set it as current, then save the model configuration.

Fast ASR finalization remains experimental. When the realtime final result is slow, disconnected, or unavailable, SparkSpeech keeps the saved local WAV and uses the complete-audio ASR flow.

## Verification

Recommended checks before tagging:

```powershell
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run release:check -- 0.1.5 -BuildInstaller
```
