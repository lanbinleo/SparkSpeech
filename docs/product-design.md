# SparkSpeech 产品设计文档

## 1. 产品定位

SparkSpeech 是一个给 Leo 自己使用的 Windows 桌面语音输入工具。它替代“闪电说”中最常用的个人工作流：按下全局快捷键开始说话，结束后得到经过整理的文本，并把结果保存到本地历史、复制到剪贴板，必要时自动粘贴到当前应用。

它不是聊天助手，也不是真正的输入法。它只负责把语音转成可直接使用的文本。

## 2. 目标

- 用右 Alt 作为主要全局快捷键，触发开始和结束录音。
- 录音时在屏幕下方显示一个克制的悬浮提示。
- 使用豆包流式 ASR 获取转写文本。
- 使用 OpenRouter 的 OpenAI-compatible API 对 ASR 文本做整理。
- 在首页展示历史识别结果，并保存原始 ASR、整理后文本和临时录音。
- 将整理后文本复制到剪贴板，并支持结束录音后自动粘贴。
- 提供模型配置和自定义 preference 配置。
- 从项目开始保留清晰、一致的文档。

## 3. 非目标

- 不做多用户账号、云同步、团队管理。
- 不做真正的 Windows IME。
- 不支持多家 ASR provider，ASR 只支持豆包流式语音识别。
- 不做复杂的工作区、知识库和模板市场。
- 不在第一阶段支持 macOS 或 Linux。

## 4. 用户场景

### 4.1 快速输入

Leo 在任意应用中按右 Alt，屏幕底部出现录音提示。说完后再按右 Alt，应用停止录音，整理文本，复制到剪贴板。如果启用了自动粘贴，文本会进入当前输入框。

### 4.2 查看历史

Leo 打开主窗口，在首页查看最近的语音输入记录。每条记录显示整理后文本、时间、状态和操作按钮。需要时可以复制、重新整理、重新转写或删除。

### 4.3 重试结果

如果整理结果不满意，Leo 可以基于保留的原始 ASR 重新整理；如果 ASR 结果不满意，并且录音还没有过期，可以使用保留的录音重新转写。

### 4.4 调整个人习惯

Leo 在 preference 页面维护词典和写作习惯，例如英文/数字与中文之间加空格、第三人称代词偏好、口误替换、标点读法、LaTeX 公式和插入语处理。

## 5. 核心流程

```text
右 Alt
  -> 开始录音
  -> 显示底部悬浮提示
  -> 音频分包发送给豆包流式 ASR
  -> 接收实时识别文本

右 Alt 再次按下
  -> 发送最后一包音频
  -> 停止录音
  -> 保存临时录音
  -> 得到最终 ASR 文本
  -> 调用 OpenRouter 整理文本
  -> 保存历史记录
  -> 复制到剪贴板
  -> 按设置决定是否自动粘贴
```

## 6. 功能范围

### 6.1 全局快捷键

- 默认快捷键：右 Alt。
- 行为：第一次按下开始录音，第二次按下结束录音。
- 需要支持快捷键重新绑定，但第一版可以只允许在设置中修改为少量候选按键。
- 右 Alt 在部分键盘布局中可能表现为 AltGr，Windows 实现时需要实测。

### 6.2 录音提示

- 使用独立的透明、无边框、置顶小窗口。
- 默认位置：屏幕底部居中。
- 默认内容：
  - 左侧状态文案：`直接说`
  - 分隔线
  - 右侧音量波形或动态条
- 状态：
  - `idle`：隐藏
  - `recording`：显示录音动效
  - `processing`：显示整理中
  - `error`：短暂显示失败提示，然后回到隐藏
- 视觉参考来自闪电说的胶囊提示，但整体更接近 Notion 的克制、干净和低干扰。

### 6.3 首页

首页是主工作区，展示最近识别结果。

每条记录显示：

- 整理后文本
- 原始 ASR 文本，可折叠查看
- 创建时间
- 处理状态
- 是否已复制、是否已自动粘贴
- 录音是否还可用于重新转写

操作：

