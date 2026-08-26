import { ChangeEvent, FormEvent, MouseEvent as ReactMouseEvent, useEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
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
  models: ModelConfig[];
};

type ConnectionSaveResult = {
  connection: Connection;
  skippedModels: Array<{ modelId: string; reason: string }>;
};

type AppConfig = {
  schemaVersion: number;
  activeArenaType: OutputType;
  connections: Connection[];
};

type Usage = {
  inputTokens?: number;
  outputTokens?: number;
  totalTokens?: number;
};

type ModelResult = {
  runId: string;
  modelConfigId: string;
  status: "running" | "completed" | "failed" | "ended";
  outputText?: string;
  outputImage?: string;
  outputAudio?: string;
  outputVideo?: string;
  elapsedMs: number;
  usage?: Usage;
  error?: AppError;
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
  displayName: string;
  baseUrl: string;
  apiKey: string;
  modelId: string;
  savedModels: ModelConfig[];
  connected: boolean;
  catalog: CatalogModel[];
  catalogQuery: string;
  selectedModels: string[];
  imageCapableModels: string[];
  validationToken: string;
};

const emptyConfig: AppConfig = { schemaVersion: 1, activeArenaType: "text", connections: [] };
const providerDefaults: Record<ProviderKind, { name: string; baseUrl: string }> = {
  aihubmix: { name: "AIHubMix", baseUrl: "https://aihubmix.com/v1" },
  siliconflow: { name: "硅基流动", baseUrl: "https://api.siliconflow.cn/v1" },
  openai_compatible: { name: "自定义单模型", baseUrl: "" },
};

const settingsGroups: Array<{ label: string; outputType: OutputType }> = [
  { label: "文本生成模型", outputType: "text" },
  { label: "图片生成模型", outputType: "image" },
  { label: "文本转音频", outputType: "audio" },
  { label: "音频转文本", outputType: "audio_to_text" },
  { label: "视频生成模型", outputType: "video" },
];

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

