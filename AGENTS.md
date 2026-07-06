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
cargo build --manifest-path src-tauri/Cargo.toml --release --features tauri/custom-protocol
```

Do not use `npm run tauri:build` for ordinary local compilation. Tauri does not provide the app's normal local build path in this project; that command enters installer/updater bundling and signing behavior and can fail without release signing keys.

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

When preparing and publishing a SparkSpeech release:

1. Confirm the branch and workspace status. Release work should happen from the matching `dev/x.y.z` branch unless the user explicitly chooses another flow.
2. Confirm the version number and update every version surface together:
   - `package.json`
   - `package-lock.json`
   - `src-tauri/Cargo.toml`
   - `src-tauri/Cargo.lock`
   - `src-tauri/tauri.conf.json`
   - `docs/release-x.y.z.md`
3. Write release notes that describe user-visible behavior and important operational notes, not internal trivia.
4. Prefer the release script for release verification:
   - `npm run release:check -- x.y.z`
   - `npm run release:check -- x.y.z -BuildInstaller` when local signed installer verification is needed.
5. If not using the script, run verification manually:
   - `npm run build`
   - `cargo check --manifest-path src-tauri/Cargo.toml`
   - `cargo test --manifest-path src-tauri/Cargo.toml`
   - Use `npm run release:check -- x.y.z -BuildInstaller` when verifying installer signing locally; do not call `npm run tauri:build` directly.
6. Confirm release artifacts exist after installer verification:
   - `src-tauri/target/release/sparkspeech.exe`
   - `src-tauri/target/release/bundle/nsis/SparkSpeech_x.y.z_x64-setup.exe`
   - `src-tauri/target/release/bundle/nsis/SparkSpeech_x.y.z_x64-setup.exe.sig`
   - `src-tauri/target/release/bundle/msi/SparkSpeech_x.y.z_x64_en-US.msi`
   - `src-tauri/target/release/bundle/msi/SparkSpeech_x.y.z_x64_en-US.msi.sig`
7. Review the final diff.
8. Commit with a Conventional Commits message.
9. Push the release branch and open a pull request into `main`, unless the user explicitly asks for a direct release.
10. Do not stop at PR creation when the user asked to publish. Merge the PR into `main` after checks are acceptable.
11. After merge, switch to `main`, pull, and create an annotated tag such as `v0.1.2`.
12. Do not rely on the GitHub Actions release workflow for building or publishing. The release workflow is too slow for this project flow; build the release artifacts locally and publish them manually.
13. Do not manually trigger the release workflow unless Leo explicitly requests it. The workflow is a backup path only.
14. Push the tag only when the local artifacts are ready to upload. The release workflow is manual-only, so tags should not start cloud builds.
15. Create or update the GitHub Release manually from the local artifacts and confirm it includes:
   - NSIS installer
   - MSI installer
   - updater `.sig` files
   - `latest.json`
16. Report the Release URL, PR URL, tag, and any non-blocking workflow warnings.

## Safe Verification

- For frontend changes, run `npm run build`.
- For Rust/native changes, run `cargo check --manifest-path src-tauri/Cargo.toml`.
- For local release-mode compilation, run `npm run build` and `cargo build --manifest-path src-tauri/Cargo.toml --release --features tauri/custom-protocol`.
- For release work that needs installers or updater signatures, use the release script instead of calling `npm run tauri:build` directly.
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
