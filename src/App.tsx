import { ChangeEvent, FormEvent, MouseEvent as ReactMouseEvent, memo, useEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import DOMPurify from "dompurify";
import { marked } from "marked";
import "./App.css";

type AppError = {
  code: string;
  message: string;
  retryable: boolean;
  providerStatus?: number;
};

type ProviderKind = "aihubmix" | "siliconflow" | "openai_compatible";
type OutputType = "text" | "image" | "audio" | "audio_to_text" | "video";
type ModelCategory = "text" | "image" | "audio" | "video";
type CatalogFilter = OutputType | "all";
type ReasoningFilter = "all" | "reasoning" | "non_reasoning";

type CatalogModel = {
  modelId: string;
  outputType: OutputType;
  supportsReferenceImage: boolean;
};

type ModelConfig = {
  id: string;
  modelId: string;
  displayName: string;
  outputType: OutputType;
  supportsReferenceImage: boolean;
  enabled: boolean;
};

type Connection = {
  id: string;
  displayName: string;
  providerKind: ProviderKind;
  baseUrl: string;
  hasCredential: boolean;
  credentialLength?: number;
  models: ModelConfig[];
};

type ProviderCard = {
  key: string;
  kind: ProviderKind;
  title: string;
  sub: string;
  connection?: Connection;
};

type ConnectionSaveResult = {
  connection: Connection;
  skippedModels: Array<{ modelId: string; reason: string }>;
};

type AppConfig = {
  schemaVersion: number;
  activeArenaType: OutputType;
  connections: Connection[];
  systemPrompts?: Partial<Record<OutputType, string>>;
};

const systemPromptHints: Record<OutputType, string> = {
  text: "作为 system 消息发送给所有文本模型，留空即视为未设置。",
  image: "会拼接在提示词前面，发给所有图片生成模型，留空即视为未设置。",
  video: "会拼接在提示词前面，发给所有视频生成模型，留空即视为未设置。",
  audio: "语音合成接口没有系统提示词，仅 AIHubMix 的对话语音模型会收到。",
  audio_to_text: "会作为识别提示（prompt）发给所有转写模型，留空即视为未设置。",
};

type Usage = {
  inputTokens?: number;
  outputTokens?: number;
  totalTokens?: number;
  reasoningTokens?: number;
};

type ModelResult = {
  runId: string;
  modelConfigId: string;
  status: "running" | "completed" | "failed" | "ended";
  outputText?: string;
  /** 推理模型的思考过程，流式输出时才有。 */
  outputReasoning?: string;
  outputImage?: string;
  outputAudio?: string;
  outputVideo?: string;
  /** 从发出请求到第一个字的耗时。 */
  firstTokenMs?: number;
  elapsedMs: number;
  usage?: Usage;
  error?: AppError;
};

/** 流式输出时后端按批推过来的增量内容。 */
type ModelTextDelta = {
  runId: string;
  modelConfigId: string;
  content: string;
  reasoning: string;
  elapsedMs: number;
};

type RunFinished = {
  runId: string;
  status: "completed" | "ended";
  elapsedMs: number;
};

type AutoDisabledModel = {
  modelConfigId: string;
  modelId: string;
  providerName: string;
  reason: string;
};

type EditorState = {
  connectionId?: string;
  hasCredential: boolean;
  providerKind: ProviderKind;
  modelCategory: ModelCategory;
  outputType: OutputType;
  catalogFilter: CatalogFilter;
  displayName: string;
  baseUrl: string;
  apiKey: string;
  replaceCredential: boolean;
  storedMask: string;
  modelId: string;
  savedModels: ModelConfig[];
  connected: boolean;
  catalog: CatalogModel[];
  catalogQuery: string;
  parameterLimitEnabled: boolean;
  maxParameterBillions: string;
  reasoningFilter: ReasoningFilter;
  selectedModels: string[];
  imageCapableModels: string[];
  validationToken: string;
};

const emptyConfig: AppConfig = { schemaVersion: 1, activeArenaType: "text", connections: [] };
const credentialMask = "••••••••";
// 已保存的 Key 按真实位数渲染圆点；后端没给长度时退回固定掩码。
function maskOf(connection: Connection) {
  const length = connection.credentialLength;
  return connection.hasCredential && length && length > 0 ? "•".repeat(length) : credentialMask;
}
const providerDefaults: Record<ProviderKind, { name: string; baseUrl: string }> = {
  aihubmix: { name: "AIHubMix", baseUrl: "https://aihubmix.com/v1" },
  siliconflow: { name: "硅基流动", baseUrl: "https://api.siliconflow.cn/v1" },
  openai_compatible: { name: "自定义模型", baseUrl: "" },
};

const settingsGroups: Array<{ label: string; outputType: OutputType }> = [
  { label: "文本生成模型", outputType: "text" },
  { label: "图片生成模型", outputType: "image" },
  { label: "文本转音频", outputType: "audio" },
  { label: "音频转文本", outputType: "audio_to_text" },
  { label: "视频生成模型", outputType: "video" },
];
const audioFileAccept = ".mp3,.mp4,.mpeg,.mpga,.m4a,.wav,.webm,audio/mpeg,audio/mp4,audio/wav,audio/webm,video/mp4,video/webm";
const supportedAudioExtension = /\.(mp3|mp4|mpeg|mpga|m4a|wav|webm)$/i;

function audioMimeType(fileName: string) {
  const extension = fileName.split(".").pop()?.toLowerCase();
  if (extension === "mp4" || extension === "m4a") return "audio/mp4";
  if (extension === "wav") return "audio/wav";
  if (extension === "webm") return "audio/webm";
  return "audio/mpeg";
}

function TrashIcon() {
  return <svg viewBox="0 0 24 24" aria-hidden="true">
    <path d="M4 7h16M9 7V4h6v3m3 0-1 13H7L6 7m4 4v5m4-5v5" />
  </svg>;
}

function EditIcon() {
  return <svg viewBox="0 0 24 24" aria-hidden="true">
    <path d="m14.5 5.5 4 4M4 20l4.4-1 10.1-10.1a2.1 2.1 0 0 0-3-3L5.4 16 4 20Z" />
  </svg>;
}

function PlusIcon() {
  return <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M8 3v10M3 8h10" /></svg>;
}

function CloseIcon() {
  return <svg viewBox="0 0 16 16" aria-hidden="true"><path d="m4.5 4.5 7 7m0-7-7 7" /></svg>;
}

function DownloadIcon() {
  return <svg viewBox="0 0 20 20" aria-hidden="true">
    <path d="M10 3v9m-3-3 3 3 3-3M4 15v2h12v-2" />
  </svg>;
}

function RefreshIcon() {
  return <svg viewBox="0 0 20 20" aria-hidden="true">
    <path d="M16 6.5V3.5l-1.6 1.6A6.2 6.2 0 0 0 3.8 10" />
    <path d="M4 13.5v3l1.6-1.6A6.2 6.2 0 0 0 16.2 10" />
  </svg>;
}

function CopyIcon() {
  return <svg viewBox="0 0 20 20" aria-hidden="true">
    <rect x="7" y="7" width="9" height="10" rx="1.6" />
    <path d="M13 4.5H5.6A1.6 1.6 0 0 0 4 6.1V13" />
  </svg>;
}

function ToastIcon({ success }: { success: boolean }) {
  return <svg viewBox="0 0 20 20" aria-hidden="true">
    <circle cx="10" cy="10" r="7.5" />
    {success ? <path d="m6.5 10 2.2 2.2 4.8-4.8" /> : <path d="M10 6.2v4.8m0 2.8h.01" />}
  </svg>;
}

function SelectMenu({
  label,
  value,
  options,
  disabled,
  onChange,
}: {
  label: string;
  value: string;
  options: Array<{ value: string; label: string }>;
  disabled?: boolean;
  onChange: (value: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const selected = options.find((option) => option.value === value)?.label ?? value;

  useEffect(() => {
    if (!open) return;
    const closeOutside = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("pointerdown", closeOutside);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOutside);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [open]);

  return <div className={`select-menu ${open ? "is-open" : ""}`} ref={rootRef}>
    <button
      type="button"
      className="select-trigger"
      aria-label={label}
      aria-haspopup="listbox"
      aria-expanded={open}
      disabled={disabled}
      onClick={() => setOpen((current) => !current)}
    >
      <span>{label}</span>
      <strong>{selected}</strong>
      <svg viewBox="0 0 16 16" aria-hidden="true"><path d="m4 6 4 4 4-4" /></svg>
    </button>
    {open && <div className="select-popover" role="listbox" aria-label={label}>
      {options.map((option) => <button
        type="button"
        role="option"
        aria-selected={option.value === value}
        className={option.value === value ? "is-active" : ""}
        key={option.value}
        onClick={() => {
          onChange(option.value);
          setOpen(false);
        }}
      >{option.label}</button>)}
    </div>}
  </div>;
}

function SystemPromptControl({
  outputType,
  systemPrompt,
  onSave,
}: {
  outputType: OutputType;
  systemPrompt?: string | null;
  onSave: (value: string) => Promise<void>;
}) {
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState(systemPrompt ?? "");
  const [saving, setSaving] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const hasSystemPrompt = Boolean(systemPrompt?.trim());

  useEffect(() => {
    setDraft(systemPrompt ?? "");
  }, [systemPrompt]);

  useEffect(() => {
    if (!open) return;
    const closeOutside = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("pointerdown", closeOutside);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOutside);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [open]);

  async function submit(value: string) {
    setSaving(true);
    try {
      await onSave(value);
      setOpen(false);
    } catch {
      // 失败时保留面板，方便修改后重试。
    } finally {
      setSaving(false);
    }
  }

  return <div className={`system-prompt-field ${open ? "is-open" : ""}`} ref={rootRef}>
    <button
      type="button"
      className="system-prompt-control"
      aria-haspopup="dialog"
      aria-expanded={open}
      title={hasSystemPrompt ? "已设置系统提示词，点击编辑" : "尚未设置系统提示词，点击设置"}
      onClick={() => setOpen((current) => !current)}
    >
      <i className={`system-prompt-dot ${hasSystemPrompt ? "is-set" : ""}`} aria-hidden="true" />
      <span className="system-prompt-label">系统提示词</span>
    </button>
    {open && <div className="system-prompt-popover" role="dialog" aria-label="设置系统提示词">
      <p className="system-prompt-hint">{systemPromptHints[outputType]}</p>
      <textarea
        className="system-prompt-input"
        value={draft}
        autoFocus
        placeholder="例如：你是严谨的技术编辑，回答先给结论，再给依据。"
        onChange={(event) => setDraft(event.target.value)}
      />
      <div className="system-prompt-actions">
        <button
          type="button"
          className="secondary-button"
          disabled={saving || !hasSystemPrompt}
          onClick={() => void submit("")}
        >清空</button>
        <button
          type="button"
          className="primary-button"
          disabled={saving}
          onClick={() => void submit(draft)}
        >{saving ? "保存中…" : "保存"}</button>
      </div>
    </div>}
  </div>;
}

function newEditor(providerKind: ProviderKind): EditorState {
  const defaults = providerDefaults[providerKind];
  return {
    hasCredential: false,
    providerKind,
    modelCategory: "text",
    outputType: "text",
    catalogFilter: "text",
    displayName: defaults.name,
    baseUrl: defaults.baseUrl,
    apiKey: "",
    replaceCredential: true,
    storedMask: credentialMask,
    modelId: "",
    savedModels: [],
    connected: false,
    catalog: [],
    catalogQuery: "",
    parameterLimitEnabled: false,
    maxParameterBillions: "14",
    reasoningFilter: "all",
    selectedModels: [],
    imageCapableModels: [],
    validationToken: "",
  };
}

function messageFrom(error: unknown) {
  if (typeof error === "object" && error && "message" in error) {
    return String((error as AppError).message);
  }
  return "操作失败，请稍后重试。";
}

function metric(value?: number) {
  return value === undefined ? "--" : value.toLocaleString();
}

function mediaSrc(value: string) {
  return value.startsWith("data:") || value.startsWith("http") ? value : convertFileSrc(value);
}

function providerLabel(kind: ProviderKind) {
  return providerDefaults[kind].name;
}

function modelParameterBillions(modelId: string) {
  const pattern = /(\d+(?:\.\d+)?)\s*(?:x|×)\s*(\d+(?:\.\d+)?)\s*b(?![a-z0-9])|(\d+(?:\.\d+)?)\s*b(?![a-z0-9])/gi;
  const sizes: number[] = [];
  for (const match of modelId.matchAll(pattern)) {
    const size = match[1] && match[2]
      ? Number(match[1]) * Number(match[2])
      : Number(match[3]);
    if (Number.isFinite(size)) sizes.push(size);
  }
  return sizes.length ? Math.max(...sizes) : undefined;
}

function isModelAtMostBillions(modelId: string, maximum: number) {
  const parameterBillions = modelParameterBillions(modelId);
  return parameterBillions !== undefined && parameterBillions <= maximum;
}

function modelParameterLabel(modelId: string) {
  const parameterBillions = modelParameterBillions(modelId);
  return parameterBillions === undefined ? undefined : `${parameterBillions}B`;
}

function modelCapabilityLabels(model: CatalogModel) {
  const labels = capabilityLabels(model.outputType, model.supportsReferenceImage);
  const parameterLabel = modelParameterLabel(model.modelId);
  if (parameterLabel) labels.splice(Math.max(labels.length - 1, 0), 0, parameterLabel);
  return labels;
}

function isReasoningModel(modelId: string) {
  const normalized = modelId.toLowerCase();
  return [
    /(^|[\/_-])(?:deepseek-)?r1(?:$|[\/_\-.])/,
    /(^|[\/_-])qwq(?:$|[\/_\-.])/,
    /(^|[\/_-])(?:o1|o3|o4)(?:$|[\/_\-.])/,
    /(^|[\/_-])glm-z1(?:$|[\/_\-.])/,
    /(^|[\/_-])magistral(?:$|[\/_\-.])/,
    /reasoner|reasoning|thinking/,
  ].some((pattern) => pattern.test(normalized));
}

function modelCategoryLabel(model: ModelConfig) {
  return categoryLabel(categoryOfModel(model));
}

function categoryOfModel(model: ModelConfig): ModelCategory {
  return categoryOfOutput(model.outputType);
}

function categoryOfOutput(outputType: OutputType): ModelCategory {
  return outputType === "audio_to_text" ? "audio" : outputType;
}

function categoryLabel(category: ModelCategory) {
  return {
    text: "文本生成模型",
    image: "图片生成模型",
    audio: "音频模型",
    video: "视频生成模型",
  }[category];
}

function arenaTypeLabel(outputType: OutputType) {
  return {
    text: "文本生成模型",
    image: "图片生成模型",
    audio: "文本转音频",
    audio_to_text: "音频转文本",
    video: "视频生成模型",
  }[outputType];
}

function capabilityLabels(outputType: OutputType, supportsReferenceImage: boolean) {
  if (outputType === "text") return ["文本生成", ...(supportsReferenceImage ? ["支持图片理解"] : [])];
  if (outputType === "image") return ["文生图", ...(supportsReferenceImage ? ["支持参考图"] : [])];
  if (outputType === "video") return ["文生视频", ...(supportsReferenceImage ? ["支持参考图"] : [])];
  return [outputType === "audio" ? "文本转音频" : "音频转文本"];
}

function categoryOutputType(category: ModelCategory): OutputType {
  return category;
}

function connectionTypeLabel(models: ModelConfig[]) {
  return [...new Set(models.map(modelCategoryLabel))].join(" / ");
}

type VoiceOption = { value: string; label: string };

// 硅基流动系统预置的 8 个音色（官方文档给的名称和特点），
// 界面上显示的名字就是实际发给接口的音色名，不再用别家的占位名。
const siliconflowVoices: VoiceOption[] = [
  { value: "alex", label: "alex（沉稳男声）" },
  { value: "benjamin", label: "benjamin（低沉男声）" },
  { value: "charles", label: "charles（磁性男声）" },
  { value: "david", label: "david（欢快男声）" },
  { value: "anna", label: "anna（沉稳女声）" },
  { value: "bella", label: "bella（激情女声）" },
  { value: "claire", label: "claire（温柔女声）" },
  { value: "diana", label: "diana（欢快女声）" },
];

// OpenAI 兼容语音接口的音色名（AIHubMix 的 tts 系列、自定义连接）。
const openAiVoices: VoiceOption[] = [
  { value: "alloy", label: "alloy（中性）" },
  { value: "echo", label: "echo（沉稳）" },
  { value: "fable", label: "fable（叙事）" },
  { value: "onyx", label: "onyx（深沉）" },
  { value: "nova", label: "nova（明亮）" },
  { value: "shimmer", label: "shimmer（轻柔）" },
];

// AIHubMix 上通义千问语音合成的音色名（平台原样返回，不做二次包装）。
const qwenTtsVoices: VoiceOption[] = [
  { value: "longanfengyue", label: "longanfengyue" },
  { value: "loongjohn", label: "loongjohn" },
  { value: "longanyuanfei", label: "longanyuanfei" },
  { value: "longchuanshu_v3.6", label: "longchuanshu_v3.6" },
  { value: "longanxiaoxin", label: "longanxiaoxin" },
  { value: "longanlingxi", label: "longanlingxi" },
  { value: "longanlufeng", label: "longanlufeng" },
  { value: "longanlingxin", label: "longanlingxin" },
];

/** 音色选项要跟着实际用的模型走，取不到真实音色列表时宁可不显示这个控件。 */
function voiceOptionsFor(models: Array<{ connection: Connection; model: ModelConfig }>): VoiceOption[] {
  const first = models[0];
  if (!first) return [];
  if (first.model.modelId.toLowerCase().includes("qwen-audio-3.0-tts")) return qwenTtsVoices;
  if (first.connection.providerKind === "siliconflow") return siliconflowVoices;
  return openAiVoices;
}

marked.setOptions({ gfm: true, breaks: true });

/** 模型输出基本是 Markdown，渲染成 HTML 后再消毒，避免把模型的脏数据带进界面。 */
function renderMarkdown(text: string) {
  const html = marked.parse(text, { async: false }) as string;
  return DOMPurify.sanitize(html);
}

const MarkdownText = memo(function MarkdownText({ text }: { text: string }) {
  const html = useMemo(() => renderMarkdown(text), [text]);
  return <div className="markdown-body" dangerouslySetInnerHTML={{ __html: html }} />;
});

function secondsText(elapsedMs?: number) {
  return elapsedMs === undefined ? "-- 秒" : `${(elapsedMs / 1000).toFixed(1)} 秒`;
}

/** 从待显示缓冲区里取出一小段字符，让文字看起来像在逐字打出来。 */
function revealChunk(entry: { content: string; reasoning: string }, key: "content" | "reasoning") {
  const value = entry[key];
  if (!value) return "";
  const size = Math.max(4, Math.ceil(value.length / 6));
  entry[key] = value.slice(size);
  return value.slice(0, size);
}

/** 出字速度：推理模型的思考 token 也算在内，所以从“首字之后”开始算更公平。 */
function tokensPerSecond(result?: ModelResult) {
  const outputTokens = result?.usage?.outputTokens;
  if (!result || result.status !== "completed" || !outputTokens || !result.elapsedMs) return undefined;
  const firstToken = result.firstTokenMs;
  const generationMs = firstToken !== undefined && result.elapsedMs > firstToken
    ? result.elapsedMs - firstToken
    : result.elapsedMs;
  return generationMs > 0 ? outputTokens / (generationMs / 1000) : undefined;
}

function speedText(result?: ModelResult) {
  const speed = tokensPerSecond(result);
  return speed === undefined ? undefined : `${speed.toFixed(1)} tok/s`;
}

function ReasoningBlock({ text, hasAnswer }: { text: string; hasAnswer: boolean }) {
  const [open, setOpen] = useState(false);
  // 答案还没开始前把思考过程直接翻开展示，让用户知道模型没卡住。
  if (!hasAnswer) return <div className="reasoning-live">
    <span className="reasoning-label">思考中…</span>
    <p className="reasoning-text">{text}</p>
  </div>;
  return <div className={`reasoning-block ${open ? "is-open" : ""}`}>
    <button type="button" className="reasoning-toggle" aria-expanded={open} onClick={() => setOpen((current) => !current)}>
      {open ? "收起思考过程" : "查看思考过程"}
    </button>
    {open && <p className="reasoning-text">{text}</p>}
  </div>;
}

function ResultCard({
  model,
  connectionName,
  result,
  running,
  showTokenMetrics,
  canRegenerate,
  onRegenerate,
  onCopy,
  onDownloadAudio,
}: {
  model: ModelConfig;
  connectionName: string;
  result?: ModelResult;
  running: boolean;
  showTokenMetrics: boolean;
  canRegenerate: boolean;
  onRegenerate: (model: ModelConfig) => void;
  onCopy: (result: ModelResult, model: ModelConfig) => void;
  onDownloadAudio: (dataUrl: string, modelName: string) => void;
}) {
  const status = result?.status ?? (running ? "running" : "idle");
  const statusText = {
    idle: "待运行", running: "生成中", completed: "已完成", failed: "失败", ended: "已结束",
  }[status];
  const speed = speedText(result);
  const reasoning = result?.outputReasoning;
  // 和复制/下载一样：这一张卡还没有结果时，不显示"重新生成"；
  // 已经有别的模型在跑的时候也不显示，免得给出一个点不动的图标。
  const showRegenerate = Boolean(result) && !running;
  const hasActions = showRegenerate || Boolean(result?.outputText) || Boolean(result?.outputAudio);

  return (
    <article className="model-card result-card" key={model.id}>
      <header data-model-name={model.displayName}>
        <h2
          onMouseEnter={(event) => {
            event.currentTarget.toggleAttribute(
              "data-truncated",
              event.currentTarget.scrollWidth > event.currentTarget.clientWidth,
            );
          }}
        >
          {model.displayName}
        </h2>
        <span className={`status status-${status}`}>
          <i className="status-dot" aria-hidden="true" />
          {statusText}
        </span>
      </header>
      <div className="result-content">
        {reasoning && <ReasoningBlock text={reasoning} hasAnswer={Boolean(result?.outputText)} />}
        {result?.outputText && <MarkdownText text={result.outputText} />}
        {result?.outputImage && <img src={result.outputImage} alt={`${model.displayName} 生成结果`} />}
        {result?.outputAudio && <audio src={mediaSrc(result.outputAudio)} controls preload="metadata" />}
        {result?.outputVideo && <video src={mediaSrc(result.outputVideo)} controls preload="metadata" />}
        {result?.error && <p className="error-copy" role="alert">{result.error.message}</p>}
      </div>
      <footer title={`连接：${connectionName}`}>
        {/* 流式输出时用增量事件带回来的时间，边生成边跳动。 */}
        <span>{secondsText(result && (result.status !== "running" || result.elapsedMs > 0) ? result.elapsedMs : undefined)}</span>
        {speed && <span>{speed}</span>}
        {showTokenMetrics && <span>输出 {metric(result?.usage?.outputTokens)}</span>}
        {hasActions && <span className="card-actions">
          {showRegenerate && <button
            type="button"
            className="card-action"
            data-tip="重新生成"
            aria-label={`重新生成 ${model.displayName}`}
            disabled={!canRegenerate}
            onClick={() => onRegenerate(model)}
          ><RefreshIcon /></button>}
          {result?.outputText && <button
            type="button"
            className="card-action"
            data-tip="复制回答"
            aria-label={`复制 ${model.displayName} 的回答`}
            onClick={() => onCopy(result, model)}
          ><CopyIcon /></button>}
          {result?.outputAudio && <button
            type="button"
            className="card-action"
            data-tip="下载音频"
            aria-label={`下载 ${model.displayName} 音频`}
            onClick={() => onDownloadAudio(result.outputAudio!, model.displayName)}
          ><DownloadIcon /></button>}
        </span>}
      </footer>
    </article>
  );
}

export default function App() {
  const [config, setConfig] = useState<AppConfig>(emptyConfig);
  const [loading, setLoading] = useState(true);
  const [activePage, setActivePage] = useState<"arena" | "settings">("arena");
  const [settingsOutputType, setSettingsOutputType] = useState<OutputType>("text");
  const [sourceFilters, setSourceFilters] = useState<ProviderKind[]>(["aihubmix", "siliconflow", "openai_compatible"]);
  const [editor, setEditor] = useState<EditorState>();
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const [noticeSuccess, setNoticeSuccess] = useState(false);
  // 提示词按 Arena 类型分开保存：图片、文本、音频之间互不影响，切换时各自保留自己的内容。
  const [prompts, setPrompts] = useState<Partial<Record<OutputType, string>>>({});
  const [referenceImage, setReferenceImage] = useState<{ name: string; dataUrl: string }>();
  const [audioInput, setAudioInput] = useState<{ name: string; dataUrl: string }>();
  // 「模型配置」里测试连接用的音频单独存，避免和主页音频转文本的输入互相污染。
  const [testAudioInput, setTestAudioInput] = useState<{ name: string; dataUrl: string }>();
  const [imageRatio, setImageRatio] = useState("1:1");
  const [videoRatio, setVideoRatio] = useState("16:9");
  const [audioVoice, setAudioVoice] = useState("alloy");
  const [videoSeconds, setVideoSeconds] = useState("5");
  const [runId, setRunId] = useState<string>();
  const [running, setRunning] = useState(false);
  const [results, setResults] = useState<Record<string, ModelResult>>({});
  const [pendingConnectionRemoval, setPendingConnectionRemoval] = useState<string>();
  const [pendingKeyClear, setPendingKeyClear] = useState<string>();
  const keyInputRef = useRef<HTMLInputElement>(null);
  const [pendingGroupClear, setPendingGroupClear] = useState<{
    outputType: OutputType;
    label: string;
    modelCount: number;
    providerKinds: ProviderKind[];
  }>();

  function startWindowDrag(event: ReactMouseEvent<HTMLElement>) {
    if (event.button !== 0 || (event.target as HTMLElement).closest("button")) return;
    void getCurrentWindow().startDragging();
  }

  const prompt = prompts[config.activeArenaType] ?? "";

  function updatePrompt(value: string) {
    setPrompts((current) => ({ ...current, [config.activeArenaType]: value }));
  }

  // 清空当前 Arena：用户输入（提示词、参考图、音频）与模型生成的内容一起清掉，不影响其他 Arena 类型。
  function clearArena() {
    setNotice("");
    setPrompts((current) => ({ ...current, [config.activeArenaType]: "" }));
    setReferenceImage(undefined);
    setAudioInput(undefined);
    setResults({});
  }

  const enabledModels = useMemo(() => config.connections.flatMap((connection) =>
    connection.models
      .filter((model) => model.enabled && model.outputType === config.activeArenaType)
      .map((model) => ({ connection, model })),
  ), [config]);

  const configuredConnections = useMemo(() => config.connections.filter((connection) => connection.models.length > 0), [config]);

  // 主按钮和卡片上的"重新生成"共用同一套前置条件。
  const canRun = !running
    && enabledModels.length > 0
    && Boolean(config.activeArenaType === "audio_to_text" ? audioInput : prompt.trim());

  // 音色按实际用到的模型平台给，选中的值就是发出去的值。
  const voiceOptions = useMemo(() => voiceOptionsFor(enabledModels), [enabledModels]);
  const audioVoiceValue = voiceOptions.some((option) => option.value === audioVoice)
    ? audioVoice
    : voiceOptions[0]?.value ?? "";

  // 视频时长不是每个平台都能指定：硅基流动的视频接口只有 model/prompt/image_size/image，
  // 没有时长字段，所以那边不提供选择器，避免给出一个不起作用的控件。
  const videoSecondsSelectable = useMemo(
    () => enabledModels.every(({ connection }) => connection.providerKind !== "siliconflow"),
    [enabledModels],
  );

  // 集成平台连上之后，添加卡片原地变成"已连接"卡片（不能再加一条）；
  // 自定义模型可以有多条，所以永远保留"添加"卡片。
  const providerCards = useMemo<ProviderCard[]>(() => {
    const cards: ProviderCard[] = [];
    for (const spec of [
      { kind: "aihubmix" as ProviderKind, title: "AIHubMix（梯子必须开全局模式）", sub: "填一次 Key，选择多个模型" },
      { kind: "siliconflow" as ProviderKind, title: "硅基流动", sub: "填一次 Key，选择多个模型" },
    ]) {
      const connections = configuredConnections.filter((connection) => connection.providerKind === spec.kind);
      if (!connections.length) cards.push({ key: `add-${spec.kind}`, ...spec });
      else connections.forEach((connection) => cards.push({ ...spec, key: connection.id, connection }));
    }
    configuredConnections
      .filter((connection) => connection.providerKind === "openai_compatible")
      .forEach((connection) => cards.push({
        key: connection.id,
        kind: "openai_compatible",
        title: connection.displayName,
        sub: "连接独立 OpenAI 兼容接口",
        connection,
      }));
    cards.push({ key: "add-openai_compatible", kind: "openai_compatible", title: "自定义模型", sub: "连接独立 OpenAI 兼容接口" });
    return cards;
  }, [configuredConnections]);

  const pendingClearConnection = useMemo(
    () => configuredConnections.find((connection) => connection.id === (pendingKeyClear ?? pendingConnectionRemoval)),
    [configuredConnections, pendingKeyClear, pendingConnectionRemoval],
  );

  const availableProviders = useMemo(() => Array.from(new Set(
    configuredConnections.map((connection) => connection.providerKind),
  )), [configuredConnections]);

  // 只有一个来源时筛选没有意义，隐藏筛选行并直接忽略筛选值（避免被隐藏的旧勾选状态把模型藏起来）。
  const sourceFilterActive = availableProviders.length > 1;

  const settingsModelGroups = useMemo(() => settingsGroups
    .filter((group) => group.outputType === settingsOutputType)
    .map((group) => ({
      ...group,
      models: config.connections
        .filter((connection) => !sourceFilterActive || sourceFilters.includes(connection.providerKind))
        .flatMap((connection) => connection.models
          .filter((model) => model.outputType === group.outputType)
          .map((model) => ({ connection, model }))),
    })), [config, settingsOutputType, sourceFilters, sourceFilterActive]);

  async function refreshSettings() {
    try {
      setConfig(await invoke<AppConfig>("settings_get"));
    } catch (error) {
      setNotice(messageFrom(error));
      setNoticeSuccess(false);
    } finally {
      setLoading(false);
    }
  }

  function toggleSourceFilter(providerKind: ProviderKind) {
    setSourceFilters((current) => current.includes(providerKind)
      ? current.filter((kind) => kind !== providerKind)
      : [...current, providerKind]);
  }

  // 流式输出的待显示缓冲区（打字机效果），key 是模型配置 ID。
  const pendingDeltasRef = useRef<Record<string, { content: string; reasoning: string; elapsedMs: number }>>({});

  useEffect(() => {
    void refreshSettings();
    let disposed = false;
    const stops: Array<() => void> = [];
    void listen<ModelResult>("text-model-finished", ({ payload }) => {
      if (disposed) return;
      const finished = payload.outputText ? { content: "", reasoning: "", elapsedMs: payload.elapsedMs } : undefined;
      if (finished) {
        // 完整结果已经拿到，丢掉还没显示完的增量，避免重复拼接。
        pendingDeltasRef.current[payload.modelConfigId] = finished;
      }
      setResults((current) => {
        const previous = current[payload.modelConfigId];
        return {
          ...current,
          [payload.modelConfigId]: {
            ...payload,
            // 手动结束或失败时，保留流式过程中已经显示出来的部分内容。
            outputText: payload.outputText ?? previous?.outputText,
            outputReasoning: payload.outputReasoning ?? previous?.outputReasoning,
          },
        };
      });
    }).then((stop) => disposed ? stop() : stops.push(stop));
    void listen<ModelTextDelta>("text-model-delta", ({ payload }) => {
      if (disposed) return;
      const entry = pendingDeltasRef.current[payload.modelConfigId] ?? { content: "", reasoning: "", elapsedMs: 0 };
      entry.content += payload.content;
      entry.reasoning += payload.reasoning;
      entry.elapsedMs = Math.max(entry.elapsedMs, payload.elapsedMs);
      pendingDeltasRef.current[payload.modelConfigId] = entry;
    }).then((stop) => disposed ? stop() : stops.push(stop));
    void listen<RunFinished>("text-run-finished", ({ payload }) => {
      if (disposed) return;
      setRunning(false);
      setRunId(undefined);
      if (payload.status === "ended") {
        setResults((current) => Object.fromEntries(Object.entries(current).map(([id, result]) => [
          id,
          result.status === "running" ? { ...result, status: "ended", elapsedMs: payload.elapsedMs } : result,
        ])));
      }
    }).then((stop) => disposed ? stop() : stops.push(stop));
    void listen<AutoDisabledModel[]>("models-auto-disabled", ({ payload }) => {
      if (disposed || !payload.length) return;
      const detail = payload.slice(0, 3).map((model) => `${model.providerName} 的 ${model.modelId}`).join("、");
      setNotice(`${detail}${payload.length > 3 ? `等 ${payload.length} 个模型` : ""}无法调用，已从主页停用。请在模型配置中检查权限或更换模型。`);
      setNoticeSuccess(false);
      void refreshSettings();
    }).then((stop) => disposed ? stop() : stops.push(stop));
    return () => {
      disposed = true;
      stops.forEach((stop) => stop());
    };
  }, []);

  // 同一时刻只允许播一条音频/视频：开始播新的，旧的自动暂停。
  useEffect(() => {
    const pauseOthers = (event: Event) => {
      const target = event.target;
      if (!(target instanceof HTMLMediaElement)) return;
      document.querySelectorAll<HTMLMediaElement>("audio, video").forEach((element) => {
        if (element !== target && !element.paused) element.pause();
      });
    };
    // media 的 play 事件不冒泡，用捕获阶段在 document 上统一处理。
    document.addEventListener("play", pauseOthers, true);
    return () => document.removeEventListener("play", pauseOthers, true);
  }, []);

  // 打字机效果：每 60ms 把缓冲区里的字拿出来逐渐显示，而不是一次性刷上去。
  useEffect(() => {
    const timer = window.setInterval(() => {
      const pending = pendingDeltasRef.current;
      const ids = Object.keys(pending);
      if (!ids.length) return;
      setResults((current) => {
        let changed = false;
        const draft = { ...current };
        for (const modelConfigId of ids) {
          const entry = pending[modelConfigId];
          const existing = draft[modelConfigId];
          if (!entry.content && !entry.reasoning) continue;
          if (!existing) {
            pending[modelConfigId] = { content: "", reasoning: "", elapsedMs: entry.elapsedMs };
            continue;
          }
          const content = revealChunk(entry, "content");
          const reasoning = revealChunk(entry, "reasoning");
          if (!content && !reasoning && entry.elapsedMs === existing.elapsedMs) continue;
          draft[modelConfigId] = {
            ...existing,
            outputText: `${existing.outputText ?? ""}${content}`,
            outputReasoning: `${existing.outputReasoning ?? ""}${reasoning}`,
            elapsedMs: Math.max(existing.elapsedMs, entry.elapsedMs),
          };
          changed = true;
        }
        return changed ? draft : current;
      });
    }, 60);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    if (!pendingGroupClear) return;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setPendingGroupClear(undefined);
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [pendingGroupClear]);

  useEffect(() => {
    if (!notice) return;
    const timeout = window.setTimeout(() => setNotice(""), 3200);
    return () => window.clearTimeout(timeout);
  }, [notice]);

  function startAdd(providerKind: ProviderKind) {
    setEditor(newEditor(providerKind));
    setNotice("");
  }

  function startEdit(connection: Connection) {
    const outputType = connection.models.some((model) => model.outputType === config.activeArenaType)
      ? config.activeArenaType
      : connection.models[0]?.outputType ?? "text";
    const modelCategory = categoryOfOutput(outputType);
    const models = connection.models.filter((model) => model.outputType === outputType);
    setEditor({
      connectionId: connection.id,
      hasCredential: connection.hasCredential,
      providerKind: connection.providerKind,
      modelCategory,
      outputType,
      catalogFilter: outputType,
      displayName: connection.displayName,
      baseUrl: connection.baseUrl,
      apiKey: "",
      replaceCredential: !connection.hasCredential,
      storedMask: maskOf(connection),
      modelId: connection.providerKind === "openai_compatible" ? models[0]?.modelId ?? connection.models[0]?.modelId ?? "" : "",
      savedModels: connection.models,
      connected: true,
      catalog: models.map((model) => ({ modelId: model.modelId, outputType: model.outputType, supportsReferenceImage: model.supportsReferenceImage })),
      catalogQuery: "",
      parameterLimitEnabled: false,
      maxParameterBillions: "14",
      reasoningFilter: "all",
      selectedModels: models.map((model) => model.modelId),
      imageCapableModels: connection.models.filter((model) => model.supportsReferenceImage).map((model) => model.modelId),
      validationToken: "",
    });
    setNotice("");
  }

  function updateEditor(field: "displayName" | "baseUrl" | "apiKey" | "modelId", value: string) {
    setEditor((current) => current ? { ...current, [field]: value, validationToken: "" } : current);
    setNotice("");
  }

  function setEditorCategory(modelCategory: ModelCategory) {
    const outputType = categoryOutputType(modelCategory);
    setEditor((current) => {
      if (!current) return current;
      const models = current.savedModels.filter((model) => categoryOfModel(model) === modelCategory);
      return {
        ...current,
        modelCategory,
        outputType,
        catalogFilter: outputType,
        modelId: current.providerKind === "openai_compatible" ? models[0]?.modelId ?? current.modelId : current.modelId,
        catalog: models.map((model) => ({ modelId: model.modelId, outputType: model.outputType, supportsReferenceImage: model.supportsReferenceImage })),
        catalogQuery: "",
        selectedModels: models.map((model) => model.modelId),
        validationToken: "",
      };
    });
    setNotice("");
  }

  function setIntegratedCatalogFilter(catalogFilter: CatalogFilter) {
    setEditor((current) => {
      if (!current) return current;
      const models = catalogFilter === "all"
        ? current.savedModels
        : current.savedModels.filter((model) => model.outputType === catalogFilter);
      return {
        ...current,
        modelCategory: catalogFilter === "all" ? current.modelCategory : categoryOfOutput(catalogFilter),
        outputType: catalogFilter === "all" ? current.outputType : catalogFilter,
        catalogFilter,
        catalog: models.map((model) => ({ modelId: model.modelId, outputType: model.outputType, supportsReferenceImage: model.supportsReferenceImage })),
        catalogQuery: "",
        parameterLimitEnabled: catalogFilter === "text" ? current.parameterLimitEnabled : false,
        reasoningFilter: catalogFilter === "text" ? current.reasoningFilter : "all",
        selectedModels: models.map((model) => model.modelId),
        validationToken: "",
      };
    });
    setNotice("");
  }

  async function selectEditorCatalogFilter(catalogFilter: CatalogFilter) {
    setIntegratedCatalogFilter(catalogFilter);
    if (editor?.providerKind !== "openai_compatible" && editor?.connected) {
      await loadModels(catalogFilter);
    }
  }

  async function loadModels(catalogFilter = editor?.catalogFilter) {
    if (!editor) return;
    if (!catalogFilter) return;
    setBusy(true);
    setNotice("");
    try {
      const response = await invoke<{ validationToken: string; models: CatalogModel[] }>("provider_models", {
        input: {
          connectionId: editor.connectionId,
          providerKind: editor.providerKind,
          baseUrl: editor.baseUrl,
          apiKey: editor.replaceCredential ? editor.apiKey.trim() || undefined : undefined,
          outputType: catalogFilter,
        },
      });
      setEditor((current) => current ? {
        ...current,
        connected: true,
        catalog: response.models,
        selectedModels: Array.from(new Set([
          ...current.selectedModels,
          ...current.savedModels
            .filter((model) => (catalogFilter === "all" || model.outputType === catalogFilter) && response.models.some((item) => item.modelId === model.modelId && item.outputType === model.outputType))
            .map((model) => model.modelId),
        ])).filter((id) => response.models.some((model) => model.modelId === id)),
        imageCapableModels: Array.from(new Set([
          ...current.imageCapableModels.filter((id) => !response.models.some((model) => model.modelId === id)),
          ...response.models.filter((model) => model.supportsReferenceImage).map((model) => model.modelId),
        ])),
        validationToken: response.validationToken,
      } : current);
      setNotice(`连接成功，已加载 ${response.models.length} 个${catalogFilter === "all" ? "全部模型" : arenaTypeLabel(catalogFilter)}。`);
      setNoticeSuccess(true);
    } catch (error) {
      setNotice(messageFrom(error));
      setNoticeSuccess(false);
    } finally {
      setBusy(false);
    }
  }

  async function testCustomModel() {
    if (!editor) return;
    setBusy(true);
    setNotice("");
    try {
      const response = await invoke<{ validationToken: string; hasUsage: boolean; elapsedMs: number }>("connection_test", {
        input: {
          connectionId: editor.connectionId,
          providerKind: editor.providerKind,
          baseUrl: editor.baseUrl,
          apiKey: editor.replaceCredential ? editor.apiKey.trim() || undefined : undefined,
          modelId: editor.modelId,
          outputType: editor.providerKind === "openai_compatible" ? editor.outputType : editor.catalogFilter,
          audioInput: editor.outputType === "audio_to_text" ? testAudioInput?.dataUrl : undefined,
        },
      });
      setEditor((current) => current ? { ...current, validationToken: response.validationToken } : current);
      setNotice(`连接成功 · ${(response.elapsedMs / 1000).toFixed(1)} 秒${response.hasUsage ? " · 支持 Token" : ""}`);
      setNoticeSuccess(true);
    } catch (error) {
      setNotice(messageFrom(error));
      setNoticeSuccess(false);
    } finally {
      setBusy(false);
    }
  }

  function toggleSelected(modelId: string) {
    setEditor((current) => {
      if (!current) return current;
      const selected = current.selectedModels.includes(modelId)
        ? current.selectedModels.filter((id) => id !== modelId)
        : [...current.selectedModels, modelId];
      return { ...current, selectedModels: selected };
    });
  }

  async function saveConnection(event: FormEvent) {
    event.preventDefault();
    if (!editor?.validationToken) return;
    const modelIds = editor.providerKind === "openai_compatible" ? [editor.modelId.trim()] : editor.selectedModels;
    if (!modelIds.length || modelIds.some((id) => !id)) {
      setNotice("请至少选择或填写一个模型。");
      setNoticeSuccess(false);
      return;
    }
    setBusy(true);
    try {
      const result = await invoke<ConnectionSaveResult>("connection_save", {
        input: {
          connectionId: editor.connectionId,
          displayName: editor.displayName,
          providerKind: editor.providerKind,
          baseUrl: editor.baseUrl,
          apiKey: editor.replaceCredential ? editor.apiKey.trim() || undefined : undefined,
          outputType: editor.providerKind === "openai_compatible" ? editor.outputType : editor.catalogFilter,
          supportsReferenceImage: modelIds.some((id) => editor.imageCapableModels.includes(id)),
          validationToken: editor.validationToken,
          models: modelIds.map((id) => ({
            modelId: id,
            displayName: id,
            outputType: editor.providerKind === "openai_compatible"
              ? editor.outputType
              : editor.catalog.find((model) => model.modelId === id)?.outputType ?? editor.outputType,
            supportsReferenceImage: editor.imageCapableModels.includes(id),
            enabled: true,
          })),
        },
      });
      setEditor(undefined);
      const skipped = result?.skippedModels ?? [];
      const skippedDetail = skipped.slice(0, 4).map((model) => {
        const reason = model.reason.split(" 平台提示：")[0].replace("，请删除或更换模型。", "");
        return `${model.modelId}（${reason}）`;
      }).join("、");
      setNotice(skipped.length
        ? `${editor.displayName} 有 ${skipped.length} 个模型经实际调用验证失败，已跳过且不会进入主页：${skippedDetail}${skipped.length > 4 ? `等 ${skipped.length} 个` : ""}`
        : "模型配置已保存。");
      setNoticeSuccess(true);
      await refreshSettings();
    } catch (error) {
      setNotice(messageFrom(error));
      setNoticeSuccess(false);
    } finally {
      setBusy(false);
    }
  }

  async function toggleModel(connection: Connection, model: ModelConfig) {
    try {
      await invoke("model_set_enabled", {
        connectionId: connection.id,
        modelConfigId: model.id,
        enabled: !model.enabled,
      });
      await refreshSettings();
    } catch (error) {
      setNotice(messageFrom(error));
      setNoticeSuccess(false);
    }
  }

  async function clearModelGroup() {
    if (!pendingGroupClear) return;
    try {
      await invoke("models_clear", {
        outputType: pendingGroupClear.outputType,
        providerKinds: pendingGroupClear.providerKinds,
      });
      setPendingGroupClear(undefined);
      await refreshSettings();
    } catch (error) {
      setNotice(messageFrom(error));
      setNoticeSuccess(false);
    }
  }

  async function saveSystemPrompt(outputType: OutputType, value: string) {
    try {
      const updated = await invoke<AppConfig>("system_prompt_set", { outputType, systemPrompt: value });
      setConfig(updated);
      setNotice(value.trim() ? `${arenaTypeLabel(outputType)}的系统提示词已保存。` : `${arenaTypeLabel(outputType)}的系统提示词已清空。`);
      setNoticeSuccess(true);
    } catch (error) {
      setNotice(messageFrom(error));
      setNoticeSuccess(false);
      throw error;
    }
  }

  async function setArenaType(outputType: OutputType) {
    setNotice("");
    try {
      await invoke("arena_type_set", { outputType });
      await refreshSettings();
    } catch (error) {
      setNotice(messageFrom(error));
      setNoticeSuccess(false);
    }
  }

  async function removeConnection(connection: Connection) {
    try {
      await invoke("connection_remove", { connectionId: connection.id });
      if (editor?.connectionId === connection.id) setEditor(undefined);
      setPendingConnectionRemoval(undefined);
      setPendingKeyClear(undefined);
      await refreshSettings();
    } catch (error) {
      setNotice(messageFrom(error));
      setNoticeSuccess(false);
    }
  }

  async function removeModel(connection: Connection, model: ModelConfig) {
    try {
      await invoke("model_remove", {
        connectionId: connection.id,
        modelConfigId: model.id,
      });
      await refreshSettings();
    } catch (error) {
      setNotice(messageFrom(error));
      setNoticeSuccess(false);
    }
  }

  function chooseReferenceImage(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) return;
    if (file.size > 10 * 1024 * 1024) {
      setNotice("图片不能超过 10 MB。");
      setNoticeSuccess(false);
      return;
    }
    const reader = new FileReader();
    reader.onload = () => {
      if (typeof reader.result === "string") {
        setReferenceImage({ name: file.name, dataUrl: reader.result });
        setNotice("");
      }
    };
    reader.onerror = () => {
      setNotice("无法读取这张图片。");
      setNoticeSuccess(false);
    };
    reader.readAsDataURL(file);
  }

  /** 主页与「模型配置」各自传入自己的 setter，两份音频互不影响。 */
  function chooseAudioInput(
    event: ChangeEvent<HTMLInputElement>,
    apply: (value: { name: string; dataUrl: string }) => void,
  ) {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) return;
    if (!supportedAudioExtension.test(file.name)) {
      setNotice("仅支持 mp3、mp4、mpeg、mpga、m4a、wav 或 webm 音频。");
      setNoticeSuccess(false);
      return;
    }
    if (!file.size) {
      setNotice("音频文件为空，请重新选择。");
      setNoticeSuccess(false);
      return;
    }
    if (file.size > 25 * 1024 * 1024) {
      setNotice("音频文件不能超过 25 MB。");
      setNoticeSuccess(false);
      return;
    }
    const reader = new FileReader();
    reader.onload = () => {
      if (typeof reader.result === "string") {
        const dataUrl = reader.result.replace(/^data:[^;,]*/, `data:${audioMimeType(file.name)}`);
        apply({ name: file.name, dataUrl });
        setNotice("");
      }
    };
    reader.onerror = () => {
      setNotice("无法读取这个音频文件。");
      setNoticeSuccess(false);
    };
    reader.readAsDataURL(file);
  }

  /** 传 modelIds 时只重跑这几个模型（卡片上的"重新生成"），不传则跑当前类型全部启用模型。 */
  async function run(modelIds?: string[]) {
    const targets = modelIds
      ? enabledModels.filter(({ model }) => modelIds.includes(model.id))
      : enabledModels;
    if (running || !targets.length) return;
    setNotice("");
    setRunning(true);
    setResults((current) => {
      const next: Record<string, ModelResult> = modelIds ? { ...current } : {};
      for (const { model } of targets) {
        next[model.id] = { runId: "", modelConfigId: model.id, status: "running", elapsedMs: 0 };
      }
      return next;
    });
    for (const { model } of targets) {
      pendingDeltasRef.current[model.id] = { content: "", reasoning: "", elapsedMs: 0 };
    }
    try {
      const started = await invoke<{ runId: string }>("text_run_start", {
        input: {
          prompt,
          referenceImage: referenceImage?.dataUrl,
          audioInput: audioInput?.dataUrl,
          imageRatio: config.activeArenaType === "video" ? videoRatio : imageRatio,
          audioVoice: audioVoiceValue,
          videoSeconds,
          modelConfigIds: modelIds,
        },
      });
      setRunId(started.runId);
    } catch (error) {
      setRunning(false);
      setNotice(messageFrom(error));
      setNoticeSuccess(false);
      setResults((current) => modelIds
        ? Object.fromEntries(Object.entries(current).filter(([id]) => !modelIds.includes(id)))
        : {});
    }
  }

  function regenerateModel(model: ModelConfig) {
    void run([model.id]);
  }

  async function cancel() {
    if (!runId) return;
    try {
      await invoke("text_run_cancel", { runId });
    } catch (error) {
      setNotice(messageFrom(error));
      setNoticeSuccess(false);
    }
  }

  async function downloadAudio(dataUrl: string, modelName: string) {
    try {
      const path = await invoke<string>("audio_save", { dataUrl, modelName });
      setNotice(`音频已保存到 ${path}`);
      setNoticeSuccess(true);
    } catch (error) {
      setNotice(messageFrom(error));
      setNoticeSuccess(false);
    }
  }

  // WKWebView 里 navigator.clipboard 有时不可用，退回选中临时文本框的方案。
  async function copyResult(result: ModelResult, model: ModelConfig) {
    const text = result.outputText ?? "";
    if (!text.trim()) return;
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      const area = document.createElement("textarea");
      area.value = text;
      area.setAttribute("readonly", "");
      area.style.position = "fixed";
      area.style.opacity = "0";
      document.body.appendChild(area);
      area.select();
      try {
        document.execCommand("copy");
      } finally {
        document.body.removeChild(area);
      }
    }
    setNotice(`已复制 ${model.displayName} 的回答。`);
    setNoticeSuccess(true);
  }

  return (
    <main className="app-shell">
      <header className="topbar" onMouseDown={startWindowDrag}>
        <nav className="page-tabs" aria-label="主页面">
          <button
            className={activePage === "arena" ? "is-active" : ""}
            aria-current={activePage === "arena" ? "page" : undefined}
            onClick={() => { setNotice(""); setActivePage("arena"); }}
          >模型 Battle</button>
          <button
            className={activePage === "settings" ? "is-active" : ""}
            aria-current={activePage === "settings" ? "page" : undefined}
            onClick={() => {
              setSettingsOutputType(config.activeArenaType);
              setActivePage("settings");
            }}
          >模型配置</button>
        </nav>
      </header>

      {activePage === "arena" ? (<div className="arena-page">
      <div className="arena-switch-row">
        <nav className="type-filter arena-type-switch" aria-label="模型类型">
          {(["text", "image", "audio", "audio_to_text", "video"] as OutputType[]).map((outputType) => <button
            type="button"
            key={outputType}
            className={config.activeArenaType === outputType ? "is-active" : ""}
            aria-pressed={config.activeArenaType === outputType}
            onClick={() => void setArenaType(outputType)}
            disabled={running}
          >{arenaTypeLabel(outputType)}</button>)}
        </nav>
        <SystemPromptControl
          outputType={config.activeArenaType}
          systemPrompt={config.systemPrompts?.[config.activeArenaType]}
          onSave={(value) => saveSystemPrompt(config.activeArenaType, value)}
        />
      </div>
      <section className="prompt-card" aria-label="提示词输入">
        {config.activeArenaType === "audio_to_text" ? (
          <div className="audio-input-copy">
            {audioInput ? (
              <div className="audio-file-chip" title={audioInput.name}>
                <span>{audioInput.name}</span>
                <button
                  type="button"
                  aria-label={`移除音频 ${audioInput.name}`}
                  title="移除音频"
                  onClick={() => setAudioInput(undefined)}
                  disabled={running}
                >×</button>
              </div>
            ) : (
              <strong className="is-placeholder">添加一段音频，让所有语音识别模型同时转写</strong>
            )}
            <span>支持 mp3、mp4、mpeg、mpga、m4a、wav、webm，最大 25 MB</span>
          </div>
        ) : (
          <textarea
            key={config.activeArenaType}
            value={prompt}
            onChange={(event) => {
              updatePrompt(event.target.value);
              event.currentTarget.style.height = "auto";
              event.currentTarget.style.height = `${event.currentTarget.scrollHeight}px`;
            }}
            onKeyDown={(event) => {
              if (event.key !== "Enter" || event.shiftKey || event.nativeEvent.isComposing) return;
              event.preventDefault();
              if (prompt.trim() && enabledModels.length && !running) void run();
            }}
            placeholder="输入同一个问题或任务，让所有模型同时回答……"
            disabled={running}
            autoFocus
          />
        )}
        <div className="prompt-tools">
          {categoryOfOutput(config.activeArenaType) !== "audio" && <label className={`upload-button ${running ? "is-disabled" : ""}`}>
            ＋ 添加图片
            <input type="file" accept="image/*" onChange={chooseReferenceImage} disabled={running} />
          </label>}
          {referenceImage && categoryOfOutput(config.activeArenaType) !== "audio" && (
            <div className="reference-chip" title={referenceImage.name}>
              <img src={referenceImage.dataUrl} alt="参考图片" />
              <span>{referenceImage.name}</span>
              <button type="button" aria-label="移除图片" onClick={() => setReferenceImage(undefined)} disabled={running}>×</button>
            </div>
          )}
          {(config.activeArenaType === "image" || config.activeArenaType === "video") && (
            <SelectMenu
              label={config.activeArenaType === "video" ? "视频生成比例" : "生图比例"}
              value={config.activeArenaType === "video" ? videoRatio : imageRatio}
              options={['1:1', '3:4', '4:3', '9:16', '16:9'].map((ratio) => ({ value: ratio, label: ratio }))}
              onChange={(value) => config.activeArenaType === "video" ? setVideoRatio(value) : setImageRatio(value)}
              disabled={running}
            />
          )}
          {config.activeArenaType === "audio" && !!voiceOptions.length && (
            <SelectMenu
              label="生成音色"
              value={audioVoiceValue}
              options={voiceOptions}
              onChange={setAudioVoice}
              disabled={running}
            />
          )}
          {config.activeArenaType === "audio_to_text" && <label className={`upload-button ${running ? "is-disabled" : ""}`}>
            ＋ {audioInput ? "更换音频" : "添加音频"}
            <input type="file" accept={audioFileAccept} onChange={(event) => void chooseAudioInput(event, setAudioInput)} disabled={running} />
          </label>}
          {config.activeArenaType === "video" && (videoSecondsSelectable ? (
            <SelectMenu
              label="视频时长"
              value={videoSeconds}
              options={['4', '5', '8', '10'].map((seconds) => ({ value: seconds, label: `${seconds} 秒` }))}
              onChange={setVideoSeconds}
              disabled={running}
            />
          ) : (
            <span className="prompt-note" title="该平台的视频接口不支持指定时长，由平台按模型默认时长生成。">
              时长由平台固定
            </span>
          ))}
        </div>
        <div className="prompt-actions">
          <button
            type="button"
            className="secondary-button"
            title="清空当前输入和生成内容"
            onClick={clearArena}
            disabled={running}
          >清空</button>
          {running ? (
            <button className="secondary-button" onClick={cancel} disabled={!runId}>结束本次运行</button>
          ) : (
            <button className="primary-button" onClick={() => void run()} disabled={!canRun}>
              {{ text: "运行全部模型", image: "生成全部图片", audio: "生成全部音频", audio_to_text: "转写全部音频", video: "生成全部视频" }[config.activeArenaType]}
            </button>
          )}
        </div>
      </section>

      <section className={`result-grid ${config.activeArenaType === "audio" ? "is-audio-output" : ""}`} aria-label="模型输出">
        {!loading && enabledModels.map(({ connection, model }) => (
          <ResultCard
            key={model.id}
            model={model}
            connectionName={connection.displayName}
            result={results[model.id]}
            running={running}
            showTokenMetrics={config.activeArenaType !== "audio" && config.activeArenaType !== "audio_to_text"}
            canRegenerate={canRun}
            onRegenerate={regenerateModel}
            onCopy={copyResult}
            onDownloadAudio={downloadAudio}
          />
        ))}
        {!loading && !enabledModels.length && (
          <button className="model-card add-card" onClick={() => {
            setSettingsOutputType(config.activeArenaType);
            setActivePage("settings");
          }}>
            <span className="add-icon">＋</span>
            <strong>添加模型连接</strong>
            <span>连接集成平台，或添加一个独立模型</span>
          </button>
        )}
      </section>
      </div>) : (
          <section className="settings-page" aria-label="模型配置">
            <div className={`settings-content ${editor ? "is-editing" : ""}`}>
              {!editor && <div className="settings-overview">
                <div className="model-pool-filters">
                  <div className="type-filter" role="group" aria-label="模型池类型">
                    {settingsGroups.map(({ outputType, label }) => (
                      <button
                        type="button"
                        key={outputType}
                        className={settingsOutputType === outputType ? "is-active" : ""}
                        aria-pressed={settingsOutputType === outputType}
                        onClick={() => setSettingsOutputType(outputType)}
                      >{label}</button>
                    ))}
                  </div>
                  {sourceFilterActive && <div className="source-filters" role="group" aria-label="来源筛选">
                    {availableProviders.map((providerKind) => <label key={providerKind}>
                      <input
                        type="checkbox"
                        checked={sourceFilters.includes(providerKind)}
                        onChange={() => toggleSourceFilter(providerKind)}
                        aria-label={`筛选来源：${providerLabel(providerKind)}`}
                      />
                      <span>{providerLabel(providerKind)}</span>
                    </label>)}
                  </div>}
                </div>

                <section className="model-pool" aria-label={`${arenaTypeLabel(settingsOutputType)}模型池`}>
                  {settingsModelGroups.map((group) => <section className="model-category" key={group.outputType}>
                      <header className="model-category-header">
                        <div><strong>{group.label}</strong><span>{group.models.length} 个</span></div>
                        {!!group.models.length && <div className="model-category-actions">
                          <button
                            type="button"
                            className="ghost-action danger-action"
                            aria-label={`清空：${group.label}`}
                            title="删除当前来源筛选下的全部模型"
                            onClick={() => setPendingGroupClear({
                              outputType: group.outputType,
                              label: group.label,
                              modelCount: group.models.length,
                              providerKinds: [...sourceFilters],
                            })}
                        ><TrashIcon /><span>清空</span></button>
                        </div>}
                      </header>
                      {group.models.map(({ connection, model }) => <div className="model-setting-row" key={model.id}>
                        <div className="model-name-cell">
                          <strong title={model.displayName}>{model.displayName}</strong>
                          <em className={`source-tag source-tag--${connection.providerKind}`}>{connection.displayName}</em>
                          {capabilityLabels(model.outputType, model.supportsReferenceImage).map((label) => <em key={label}>{label}</em>)}
                        </div>
                        <button
                          type="button"
                          className={`switch ${model.enabled ? "is-on" : ""}`}
                          role="switch"
                          aria-checked={model.enabled}
                          aria-label={`${model.enabled ? "关闭" : "启用"}${model.displayName}`}
                          onClick={() => toggleModel(connection, model)}
                        ><span /></button>
                        <button
                          type="button"
                          className="ghost-action danger-action"
                          aria-label={`删除${model.displayName}`}
                          title="删除模型"
                          onClick={() => removeModel(connection, model)}
                        ><TrashIcon /><span>删除</span></button>
                      </div>)}
                      {!group.models.length && <p className="empty-group">当前筛选下没有{group.label}。</p>}
                    </section>)}
                </section>

              </div>}

              {editor ? (
                <form className="connection-editor" onSubmit={saveConnection}>
                  <header className="editor-header">
                    <div>
                      <h3>{editor.connectionId ? "编辑连接" : `连接 ${providerLabel(editor.providerKind)}`}</h3>
                      <p>{editor.providerKind === "openai_compatible"
                        ? "适用于只有单个 OpenAI 兼容模型地址的厂商。"
                        : "先连接平台，再按输出类型筛选并选择模型。"}</p>
                    </div>
                    <button
                      type="button"
                      className="editor-close"
                      aria-label="关闭连接编辑"
                      title="关闭"
                      onClick={() => setEditor(undefined)}
                    ><CloseIcon /></button>
                  </header>

                  <div className="form-grid">
                    <label>
                      <span>连接名称 <b>*</b></span>
                      <input value={editor.displayName} onChange={(event) => updateEditor("displayName", event.target.value)} required />
                    </label>
                    {editor.providerKind === "openai_compatible" && (
                      <label>
                        <span>API Base URL <b>*</b></span>
                        <input value={editor.baseUrl} onChange={(event) => updateEditor("baseUrl", event.target.value)} placeholder="https://api.example.com/v1" inputMode="url" required />
                      </label>
                    )}
                    <div className="credential-field">
                      <span className="credential-label">
                        <label htmlFor="connection-api-key">API Key {!editor.connectionId && <b>*</b>}</label>
                        {!editor.replaceCredential && <button
                          type="button"
                          className="credential-replace"
                          onClick={() => {
                            setEditor((current) => current ? { ...current, replaceCredential: true, apiKey: "", validationToken: "" } : current);
                            setNotice("");
                            keyInputRef.current?.focus();
                          }}
                        >更换</button>}
                      </span>
                      <input
                        id="connection-api-key"
                        ref={keyInputRef}
                        value={editor.replaceCredential ? editor.apiKey : editor.storedMask}
                        readOnly={!editor.replaceCredential}
                        onChange={(event) => updateEditor("apiKey", event.target.value)}
                        onBlur={() => {
                          if (!editor.hasCredential || !editor.replaceCredential || editor.apiKey.trim()) return;
                          setEditor((current) => current ? { ...current, replaceCredential: false, apiKey: "" } : current);
                        }}
                        type="password"
                        autoComplete="off"
                        placeholder={editor.hasCredential ? "已保存在本软件" : "仅保存在本软件中"}
                        required={!editor.connectionId}
                      />
                    </div>
                    {editor.providerKind === "openai_compatible" && (
                      <label>
                        <span>模型 ID <b>*</b></span>
                        <input value={editor.modelId} onChange={(event) => updateEditor("modelId", event.target.value)} placeholder="厂商提供的模型 ID" required />
                      </label>
                    )}
                  </div>

                  {editor.providerKind === "openai_compatible" && (
                    <section className="model-type-field">
                      <strong>模型分类</strong>
                      <div className="type-filter" role="group" aria-label="模型输出类型">
                        {(["text", "image", "audio", "video"] as ModelCategory[]).map((category) => (
                          <button
                            type="button"
                            key={category}
                            className={editor.modelCategory === category ? "is-active" : ""}
                            onClick={() => setEditorCategory(category)}
                          >{categoryLabel(category)}</button>
                        ))}
                      </div>
                      {editor.modelCategory === "audio" && <>
                        <strong>任务方向</strong>
                        <div className="type-filter" role="group" aria-label="音频任务方向">
                          <button
                            type="button"
                            className={editor.outputType === "audio" ? "is-active" : ""}
                            onClick={() => setEditor((current) => current ? { ...current, outputType: "audio", validationToken: "" } : current)}
                          >文本转音频</button>
                          <button
                            type="button"
                            className={editor.outputType === "audio_to_text" ? "is-active" : ""}
                            onClick={() => setEditor((current) => current ? { ...current, outputType: "audio_to_text", validationToken: "" } : current)}
                          >音频转文本</button>
                        </div>
                        {editor.outputType === "audio_to_text" && <label className="upload-button editor-audio-upload">
                          ＋ {testAudioInput ? `测试音频：${testAudioInput.name}` : "添加测试音频"}
                          <input type="file" accept={audioFileAccept} onChange={(event) => void chooseAudioInput(event, setTestAudioInput)} />
                        </label>}
                      </>}
                    </section>
                  )}

                  {editor.providerKind !== "openai_compatible" && (
                    <>
                      <div className="provider-endpoint">
                        <span>接口地址</span><code>{editor.baseUrl}</code>
                      </div>
                      {editor.providerKind === "aihubmix" && (
                        <p className="provider-hint">连接 AIHubMix 时，梯子必须开启“全局模式”；使用规则模式可能导致验证或加载模型超时。</p>
                      )}
                      {editor.connected ? (
                        <>
                          <div className="type-filter" role="group" aria-label="模型目录筛选">
                            {([{ outputType: "all" as const, label: "全部" }, ...settingsGroups]).map(({ outputType, label }) => (
                              <button
                                type="button"
                                key={outputType}
                                aria-label={outputType === "all" ? "全部模型" : undefined}
                                className={editor.catalogFilter === outputType ? "is-active" : ""}
                                onClick={() => void selectEditorCatalogFilter(outputType)}
                                disabled={busy}
                              >{label}</button>
                            ))}
                          </div>
                          {busy && <p className="provider-hint" role="status">正在加载当前分类模型…</p>}
                        </>
                      ) : (
                        <p className="provider-hint">验证连接后，才会显示模型类型筛选和模型目录。</p>
                      )}
                      {!!editor.catalog.length && (
                        <div className="catalog-block">
                          <div className="catalog-heading">
                            <strong>选择{editor.catalogFilter === "all" ? "全部模型" : arenaTypeLabel(editor.outputType)}</strong>
                            <span>已选 {editor.selectedModels.length} 个</span>
                          </div>
                          <div className="catalog-controls">
                            <input
                              className="catalog-search"
                              value={editor.catalogQuery}
                              onChange={(event) => setEditor((current) => current ? { ...current, catalogQuery: event.target.value } : current)}
                              placeholder="搜索模型 ID"
                              aria-label="搜索模型"
                            />
                            {editor.catalogFilter === "text" && <div className="catalog-size-filter" role="group" aria-label="模型参数量筛选">
                              <input
                                type="checkbox"
                                aria-label="启用参数量筛选"
                                checked={editor.parameterLimitEnabled}
                                onChange={(event) => setEditor((current) => current ? { ...current, parameterLimitEnabled: event.target.checked } : current)}
                              />
                              <input
                                className="catalog-size-input"
                                type="number"
                                min="0.1"
                                step="0.1"
                                inputMode="decimal"
                                aria-label="参数量上限"
                                value={editor.maxParameterBillions}
                                onChange={(event) => setEditor((current) => current ? { ...current, maxParameterBillions: event.target.value } : current)}
                                onBlur={() => setEditor((current) => current && (!Number.isFinite(Number(current.maxParameterBillions)) || Number(current.maxParameterBillions) <= 0)
                                  ? { ...current, maxParameterBillions: "14" }
                                  : current)}
                              />
                              <span>B 以下</span>
                            </div>}
                            {editor.catalogFilter === "text" && <div className="catalog-reasoning-filter" role="group" aria-label="推理能力筛选">
                              <span>推理能力</span>
                              {([
                                ["all", "全部"],
                                ["reasoning", "推理模型"],
                                ["non_reasoning", "非推理模型"],
                              ] as Array<[ReasoningFilter, string]>).map(([value, label]) => <button
                                type="button"
                                key={value}
                                aria-pressed={editor.reasoningFilter === value}
                                className={editor.reasoningFilter === value ? "is-active" : ""}
                                onClick={() => setEditor((current) => current ? { ...current, reasoningFilter: value } : current)}
                              >{label}</button>)}
                            </div>}
                          </div>
                          <div className="catalog-list">
                            {(editor.catalogFilter === "all"
                              ? settingsGroups.map(({ outputType }) => outputType)
                              : [editor.outputType]
                            ).map((outputType) => {
                              const models = editor.catalog.filter((model) =>
                                (model.outputType ?? editor.outputType) === outputType
                                && (!editor.parameterLimitEnabled
                                  || editor.modelCategory !== "text"
                                  || isModelAtMostBillions(model.modelId, Number(editor.maxParameterBillions)))
                                && (editor.reasoningFilter === "all"
                                  || (editor.reasoningFilter === "reasoning") === isReasoningModel(model.modelId))
                                && model.modelId.toLowerCase().includes(editor.catalogQuery.trim().toLowerCase()));
                              if (!models.length) return editor.modelCategory === "text"
                                && (editor.parameterLimitEnabled || editor.reasoningFilter !== "all")
                                ? <p className="catalog-empty" key={outputType}>没有符合当前筛选条件的模型。</p>
                                : null;
                              return <section className="catalog-section" key={outputType}>
                                {editor.catalogFilter === "all" && <h4>{arenaTypeLabel(outputType)} · {models.length} 个</h4>}
                                <div className="catalog-section-grid">
                                  {models.map((model) => (
                                    <div key={`${outputType}:${model.modelId}`} className="catalog-item">
                                      <label>
                                        <input
                                          type="checkbox"
                                          checked={editor.selectedModels.includes(model.modelId)}
                                          onChange={() => toggleSelected(model.modelId)}
                                        />
                                        <span title={model.modelId}>{model.modelId}</span>
                                      </label>
                                      <span className="catalog-capability" title="根据模型名称和平台能力信息生成">
                                        {modelCapabilityLabels({ ...model, outputType }).map((label) => <em key={label}>{label}</em>)}
                                      </span>
                                    </div>
                                  ))}
                                </div>
                              </section>;
                            })}
                          </div>
                        </div>
                      )}
                    </>
                  )}

                  <div className="form-actions">
                    {(editor.providerKind === "openai_compatible" || !editor.connected) && <button
                      type="button"
                      className="secondary-button"
                      onClick={editor.providerKind === "openai_compatible" ? testCustomModel : () => void loadModels()}
                      disabled={busy || (editor.outputType === "audio_to_text" && !testAudioInput)}
                    >{busy ? "验证中…" : editor.providerKind === "openai_compatible"
                      ? editor.outputType === "image" ? "测试连接（生成 1 张图）"
                        : editor.outputType === "audio" ? "测试连接（生成短音频）"
                          : editor.outputType === "audio_to_text" ? "测试连接（转写音频）"
                          : editor.outputType === "video" ? "验证并保存视频配置"
                            : "测试连接"
                      : "验证连接并加载模型"}</button>}
                    <button
                      type="button"
                      className="secondary-button"
                      onClick={() => { setEditor(undefined); setNotice(""); }}
                    >取消</button>
                    {editor.validationToken && <button
                      type="submit"
                      className="primary-button"
                      disabled={busy || (editor.providerKind !== "openai_compatible" && !editor.selectedModels.length)}
                    >保存模型配置</button>}
                  </div>
                </form>
              ) : (
                <section className="add-section">
                  <header className="section-card-header"><h3>添加连接</h3></header>
                  <div className="provider-options">
                    {providerCards.map((card) => <article
                      className={`provider-card ${card.connection ? "is-connected" : "is-add"}`}
                      key={card.key}
                      aria-label={card.connection ? `${card.connection.displayName} ${card.connection.hasCredential ? "已连接" : "缺少 API Key"}` : undefined}
                    >
                      <span className="provider-card-title">
                        <strong>{card.title}</strong>
                        {card.connection && <span className={`provider-badge ${card.connection.hasCredential ? "" : "is-missing"}`}>
                          {card.connection.hasCredential ? "已连接" : "缺少 API Key"}
                        </span>}
                      </span>
                      <span className="provider-card-meta">{card.connection
                        ? `${providerLabel(card.connection.providerKind)} · ${connectionTypeLabel(card.connection.models)} · ${card.connection.models.length} 个模型`
                        : card.sub}</span>
                      <div className="provider-card-actions">
                        {card.connection ? <>
                          <button
                            type="button"
                            className="ghost-action"
                            aria-label={`编辑${card.connection.displayName}`}
                            title="编辑连接"
                            onClick={() => startEdit(card.connection!)}
                          ><EditIcon /><span>编辑</span></button>
                          {card.connection.hasCredential ? <button
                            type="button"
                            className="ghost-action danger-action"
                            aria-label={`清除 API Key：${card.connection.displayName}`}
                            title="清除后，这个连接与它添加的模型会一起移除"
                            onClick={() => setPendingKeyClear(card.connection!.id)}
                          ><TrashIcon /><span>清除 API Key</span></button> : <button
                            type="button"
                            className="ghost-action danger-action"
                            aria-label={`删除${card.connection.displayName}`}
                            title="删除这个连接"
                            onClick={() => setPendingConnectionRemoval(card.connection!.id)}
                          ><TrashIcon /><span>删除连接</span></button>}
                        </> : <button
                          type="button"
                          className="ghost-action"
                          aria-label={`添加${card.title}`}
                          title="添加连接"
                          onClick={() => startAdd(card.kind)}
                        ><PlusIcon /><span>添加</span></button>}
                      </div>
                    </article>)}
                  </div>
                </section>
              )}
            </div>
          </section>
      )}
      {notice && <div className={`global-toast ${noticeSuccess ? "is-success" : "is-error"}`} role="alert" aria-live="assertive"><ToastIcon success={noticeSuccess} /><span>{notice}</span></div>}
      {pendingClearConnection && <div
        className="confirm-overlay"
        role="presentation"
        onMouseDown={(event) => {
          if (event.target === event.currentTarget) { setPendingKeyClear(undefined); setPendingConnectionRemoval(undefined); }
        }}
      >
        <section className="confirm-dialog" role="dialog" aria-modal="true" aria-labelledby="clear-key-dialog-title">
          <h2 id="clear-key-dialog-title">{pendingClearConnection.hasCredential ? "确认清除 API Key？" : "确认删除连接？"}</h2>
          <p>{pendingClearConnection.hasCredential
            ? `将删除连接「${pendingClearConnection.displayName}」、它已保存的 API Key，以及通过它添加的 ${pendingClearConnection.models.length} 个模型配置。此操作无法撤销。`
            : `将删除连接「${pendingClearConnection.displayName}」及其 ${pendingClearConnection.models.length} 个模型配置。此操作无法撤销。`}</p>
          <div className="confirm-actions">
            <button type="button" autoFocus onClick={() => { setPendingKeyClear(undefined); setPendingConnectionRemoval(undefined); }}>取消</button>
            <button type="button" className="confirm-danger" onClick={() => void removeConnection(pendingClearConnection)}>{pendingClearConnection.hasCredential ? "确认清除" : "确认删除"}</button>
          </div>
        </section>
      </div>}

      {pendingGroupClear && <div
        className="confirm-overlay"
        role="presentation"
        onMouseDown={(event) => {
          if (event.target === event.currentTarget) setPendingGroupClear(undefined);
        }}
      >
        <section className="confirm-dialog" role="dialog" aria-modal="true" aria-labelledby="clear-dialog-title">
          <h2 id="clear-dialog-title">确认清空模型？</h2>
          <p>将删除 {pendingGroupClear.providerKinds.map(providerLabel).join("、")} 中的 {pendingGroupClear.modelCount} 个{pendingGroupClear.label}配置。此操作无法撤销。</p>
          <div className="confirm-actions">
            <button type="button" autoFocus onClick={() => setPendingGroupClear(undefined)}>取消</button>
            <button type="button" className="confirm-danger" onClick={() => void clearModelGroup()}>确认清空</button>
          </div>
        </section>
      </div>}
    </main>
  );
}