- 复制整理后文本
- 重新整理
- 重新转写
- 删除记录
- 展开详情

空状态：

- 未有记录时显示简短说明和当前快捷键。

### 6.4 模型配置

只保留必要配置，避免把个人工具做成 provider 控制台。

#### 豆包 ASR

- API Key
- Resource ID
  - 建议默认值：`volc.seedasr.sauc.duration`
  - 如使用并发版，可改为 `volc.seedasr.sauc.concurrent`
- Endpoint
  - 建议默认值：`wss://openspeech.bytedance.com/api/v3/sauc/bigmodel_async`
- 语言
  - 建议默认值：`zh-CN`
- 测试连接按钮

豆包请求不走系统代理。

#### OpenRouter

- API Key
- Base URL
  - 默认值：`https://openrouter.ai/api/v1`
- Model
  - 默认值待 Leo 确认
- HTTP-Referer
  - 可选
- X-OpenRouter-Title
  - 建议默认值：`SparkSpeech`
- 测试调用按钮

OpenRouter 请求默认走系统代理。

#### 安全

- API Key 存在本地。
- Windows 第一版建议使用系统凭据或本地加密存储。
- 配置导出时默认不包含密钥。

### 6.5 Preference

Preference 页面用于维护文本整理偏好，不改变产品边界，不让模型回答问题或执行指令。

配置分为三块：

- 系统整理规则：固定提示词，默认不在 UI 中频繁编辑。
- 用户词典：人名、学校、产品名、技术名，以及明确的 `A -> B` ASR 错写替换。
- 写作习惯：分段、空格、代词、口误、标点读法、公式、脏话保留和插入语处理。

文本整理请求结构：

- system：固定的“语音输入文本整理器”规则。
- system：用户词典和 preference。
- user：ASR 原文。

输出要求：

- 只输出最终文本。
- 不回答问题。
- 不解释改动。
- 不复制 prompt 内容。
- 不添加前言和后缀。

### 6.6 录音留存

录音需要短期保存，用于重新转写。

建议默认策略：

- 整理后文本和原始 ASR 历史长期保存，直到手动删除。
- 原始录音默认保留 7 天。
- 录音过期后，记录仍保留，但“重新转写”不可用。
- 支持手动立即删除某条记录的录音。
- 后续可以提供全局清理入口。

录音文件命名建议：

```text
recordings/YYYY-MM-DD/{record_id}.wav
```

### 6.7 剪贴板与自动粘贴

- 每次整理成功后，将最终文本复制到剪贴板。
- 设置中提供“整理成功后自动粘贴”开关。
- 自动粘贴使用 Windows 原生能力模拟 `Ctrl+V`。
- 自动粘贴失败时，不影响历史保存和剪贴板复制。

## 7. 数据模型

### 7.1 Record

| 字段 | 说明 |
| --- | --- |
| id | 本地记录 ID |
| created_at | 创建时间 |
| updated_at | 更新时间 |
| raw_asr_text | 原始 ASR 文本 |
| final_text | 整理后文本 |
| audio_path | 临时录音路径，可为空 |
| audio_expires_at | 录音过期时间，可为空 |
| asr_status | ASR 状态 |
| optimize_status | 整理状态 |
| copied_at | 复制到剪贴板时间 |
| pasted_at | 自动粘贴时间，可为空 |
| error_message | 错误信息，可为空 |
| doubao_request_id | 豆包请求 ID |
| doubao_log_id | 豆包返回的 Log ID |
| openrouter_model | 使用的整理模型 |

### 7.2 Settings

| 字段 | 说明 |
| --- | --- |
| global_shortcut | 全局快捷键 |
| auto_paste | 是否自动粘贴 |
| recording_retention_days | 录音保留天数 |
| doubao_resource_id | 豆包 Resource ID |
| doubao_endpoint | 豆包 WebSocket 地址 |
| doubao_language | 识别语言 |
| openrouter_base_url | OpenRouter Base URL |
| openrouter_model | OpenRouter 模型 |
| use_system_proxy_for_openrouter | OpenRouter 是否走系统代理 |

