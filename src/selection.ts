import { invoke } from "@tauri-apps/api/core";

export const DEFAULT_ASSISTANT_MODEL = "deepseek-flash";
export const selectionContextFields = [
  ["projectContext", "业务背景", "业务领域和要解决的问题"],
  ["usageScenario", "用户及使用场景", "谁在什么情况下使用这个 AI 环节，例如教师在线下备课"],
] as const;
export const requirementFields = [
  ["taskType", "任务类型", "希望 AI 做什么，可多选"],
  ["taskDifficulty", "任务难度", "AI 判断，可修改"],
  ["taskFocus", "任务侧重", "例如意图分类、信息抽取、代码生成、数学推理"],
  ["businessContext", "业务背景", "谁使用、什么场景、目前如何处理"],
  ["businessGoal", "希望 AI 完成", "描述 AI 在这个环节具体要做什么"],
  ["input", "输入", "提供给 AI 的材料或内容"],
  ["output", "输出", "希望 AI 生成的结果或格式"],
  ["accuracy", "准确性", "哪些错误不能接受，如何审核或验收"],
  ["speed", "速度", "实时还是异步，首次反馈和完整结果能等多久"],
  ["cost", "预算与用量", "可选，例如每月预算、每天调用量；不确定可留空"],
  ["constraints", "模型要求", "可选，例如长文上下文、JSON 输出、工具调用、私有化部署"],
  ["priorities", "业务优先级", "可多选；全选或不选表示无偏重"],
  ["fallback", "兜底要求", "哪些结果需要兜底，可接受的额外费用和等待"],
] as const;
export const taskTypes = ["文生文", "文生图", "语音转写", "语音合成", "图片理解", "视频理解", "视频生成", "代码生成"];
export function normalizeTaskType(value: string): string {
  const aliases: Record<string, string> = {
    "文本生成": "文生文", "文本理解": "文生文", "意图识别": "文生文", "填槽": "文生文", "课时编排": "文生文",
    "图片生成": "文生图", "互动课件生成": "代码生成", "html课件拼接": "代码生成", "html拼接": "代码生成", "html生成": "代码生成",
  };
  return Array.from(new Set(value.split(/[、，,]/).map((item) => {
    const key = item.replace(/\s/g, "");
    return taskTypes.includes(key) ? key : aliases[key.toLowerCase()];
  }).filter(Boolean))).join("、");
}
export const priorityOptions = ["准确性", "速度", "成本"];

export function selectedPriorities(value: string): string[] {
  const values = value.split(/\s*[>、]\s*/).filter(Boolean);
  return values.every((item) => priorityOptions.includes(item)) && new Set(values).size === values.length ? priorityOptions.filter((item) => values.includes(item)) : [];
}

export function priorityLabel(value: string): string {
  const selected = selectedPriorities(value);
  return selected.length === 0 || selected.length === 3 ? "无偏重" : selected.join("、");
}

export type FieldKey = typeof requirementFields[number][0];
export type AssistantSettings = { modelId: string; hasCredential: boolean };
export type ModelRole = "main" | "fallback";
export type RequirementField = { value: string; source: "provided" | "missing" | "user"; evidence: string };
export type RequirementTask = { id: string; name: string; userConcerns?: string[]; excludedUserConcerns?: string[]; fields: Record<FieldKey, RequirementField> };
export type RequirementCard = {
  description: string;
  excludeOverseas?: boolean;
  projectContext?: RequirementField | null;
  usageScenario?: RequirementField | null;
  targetUser?: RequirementField | null;
  tasks: RequirementTask[];
  suggestions: Array<{ id: string; taskIndex: number; field: FieldKey; value: string; reason: string; accepted: boolean }>;
};

export function userScenarioValue(card: RequirementCard): string {
  const user = card.targetUser?.value.trim() ?? "";
  const scenario = card.usageScenario?.value ?? "";
  return !user || scenario.includes(user) ? scenario : [user, scenario].filter((value) => value.trim()).join("；");
}
export type SelectionSession = {
  id: string;
  revision: number;
  description: string;
  running: boolean;
  storageError?: string;
  intentSelections?: string[];
  progress?: { message: string; startedAtMs: number; analysisPreview?: string; receivedChars?: number; modelStartedAtMs?: number | null; lastOutputAtMs?: number | null } | null;
  taskRuns?: Record<string, { research: ResearchResult | null; selectedModel: string | null; fallbackModel?: string | null }>;
  draft: RequirementCard | null;
  confirmed: { card: RequirementCard; revision: number; confirmedAtMs: number } | null;
  invocation: { modelId: string; connectionName: string; elapsedMs: number; usage?: { totalTokens?: number } } | null;
};

export type ConversationView = { card: RequirementCard | null; description: string; messages: string[]; editing: boolean };
export type ConversationSummary = { id: string; title: string; updatedAtMs: number };
export type SelectionWorkspace = { session: SelectionSession; view: ConversationView | null; conversations: ConversationSummary[] };

type ResearchEvidence = { sourceId: string; quote: string; kind?: string; label?: string };
export type ResearchResult = {
  status: "running" | "complete" | "partial" | "failed";
  revision: number;
  researchedAtMs?: number;
  asOfDate?: string;
  sources: Array<{ id: string; area: string; title: string; url: string; status: string; checkedAtMs: number; error: string }>;
  error: string;
  report: {
    notice?: string;
    findings: Array<{ area: string; summary: string; evidence: ResearchEvidence[] }>;
    groups: Array<{ taskId: string; priorityReason?: string; leaderboard?: ResearchEvidence | null; suitableModels?: Array<{ model: string; displayName?: string; origin?: "china" | "overseas"; reason: string; apiUrl?: string; keywords?: string[]; evidence: ResearchEvidence[] }>; candidates: Array<{ model: string; displayName?: string; origin?: "china" | "overseas"; role: string; mainRank?: number; fallbackRank?: number; basis?: "source" | "inference"; reason: string; apiUrl?: string; keywords?: string[]; evidence: ResearchEvidence[]; test: string; gaps: string[] }>; gaps: string[] }>;
    questions: string[];
  } | null;
};