function newEditor(providerKind: ProviderKind): EditorState {
  const defaults = providerDefaults[providerKind];
  return {
    hasCredential: false,
    providerKind,
    modelCategory: "text",
    outputType: "text",
    displayName: defaults.name,
    baseUrl: defaults.baseUrl,
    apiKey: "",
    modelId: "",
    savedModels: [],
    connected: false,
    catalog: [],
    catalogQuery: "",
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

export default function App() {
  const [config, setConfig] = useState<AppConfig>(emptyConfig);
  const [loading, setLoading] = useState(true);
  const [activePage, setActivePage] = useState<"arena" | "settings">("arena");
  const [settingsCategory, setSettingsCategory] = useState<ModelCategory>("text");
  const [sourceFilters, setSourceFilters] = useState<ProviderKind[]>(["aihubmix", "siliconflow", "openai_compatible"]);
  const [editor, setEditor] = useState<EditorState>();
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const [noticeSuccess, setNoticeSuccess] = useState(false);
  const [prompt, setPrompt] = useState("");
  const [referenceImage, setReferenceImage] = useState<{ name: string; dataUrl: string }>();
  const [audioInput, setAudioInput] = useState<{ name: string; dataUrl: string }>();
  const [imageRatio, setImageRatio] = useState("1:1");
  const [videoRatio, setVideoRatio] = useState("16:9");
  const [audioVoice, setAudioVoice] = useState("alloy");
  const [videoSeconds, setVideoSeconds] = useState("5");
  const [runId, setRunId] = useState<string>();
  const [running, setRunning] = useState(false);
  const [results, setResults] = useState<Record<string, ModelResult>>({});
  const [pendingConnectionRemoval, setPendingConnectionRemoval] = useState<string>();
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

  const enabledModels = useMemo(() => config.connections.flatMap((connection) =>
    connection.models
      .filter((model) => model.enabled && model.outputType === config.activeArenaType)
      .map((model) => ({ connection, model })),
  ), [config]);

  const configuredConnections = useMemo(() => config.connections.filter((connection) => connection.models.length > 0), [config]);

  const availableProviders = useMemo(() => Array.from(new Set(
    configuredConnections.map((connection) => connection.providerKind),
  )), [configuredConnections]);

  const settingsModelGroups = useMemo(() => settingsGroups
    .filter((group) => categoryOfOutput(group.outputType) === settingsCategory)
    .map((group) => ({
      ...group,
      models: config.connections
        .filter((connection) => sourceFilters.includes(connection.providerKind))
        .flatMap((connection) => connection.models
          .filter((model) => model.outputType === group.outputType)
          .map((model) => ({ connection, model }))),
    })), [config, settingsCategory, sourceFilters]);

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

  useEffect(() => {
    void refreshSettings();
    let disposed = false;
    const stops: Array<() => void> = [];
    void listen<ModelResult>("text-model-finished", ({ payload }) => {
      if (!disposed) setResults((current) => ({ ...current, [payload.modelConfigId]: payload }));
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
    const models = connection.models.filter((model) => categoryOfModel(model) === modelCategory);
    setEditor({
      connectionId: connection.id,
      hasCredential: connection.hasCredential,
      providerKind: connection.providerKind,
      modelCategory,
      outputType,
      displayName: connection.displayName,
      baseUrl: connection.baseUrl,
      apiKey: "",
      modelId: connection.providerKind === "openai_compatible" ? models[0]?.modelId ?? connection.models[0]?.modelId ?? "" : "",
      savedModels: connection.models,
      connected: true,
      catalog: models.map((model) => ({ modelId: model.modelId, outputType: model.outputType, supportsReferenceImage: model.supportsReferenceImage })),
      catalogQuery: "",
      selectedModels: models.map((model) => model.modelId),
      imageCapableModels: connection.models.filter((model) => model.supportsReferenceImage).map((model) => model.modelId),
      validationToken: "",
    });
    setNotice("");
    void invoke<string>("connection_key_get", { connectionId: connection.id })
      .then((apiKey) => setEditor((current) => (
        current?.connectionId === connection.id && !current.apiKey ? { ...current, apiKey } : current
      )))
      .catch((error) => {
        setNotice(messageFrom(error));
        setNoticeSuccess(false);
      });
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
        modelId: current.providerKind === "openai_compatible" ? models[0]?.modelId ?? current.modelId : current.modelId,
        catalog: models.map((model) => ({ modelId: model.modelId, outputType: model.outputType, supportsReferenceImage: model.supportsReferenceImage })),
        catalogQuery: "",
        selectedModels: models.map((model) => model.modelId),
        validationToken: "",
      };
    });
    setNotice("");
  }

  async function selectEditorCategory(modelCategory: ModelCategory) {
    setEditorCategory(modelCategory);
    if (editor?.providerKind !== "openai_compatible" && editor?.connected) {
      await loadModels(modelCategory);
    }
  }

  async function loadModels(modelCategory = editor?.modelCategory) {
    if (!editor) return;
    const outputType = modelCategory ? categoryOutputType(modelCategory) : editor.outputType;
    setBusy(true);
    setNotice("");
    try {
      const response = await invoke<{ validationToken: string; models: CatalogModel[] }>("provider_models", {
        input: {
          connectionId: editor.connectionId,
          providerKind: editor.providerKind,
          baseUrl: editor.baseUrl,
          apiKey: editor.apiKey || undefined,
          outputType,
        },
      });
      setEditor((current) => current ? {
        ...current,
        connected: true,
        catalog: response.models,
        selectedModels: Array.from(new Set([
          ...current.selectedModels,
          ...current.savedModels
            .filter((model) => categoryOfModel(model) === categoryOfOutput(outputType) && response.models.some((item) => item.modelId === model.modelId))
            .map((model) => model.modelId),
        ])).filter((id) => response.models.some((model) => model.modelId === id)),
        imageCapableModels: Array.from(new Set([
          ...current.imageCapableModels.filter((id) => !response.models.some((model) => model.modelId === id)),
          ...response.models.filter((model) => model.supportsReferenceImage).map((model) => model.modelId),
        ])),
        validationToken: response.validationToken,
      } : current);
      setNotice(`连接成功，已加载 ${response.models.length} 个${categoryLabel(categoryOfOutput(outputType))}。`);
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
          apiKey: editor.apiKey || undefined,
          modelId: editor.modelId,
          outputType: editor.providerKind === "openai_compatible" ? editor.outputType : categoryOutputType(editor.modelCategory),
          audioInput: editor.outputType === "audio_to_text" ? audioInput?.dataUrl : undefined,
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
          apiKey: editor.apiKey || undefined,
          outputType: editor.providerKind === "openai_compatible" ? editor.outputType : categoryOutputType(editor.modelCategory),
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
    if (pendingConnectionRemoval !== connection.id) {
      setPendingConnectionRemoval(connection.id);
      return;
    }
    try {
      await invoke("connection_remove", { connectionId: connection.id });
      if (editor?.connectionId === connection.id) setEditor(undefined);
      await refreshSettings();
    } catch (error) {
      setNotice(messageFrom(error));
      setNoticeSuccess(false);
    } finally {
      setPendingConnectionRemoval(undefined);
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

  function chooseAudioInput(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) return;
    if (file.size > 50 * 1024 * 1024) {
      setNotice("音频文件不能超过 50 MB。");
      setNoticeSuccess(false);
      return;
    }
    const reader = new FileReader();
    reader.onload = () => {
      if (typeof reader.result === "string") {
        setAudioInput({ name: file.name, dataUrl: reader.result });
        setNotice("");
      }
    };
    reader.onerror = () => {
      setNotice("无法读取这个音频文件。");
      setNoticeSuccess(false);
    };
    reader.readAsDataURL(file);
  }

  async function run() {
    if (running || !enabledModels.length) return;
    setNotice("");
    setRunning(true);
    setResults(Object.fromEntries(enabledModels.map(({ model }) => [model.id, {
      runId: "",
      modelConfigId: model.id,
      status: "running",
      elapsedMs: 0,
    }])));
    try {
      const started = await invoke<{ runId: string }>("text_run_start", {
        input: {
          prompt,
          referenceImage: referenceImage?.dataUrl,
          audioInput: audioInput?.dataUrl,
          imageRatio: config.activeArenaType === "video" ? videoRatio : imageRatio,
          audioVoice,
          videoSeconds,
        },
      });
      setRunId(started.runId);
    } catch (error) {
      setRunning(false);
      setNotice(messageFrom(error));
      setNoticeSuccess(false);
      setResults({});
    }
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
              setSettingsCategory(categoryOfOutput(config.activeArenaType));
              setActivePage("settings");
            }}
          >模型配置</button>
        </nav>
      </header>

      {activePage === "arena" ? (<div className="arena-page">
      <div className="arena-switch-row">
        <nav className="type-filter arena-type-switch" aria-label="模型类型">
          {(["text", "image", "audio", "video"] as ModelCategory[]).map((category) => <button
            type="button"
            key={category}
            className={categoryOfOutput(config.activeArenaType) === category ? "is-active" : ""}
            aria-pressed={categoryOfOutput(config.activeArenaType) === category}
            onClick={() => void setArenaType(category === "audio" ? "audio" : categoryOutputType(category))}
            disabled={running}
          >{categoryLabel(category)}</button>)}
        </nav>
        {categoryOfOutput(config.activeArenaType) === "audio" && <nav className="type-filter arena-audio-switch" aria-label="音频任务类型">
          {(["audio", "audio_to_text"] as OutputType[]).map((outputType) => <button
            type="button"
            key={outputType}
            className={config.activeArenaType === outputType ? "is-active" : ""}
            aria-pressed={config.activeArenaType === outputType}
            onClick={() => void setArenaType(outputType)}
            disabled={running}
          >{outputType === "audio" ? "文本转音频" : "音频转文本"}</button>)}
        </nav>}
      </div>
      <section className="prompt-card" aria-label="提示词输入">
        {config.activeArenaType === "audio_to_text" ? (
          <div className="audio-input-copy">
            <strong>{audioInput ? audioInput.name : "添加一段音频，让所有语音识别模型同时转写"}</strong>
            <span>支持 mp3、mp4、mpeg、mpga、m4a、wav、webm，最大 50 MB</span>
          </div>
        ) : (
          <textarea
            value={prompt}
            onChange={(event) => {
              setPrompt(event.target.value);
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
          {config.activeArenaType === "audio" && (
            <SelectMenu
              label="生成音色"
              value={audioVoice}
              options={['alloy', 'echo', 'fable', 'onyx', 'nova', 'shimmer'].map((voice) => ({ value: voice, label: voice }))}
              onChange={setAudioVoice}
              disabled={running}
            />
          )}
          {config.activeArenaType === "audio_to_text" && <label className={`upload-button ${running ? "is-disabled" : ""}`}>
            ＋ {audioInput ? "更换音频" : "添加音频"}
            <input type="file" accept="audio/*,.mp4,.mpeg,.mpga,.m4a,.webm" onChange={chooseAudioInput} disabled={running} />
          </label>}
          {config.activeArenaType === "audio_to_text" && audioInput && (
            <button type="button" className="text-button" onClick={() => setAudioInput(undefined)} disabled={running}>移除音频</button>
          )}
          {config.activeArenaType === "video" && (
            <SelectMenu
              label="视频时长"
              value={videoSeconds}
              options={['4', '5', '8', '10'].map((seconds) => ({ value: seconds, label: `${seconds} 秒` }))}
              onChange={setVideoSeconds}
              disabled={running}
            />
          )}
        </div>
        <div className="prompt-actions">
          {running ? (
            <button className="secondary-button" onClick={cancel} disabled={!runId}>结束本次运行</button>
          ) : (
            <button className="primary-button" onClick={run} disabled={(config.activeArenaType === "audio_to_text" ? !audioInput : !prompt.trim()) || !enabledModels.length}>
              {{ text: "运行全部模型", image: "生成全部图片", audio: "生成全部音频", audio_to_text: "转写全部音频", video: "生成全部视频" }[config.activeArenaType]}
            </button>
          )}
        </div>
      </section>

      <section className="result-grid" aria-label="模型输出">
        {!loading && enabledModels.map(({ connection, model }) => {
          const result = results[model.id];
          const status = result?.status ?? (running ? "running" : "idle");
          const statusText = {
            idle: "待运行", running: "生成中", completed: "已完成", failed: "失败", ended: "已结束",
          }[status];
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
                <span className={`status status-${status}`}>{statusText}</span>
              </header>
              <div className="result-content">
                {result?.outputText && <p>{result.outputText}</p>}
                {result?.outputImage && <img src={result.outputImage} alt={`${model.displayName} 生成结果`} />}
                {result?.outputAudio && <audio src={mediaSrc(result.outputAudio)} controls preload="metadata" />}
                {result?.outputVideo && <video src={mediaSrc(result.outputVideo)} controls preload="metadata" />}
                {result?.error && <p className="error-copy" role="alert">{result.error.message}</p>}
              </div>
              <footer title={`连接：${connection.displayName}`}>
                <span>{result && result.status !== "running" ? `${(result.elapsedMs / 1000).toFixed(1)} 秒` : "-- 秒"}</span>
                <span>输入 {metric(result?.usage?.inputTokens)}</span>
                <span>输出 {metric(result?.usage?.outputTokens)}</span>
                <span>总计 {metric(result?.usage?.totalTokens)}</span>
              </footer>
            </article>
          );
        })}
        {!loading && !enabledModels.length && (
          <button className="model-card add-card" onClick={() => {
            setSettingsCategory(categoryOfOutput(config.activeArenaType));
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
                    {(["text", "image", "audio", "video"] as ModelCategory[]).map((category) => (
                      <button
                        type="button"
                        key={category}
                        className={settingsCategory === category ? "is-active" : ""}
                        aria-pressed={settingsCategory === category}
                        onClick={() => setSettingsCategory(category)}
                      >{categoryLabel(category)}</button>
                    ))}
                  </div>
                  {!!availableProviders.length && <div className="source-filters" role="group" aria-label="来源筛选">
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

                <section className="model-pool" aria-label={`${categoryLabel(settingsCategory)}模型池`}>
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

                {!!configuredConnections.length && <section className="connection-management">
                  <div className="connection-source-list">
                    <header className="section-card-header"><h3>连接来源</h3></header>
                    {configuredConnections.map((connection) => <div className="connection-source-row" key={connection.id}>
                      <div>
                        <strong>{connection.displayName}</strong>
                        <span>{providerLabel(connection.providerKind)} · {connectionTypeLabel(connection.models)} · {connection.models.length} 个模型</span>
                      </div>
                      <div className="row-actions">
                        <button
                          type="button"
                          className="ghost-action"
                          aria-label={`编辑${connection.displayName}`}
                          title="编辑连接"
                          onClick={() => startEdit(connection)}
                        ><EditIcon /><span>编辑</span></button>
                        <button
                          type="button"
                          className={`ghost-action danger-action ${pendingConnectionRemoval === connection.id ? "is-confirming" : ""}`}
                          aria-label={`${pendingConnectionRemoval === connection.id ? "确认删除" : "删除"}${connection.displayName}`}
                          title={pendingConnectionRemoval === connection.id ? "再次点击确认删除" : "删除连接"}
                          onClick={() => removeConnection(connection)}
                        ><TrashIcon /><span>{pendingConnectionRemoval === connection.id ? "确认删除" : "删除"}</span></button>
                      </div>
                    </div>)}
                  </div>
                </section>}
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
                    <label>
                      <span>API Key {!editor.connectionId && <b>*</b>}</span>
                      <input
                        value={editor.apiKey}
                        onChange={(event) => updateEditor("apiKey", event.target.value)}
                        type="password"
                        autoComplete="off"
                        placeholder={editor.hasCredential ? "正在读取已保存的 API Key…" : "仅保存到 macOS 钥匙串"}
                        required={!editor.connectionId}
                      />
                    </label>
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
                          ＋ {audioInput ? `测试音频：${audioInput.name}` : "添加测试音频"}
                          <input type="file" accept="audio/*,.mp4,.mpeg,.mpga,.m4a,.webm" onChange={chooseAudioInput} />
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
                            {(["text", "image", "audio", "video"] as ModelCategory[]).map((category) => (
                              <button
                                type="button"
                                key={category}
                                className={editor.modelCategory === category ? "is-active" : ""}
                                onClick={() => void selectEditorCategory(category)}
                                disabled={busy}
                              >{categoryLabel(category)}</button>
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
                            <strong>选择{categoryLabel(editor.modelCategory)}</strong>
                            <span>已选 {editor.selectedModels.length} 个</span>
                          </div>
                          <input
                            className="catalog-search"
                            value={editor.catalogQuery}
                            onChange={(event) => setEditor((current) => current ? { ...current, catalogQuery: event.target.value } : current)}
                            placeholder="搜索模型 ID"
                            aria-label="搜索模型"
                          />
                          <div className="catalog-list">
                            {(editor.modelCategory === "audio"
                              ? (["audio", "audio_to_text"] as OutputType[])
                              : [editor.outputType]
                            ).map((outputType) => {
                              const models = editor.catalog.filter((model) =>
                                (model.outputType ?? editor.outputType) === outputType
                                && model.modelId.toLowerCase().includes(editor.catalogQuery.trim().toLowerCase()));
                              if (!models.length) return null;
                              return <section className="catalog-section" key={outputType}>
                                {editor.modelCategory === "audio" && <h4>{capabilityLabels(outputType, false)[0]} · {models.length} 个</h4>}
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
                                      <span className="catalog-capability" title="由模型平台返回的能力信息">
                                        {capabilityLabels(outputType, model.supportsReferenceImage).map((label) => <em key={label}>{label}</em>)}
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
                      disabled={busy || (editor.outputType === "audio_to_text" && !audioInput)}
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
                    <button onClick={() => startAdd("aihubmix")}><PlusIcon /><span className="provider-option-copy"><strong>AIHubMix（梯子必须开全局模式）</strong><span>填一次 Key，选择多个模型</span></span></button>
                    <button onClick={() => startAdd("siliconflow")}><PlusIcon /><span className="provider-option-copy"><strong>硅基流动</strong><span>填一次 Key，选择多个模型</span></span></button>
                    <button onClick={() => startAdd("openai_compatible")}><PlusIcon /><span className="provider-option-copy"><strong>自定义单模型</strong><span>连接独立 OpenAI 兼容接口</span></span></button>
                  </div>
                </section>
              )}
            </div>
          </section>
      )}
      {notice && <div className={`global-toast ${noticeSuccess ? "is-success" : "is-error"}`} role="alert" aria-live="assertive"><ToastIcon success={noticeSuccess} /><span>{notice}</span></div>}
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
