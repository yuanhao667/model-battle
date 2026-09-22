# Model Battle

仅面向 Apple Silicon Mac 的本地多模型对比工具。输入一次任务，即可并行调用多个同类型模型，并在同一界面比较输出、耗时和 Token 用量。

## 功能

- 支持文本、图片、音频和视频模型对比
- 内置 AIHubMix 和硅基流动模型目录
- 支持自定义 OpenAI 兼容连接
- 多模型并行运行，可手动结束当前任务
- 图片和视频支持参考图，图片支持统一生成比例
- API Key 仅保存在 macOS Keychain

## 技术栈

- Tauri 2 + Rust
- React 19 + TypeScript
- Vite 7
- Vitest + Testing Library

## 环境

- Apple Silicon Mac，macOS 14+
- Node.js 24 LTS 与 npm
- Rust stable
- Xcode Command Line Tools

## 安装

从 [GitHub Releases](https://github.com/yuanhao667/model-battle/releases) 下载 Apple Silicon 版本的 `.dmg`，打开后将 `Model Battle.app` 拖入“应用程序”文件夹即可。

当前版本未使用 Apple Developer ID 签名或公证。首次打开时如被 macOS 拦截，请在 Finder 中右键点击 `Model Battle.app`，选择“打开”。

## 启动

```bash
npm install
npm run tauri dev
```

开发页面固定使用 `http://localhost:1420`，产品运行不依赖常驻本地服务。

## 检查

```bash
npm run typecheck
npm test
npm run build
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

## 构建 DMG

```bash
npm run tauri build
```

构建产物位于 `src-tauri/target/release/bundle/dmg/`。

## 项目结构

```text
.
├── src/                 # React 界面与前端测试
├── src-tauri/           # Tauri/Rust 本地能力与打包配置
├── README.md            # 项目说明
└── package.json         # 前端脚本与依赖
```

## 真实模型冒烟

1. 在“模型配置”中选择 AIHubMix 或硅基流动，只填写 API Key 后验证并加载模型；也可添加自定义单模型连接。
2. 连接成功后，使用文本、图片、音频或视频分类筛选目录，勾选需要加入竞技场的同类模型；音频目录同时加载文本转音频和音频转文本模型，并按任务方向分组，能力标签不由用户手动配置。
3. 返回“模型 Battle”输入提示词；图片和视频竞技场可以添加一张参考图片并统一设置生成比例，音频竞技场可以统一选择音色。
4. 一次运行当前类型下全部已启用模型，检查输出、耗时和真实 Token；接口不返回 usage 时显示 `--`。
5. 完全退出后重启，模型配置应保留，上一轮提示词和结果应清空。

### 音频 Battle 验收

- 文本转音频：标准 TTS 模型调用 `/audio/speech`；AIHubMix 的 `gpt-4o-audio-preview` 按其要求调用 `/chat/completions` 并解析 Base64 WAV。
- 硅基流动：系统音色会转换成模型限定的 `模型 ID:音色` 格式；MOSS-TTSD 的单说话人输入会补齐 `[S1]` 标记。
- 音频转文本：上传使用 `/audio/transcriptions`，支持 mp3、mp4、mpeg、mpga、m4a、wav、webm，统一限制为 25 MB，并保留可供服务端识别的文件扩展名。
- 自动化测试只验证界面、请求路由、请求体和响应解析。发布前仍需分别使用有效的 AIHubMix、硅基流动 Key 完成一次 TTS 与 STT 真机冒烟；这一步会产生实际 API 调用和费用。

API Key 仅进入 macOS Keychain，不得写入文档、`.env`、终端命令或 Git。

非敏感模型配置保存在本地 JSON 文件中，并保留备份用于损坏恢复。

## 当前边界

当前开放文本、图片、音频和视频四类竞技场；同一轮只允许一种输出类型，但各类型模型的启用状态互不影响。应用不保存运行历史；自定义连接使用 OpenAI 兼容的文本、图片、音频与视频协议，厂商若采用不同端点需增加专用适配。
