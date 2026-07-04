# SparkSpeech 文档

SparkSpeech 是一个 Windows 优先的个人语音输入工具，用全局快捷键触发录音，将豆包流式 ASR 的识别结果交给 OpenAI-compatible 文本模型整理，最后保存到本地历史，并复制到剪贴板，可选自动粘贴到当前光标位置。

## 文档结构

| 文档 | 用途 |
| --- | --- |
| [产品设计文档](./product-design.md) | 产品范围、核心流程、页面设计、配置项、数据留存和阶段计划 |
| [实现状态](./implementation-status.md) | 当前代码已经完成的能力、模拟部分和下一步实现入口 |
| [Roadmap](./roadmap.md) | 后续版本计划，当前重点是 0.1.3 输入可靠性与可见反馈 |
| [开发与发布流程](./development-workflow.md) | 分支模型、提交规范、验证命令、版本面、发布清单和数据迁移 |
| [0.1.0 发布说明](./release-0.1.0.md) | 首个 Windows 版本包含的功能、数据目录和构建产物 |
| [0.1.1 发布说明](./release-0.1.1.md) | 自动更新、模型设置和体验优化 |
| [0.1.2 发布说明](./release-0.1.2.md) | 豆包长录音可靠性改进和 WAV 拖放导入 |
| [0.1.3 发布说明](./release-0.1.3.md) | 输入可靠性、可见处理进度、多文本优化 Provider 和启动项 |
| [参考文档](./references/) | 外部 API、竞品视觉参考和接口资料，保留原文，不直接改写 |

## 当前技术方向

- 桌面框架：Tauri 2
- 前端：React + Vite + TypeScript
- 平台：Windows
- ASR：火山引擎豆包流式语音识别
- 文本整理：OpenRouter、DeepSeek 或自定义 OpenAI-compatible API
- 输入方式：复制到剪贴板，可选模拟粘贴；不做真正的 Windows IME
- 0.1.3 方向：分段保存录音、完整音频 ASR 进度、文本优化进度、整理强度选择、开机自启动、每日备份

## 文档维护原则

- 产品决策写入 `docs/product-design.md`。
- 外部接口细节保留在 `docs/references/`，实现文档引用它们，不直接覆盖。
- 需求变化先更新文档，再进入代码实现。
- 对未确认的参数，用“建议默认值”标明，后续可以改。
- 面向 Agent 的维护约定写在仓库根目录的 `AGENTS.md`。
