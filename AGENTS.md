# SparkSpeech Agent Notes

## Project

SparkSpeech is a Windows-first personal dictation app built with Tauri 2, React, TypeScript, and Rust.

The app is intentionally small:
- Global shortcut starts and stops recording.
- A transparent bottom overlay shows recording and processing status.
- Audio is saved locally before any network work.
- Doubao streaming ASR turns audio into text.
- OpenRouter-compatible chat completions clean up the ASR text.
- The final text is copied to the clipboard and can be auto-pasted.

## Product Rules

- Windows is the only supported platform for now.
- Doubao is the only ASR provider for `0.1.x`.
- OpenRouter is the only optimization provider for `0.1.x`.
- OpenRouter should use the system proxy by default.
- Recordings must be saved locally before transcription starts.
- A failed transcription must not delete audio.
- No-speech transcription is not an error. Store it as a history record with the `no_speech` status.
- Deleting a history item must ask for confirmation.
- The app should keep running in the system tray after the main window is closed.

## Local Data

The production app stores data under the Tauri app data directory, normally:

`%APPDATA%\com.leo.sparkspeech`

Important files:
- `settings.json`
- `prompts.json`
- `records.json`
- `app.log`
- `recordings/YYYY-MM-DD/*.wav`
- `tests/microphone-test.wav`

## Development

Use these commands from the repository root:

```powershell
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
npm run tauri:build -- --no-bundle
```

The npm/Tauri command currently still creates MSI and NSIS bundles even when `--no-bundle` is passed through npm.

## Branching and Commits

- Check the current Git branch and workspace status before changing code.
- Ongoing version work should happen on a version branch named `dev/x.y.z`, for example `dev/0.1.1`.
- Short-lived focused work can use `feat/<short-name>`, `fix/<short-name>`, or `docs/<short-name>` when it will be merged back into the active `dev/x.y.z` branch.
- Do not develop directly on `main` unless the user explicitly asks for a tiny documentation-only change or an emergency release fix.
- Before editing on `main`, ask which version branch should carry the work.
- Commit messages should use Conventional Commits:
  - `feat:`
  - `fix:`
  - `docs:`
  - `style:`
  - `refactor:`
  - `test:`
  - `chore:`
  - `perf:`
- Keep commits grouped by intent. Good batches are documentation, frontend UI, native backend, release metadata, and build tooling.
- Do not mix unrelated cleanup into feature commits.

## Release Process

When preparing a SparkSpeech release:

1. Confirm the branch and workspace status. Release work should happen from the matching `dev/x.y.z` branch unless the user explicitly chooses another flow.
2. Confirm the version number and update every version surface together:
   - `package.json`
   - `package-lock.json`
   - `src-tauri/Cargo.toml`
   - `src-tauri/tauri.conf.json`
   - `docs/release-x.y.z.md`
3. Write release notes that describe user-visible behavior and important operational notes, not internal trivia.
4. Run verification:
   - `npm run build`
   - `cargo check --manifest-path src-tauri/Cargo.toml`
   - `npm run tauri:build -- --no-bundle`
5. Confirm release artifacts exist:
   - `src-tauri/target/release/sparkspeech.exe`
   - `src-tauri/target/release/bundle/nsis/SparkSpeech_x.y.z_x64-setup.exe`
   - `src-tauri/target/release/bundle/msi/SparkSpeech_x.y.z_x64_en-US.msi`
6. Review the final diff.
7. Commit with a Conventional Commits message.
8. Push the branch and open a pull request into `main`, unless the user explicitly asks for a direct release.
9. After merge, create an annotated tag such as `v0.1.0`.
10. Publish the GitHub Release with the release notes and upload the NSIS/MSI installers.

## Safe Verification

- For frontend changes, run `npm run build`.
- For Rust/native changes, run `cargo check --manifest-path src-tauri/Cargo.toml`.
- For release work, run the full Tauri build.
- For global shortcut, tray, microphone, clipboard, and auto-paste behavior, note that some verification requires a real Windows desktop session.
- If a release build cannot overwrite `sparkspeech.exe`, check whether a local SparkSpeech process is running before rebuilding.

## Coding Guidelines

- Keep changes small and aligned with the existing app structure.
- Prefer Tauri commands for native behavior and keep React focused on UI state.
- Do not delete user recordings or configuration during normal fixes.
- Use structured JSON storage consistently until the project migrates to SQLite.
- Do not commit generated `dist/`, `node_modules/`, `output/`, or Rust `target/`.
- Update docs when product behavior changes.

## Definition of Done

- The requested behavior is implemented or the remaining blocker is clearly stated.
- Relevant docs are updated when product behavior, release steps, storage paths, or developer workflow changes.
- Verification commands have been run and reported.
- The workspace does not contain accidental generated files.
- User data safety is preserved: settings, prompts, records, logs, and recordings are not deleted during normal development.
