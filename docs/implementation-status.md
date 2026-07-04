# SparkSpeech 实现状态

更新时间：2026-07-04

## 已完成

- Tauri 2 + React + Vite + TypeScript 项目骨架。
- 主窗口布局：
  - 首页
  - 模型配置
  - Preference
- Notion 风格的浅色工作界面。
- 底部录音胶囊 UI。
- 本地配置读取和保存。
- 本地 Preference 读取和保存。
- 本地历史记录读取和删除。
- Rust 侧系统剪贴板写入。
- Windows 后台右 Alt 监听，使用 low-level keyboard hook。
- 独立透明 overlay 窗口：
  - 录音中：`直接说`
  - ASR 阶段：`文字转写中`
  - 优化阶段：`内容优化中`
- 麦克风录音并保存为 16k mono WAV。
- 录音失败或网络失败时保留音频文件和失败记录。
- 豆包 `bigmodel_async` WebSocket ASR 请求。
- OpenRouter `/api/v1/chat/completions` 文本整理请求。
- 历史记录分页加载，一次 60 条。
- 历史记录操作：
  - 复制
  - 重新转写
  - 重新优化
  - 删除
- 右侧主内容滚动，左侧侧边栏固定。
- 修复豆包 WebSocket 握手，使用标准 client request 自动生成 `Sec-WebSocket-Key`。
- 修复 overlay 透明背景，并移除主窗口内重复录音条。
- overlay 状态改为 Rust 持久保存，overlay 页面会主动读取当前状态，避免窗口显示但胶囊丢失。
- overlay 窗口关闭阴影，避免透明窗口外圈可见。
- 豆包鉴权支持新版 `X-Api-Key` 和旧版 `X-Api-App-Key` + `X-Api-Access-Key`。
- 主窗口增加静态启动加载态，避免 WebView 冷启动时显示纯白。
- 豆包返回 JSON 改为宽松解析，兼容中间包缺少 `text` 字段、`utterances` 文本和数组结果。
- 豆包流式音频请求改为声明 `pcm`，与实际发送的 16k mono PCM 分片一致。
- 使用本地保存的 wav 做过真实 ASR 测试，豆包返回文本成功。
- 新增独立设置页：
  - 麦克风选择
  - 快捷键录制式设置
  - 浅色 / 深色 / 跟随系统主题
  - 保存日志
  - 查看日志
- 录音会使用设置页中选择的麦克风。

## Preference 结构

Preference 已经按 Leo 的要求分成三块：

- 系统提示词：可编辑，有默认值。
- 个性化偏好：可编辑，有默认值。
- 词条替换：单独文本框，一行一个词，也支持 `A → B`。

## 需要本机实测的部分

- 豆包鉴权头和 `Resource ID` 是否与当前账号一致。
- 豆包返回帧在当前模型版本下的最终包标识。
- OpenRouter 是否能按系统代理访问。
- 右 Alt 在 Leo 当前键盘布局下是否会被系统或其他软件拦截。
- 麦克风设备默认采样配置是否正常。
- 自动粘贴尚未实现，本版仍然只复制到剪贴板。

## 本地数据

当前使用 JSON 文件保存到 Tauri 应用数据目录：

- `settings.json`
- `prompts.json`
- `records.json`

后续如果切换到 SQLite，字段结构可以沿用 `docs/product-design.md` 中的数据模型。

## 验证记录

已执行：

```text
npm run build
cargo check
npm run tauri:build -- --no-bundle
```

Tauri 构建已生成：

- `src-tauri/target/release/sparkspeech.exe`
- `src-tauri/target/release/bundle/msi/SparkSpeech_0.1.0_x64_en-US.msi`
- `src-tauri/target/release/bundle/nsis/SparkSpeech_0.1.0_x64-setup.exe`

已用 Playwright 检查页面：

- `output/playwright/home-recording.png`
- `output/playwright/models.png`
- `output/playwright/preferences.png`

## 下一步

1. 和 Leo 一起实测一次真实录音、豆包 ASR、OpenRouter 优化。
2. 根据真实返回帧调整豆包协议解析。
3. 实现自动粘贴。
4. 将 API Key 从普通 JSON 迁移到本地加密或 Windows 凭据。
5. 将历史记录从 JSON 迁移到 SQLite。