### 7.3 Preference

| 字段 | 说明 |
| --- | --- |
| system_prompt | 固定整理规则 |
| user_dictionary | 用户词典 |
| writing_preferences | 写作习惯 |
| updated_at | 更新时间 |

## 8. 技术方向

### 8.1 桌面框架

推荐使用 Tauri 2：

- Rust 侧负责系统能力：全局快捷键、麦克风、WebSocket、剪贴板、自动粘贴、本地加密和文件管理。
- React 侧负责主界面、设置页、历史记录和 preference 编辑。

### 8.2 ASR

参考 `docs/references/doubao-asr-websocket-techdoc.md`：

- 推荐接口：`bigmodel_async` 双向流式优化版。
- 音频包建议 100 到 200 ms。
- 双向流式优化版只有结果变化时返回新数据包。
- 需要记录 `X-Tt-Logid`，便于排查问题。
- full client request 和 audio only request 使用豆包二进制协议。

建议默认 ASR 参数：

```json
{
  "audio": {
    "format": "wav",
    "rate": 16000,
    "bits": 16,
    "channel": 1,
    "language": "zh-CN"
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
}
```

### 8.3 文本整理

参考 `docs/references/openrouter-api.md`：

- Endpoint：`POST /api/v1/chat/completions`
- 支持 OpenAI-compatible 请求格式。
- 请求头包含 `Authorization: Bearer <OPENROUTER_API_KEY>`。
- 可选请求头：`HTTP-Referer`、`X-OpenRouter-Title`。

### 8.4 本地存储

- SQLite 保存历史记录、设置索引和处理状态。
- 录音文件保存在应用数据目录。
- 密钥使用 Windows 系统能力或本地加密存储。

## 9. 界面设计方向

视觉参考来自 `docs/references/design-doc-notion.md`，但 SparkSpeech 是个人桌面工具，不做营销页。

### 9.1 基调

- 温暖的纸色背景，不使用纯白大面积铺底。
- 主要文字使用接近黑色的墨色。
- 只保留一个结构强调色，建议使用 Notion Blue `#0075de`。
- 功能界面保持安静，避免大面积渐变和过强装饰。

### 9.2 布局

- 左侧窄导航。
- 右侧主内容区。
- 首页以历史记录列表为主。
- 设置页和 preference 页使用表单布局，字段分组清楚。

### 9.3 组件

- 主按钮：蓝色，圆角 8px。
- 普通按钮：白色或浅灰底，1px 细边框。
- 输入框：紧凑，圆角 4px。
- 历史记录：轻边框卡片，避免重阴影。
- 录音悬浮窗：黑色或深色胶囊，底部居中，短文案配动态音量。

## 10. 状态与错误

### 10.1 录音状态

- 未录音
- 正在录音
- 正在停止
- ASR 完成
- 正在整理
- 整理完成
- 失败

### 10.2 常见失败

- 麦克风权限不可用。
- 全局快捷键注册失败。
- 豆包鉴权失败。
- 豆包 WebSocket 连接失败。
- OpenRouter API Key 无效。
- OpenRouter 代理不可用。
- 剪贴板写入失败。
- 自动粘贴失败。

错误展示原则：

- 首页记录失败状态和错误摘要。
- 悬浮提示只展示短状态，不展示长错误。
- 调试信息进入日志，不打扰正常使用。

## 11. 第一阶段范围

第一阶段目标是完成个人可用版本：

1. Tauri + React 项目骨架。
2. 首页、模型配置、Preference 三个页面。
3. 右 Alt 全局快捷键。
4. 悬浮录音提示。
5. 麦克风录音和本地临时保存。
6. 豆包流式 ASR。
7. OpenRouter 文本整理。
8. 历史记录保存。
9. 复制到剪贴板。
10. 自动粘贴开关。

## 12. 待确认事项

- OpenRouter 默认模型。
- 录音默认保留时间是否使用 7 天。
- 自动粘贴是否默认开启。
- Preference 是否需要版本历史。
- 重新整理时是否允许选择不同模型。
- 首页是否需要全文搜索。