export function emptyTask(): RequirementTask {
  return {
    id: crypto.randomUUID(), name: "待确认任务",
    fields: Object.fromEntries(requirementFields.map(([key]) => [key, { value: "", source: "missing", evidence: "" }])) as Record<FieldKey, RequirementField>,
  };
}

export function prefillSuggestions(card: RequirementCard): RequirementCard {
  const next = { ...card, tasks: card.tasks.map((task) => ({ ...task, fields: { ...emptyTask().fields, ...task.fields } })), suggestions: card.suggestions.map((item) => ({ ...item })) };
  for (const task of next.tasks) {
    const priority = task.fields.priorities;
    const selected = selectedPriorities(priority.value);
    if (selected.length) task.fields.priorities = { ...priority, value: selected.join("、") };
    const type = task.fields.taskType;
    const normalized = normalizeTaskType(type.value);
    task.fields.taskType = normalized ? { ...type, value: normalized } : { value: "", source: "missing", evidence: "" };
    const field = task.fields.constraints;
    const parts = field.value.split(/[；;。\n]/);
    const kept = parts.filter((part) => {
      const value = part.replace(/\s/g, "").toLowerCase();
      const name = task.name.replace(/\s/g, "").toLowerCase();
      return !["使用ai", "需要使用ai", "需使用ai", "必须使用ai", "需要ai", "需用ai", "必须用ai"].includes(value.startsWith(name) ? value.slice(name.length) : value);
    });
    if (kept.length !== parts.length) {
      const value = kept.map((part) => part.trim()).filter(Boolean).join("；");
      task.fields.constraints = value ? { ...field, value } : { value: "", source: "missing", evidence: "" };
      next.suggestions = next.suggestions.filter((item) => item.taskIndex !== next.tasks.indexOf(task) || item.field !== "constraints");
    }
  }
  for (const suggestion of next.suggestions) {
    if (suggestion.field === "taskType") suggestion.value = normalizeTaskType(suggestion.value);
    const task = next.tasks[suggestion.taskIndex];
    if (!task || !suggestion.value || !["taskType", "taskDifficulty", "taskFocus", "businessGoal", "input", "output", "priorities"].includes(suggestion.field) || task.fields[suggestion.field].value.trim()) continue;
    if (suggestion.field === "priorities") {
      suggestion.value = selectedPriorities(suggestion.value).join("、");
      if (!suggestion.value) continue;
    }
    task.fields[suggestion.field] = { value: suggestion.value, source: "user", evidence: "" };
    suggestion.accepted = true;
  }
  if (!card.description.includes("本次只为以下一个 AI 环节选型")) {
    next.tasks.forEach((task, index) => {
      const original = card.tasks[index].fields.priorities;
      if (original.source === "user" || (original.source === "provided" && original.value.trim())) return;
      const values = selectedPriorities(task.fields.priorities.value);
      if (!values.includes("成本")) {
        const value = priorityOptions.filter((option) => option === "成本" || values.includes(option)).join("、");
        task.fields.priorities = { value, source: "user", evidence: "" };
        for (const suggestion of next.suggestions) {
          if (suggestion.taskIndex === index && suggestion.field === "priorities" && suggestion.accepted) suggestion.value = value;
        }
      }
    });
  }
  next.suggestions = next.suggestions.filter((suggestion) => suggestion.value);
  return next;
}

export const selectionApi = {
  conversations: () => invoke<{ view: ConversationView | null; conversations: ConversationSummary[] }>("selection_conversations"),
  openConversation: (sessionId: string, revision: number, view: ConversationView, targetId: string | null = null) => invoke<SelectionWorkspace>("selection_conversation_open", { sessionId, revision, view, targetId }),
  saveConversation: (sessionId: string, revision: number, view: ConversationView) => invoke<ConversationSummary[]>("selection_conversation_save", { sessionId, revision, view }),
  openApi: (url: string) => invoke<void>("selection_open_api", { url }),
  get: () => invoke<SelectionSession>("selection_get"),
  reset: (sessionId: string) => invoke<SelectionSession>("selection_reset", { sessionId }),
  saveAssistant: (apiKey: string) => invoke<AssistantSettings>("selection_assistant_set", { input: { apiKey } }),
  extract: (input: { sessionId: string; revision: number; requestId: string; description: string }) => invoke<SelectionSession>("selection_extract", { input }),
  selectTask: (sessionId: string, revision: number, taskId: string) => invoke<SelectionSession>("selection_select_task", { sessionId, revision, taskId }),
  confirm: (input: { sessionId: string; revision: number; requestId: string; card: RequirementCard }) => invoke<SelectionSession>("selection_confirm", { input }),
  research: (sessionId: string, revision: number, taskId: string, retry = false) => {
    const today = new Date();
    const asOfDate = [today.getFullYear(), String(today.getMonth() + 1).padStart(2, "0"), String(today.getDate()).padStart(2, "0")].join("-");
    return invoke<SelectionSession>("research_command", { sessionId, revision, taskId, retry, asOfDate });
  },
  choose: (sessionId: string, revision: number, taskId: string, model: string, role: ModelRole) => invoke<SelectionSession>("selection_choose", { sessionId, revision, taskId, model, role }),
};

export function selectionError(error: unknown) {
  return error && typeof error === "object" && "message" in error
    ? String(error.message) : "操作失败，请稍后重试。";
}
