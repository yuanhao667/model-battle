import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import ModelSelection from "./ModelSelection";
import SelectionDriverSettings from "./SelectionDriverSettings";
import SelectionResearch from "./SelectionResearch";
import App from "./App";
import { AssistantSettings, DEFAULT_ASSISTANT_MODEL, emptyTask, prefillSuggestions, RequirementCard, SelectionSession } from "./selection";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke, convertFileSrc: (path: string) => path }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => undefined) }));

const assistant: AssistantSettings = { hasCredential: true, modelId: DEFAULT_ASSISTANT_MODEL };
const connections = [{ id: "c", displayName: "已有连接", hasCredential: true, providerKind: "siliconflow", baseUrl: "https://example.com/v1", models: [] }];
const initial = (): SelectionSession => ({ id: "session", revision: 0, description: "", running: false, draft: null, confirmed: null, invocation: null });
const card = (): RequirementCard => {
  const task = emptyTask();
  task.name = "会议转写";
  task.fields.taskType = { value: "语音转写", source: "provided", evidence: "录音转文字" };
  const projectContext = { value: "给运营使用", source: "provided" as const, evidence: "给运营使用" };
  task.fields.businessGoal = { value: "将会议录音转成文字", source: "provided", evidence: "会议录音转文字" };
  task.fields.speed = { value: "一分钟以内", source: "provided", evidence: "一分钟以内" };
  return { description: "给运营使用，会议录音转文字，节省整理时间，一分钟以内", projectContext, tasks: [task], suggestions: [{ id: "suggestion", taskIndex: 0, field: "output", value: "分条摘要", reason: "待用户确认的建议", accepted: false }] };
};

function mount() {
  render(<ModelSelection hidden={false} assistant={assistant} onAssistantSaved={vi.fn()} />);
}
function mockDraft() {
  const draft = card();
  invoke.mockImplementation((command: string) => Promise.resolve(command === "selection_get" ? { ...initial(), revision: 1, description: draft.description, draft } : {}));
  return draft;
}

describe("model selection", () => {
  afterEach(cleanup);
  beforeEach(() => {
    invoke.mockReset();
    invoke.mockImplementation((command: string) => {
      if (command === "selection_get") return Promise.resolve(initial());
      if (command === "settings_get") return Promise.resolve({ schemaVersion: 1, activeArenaType: "text", connections, selectionAssistant: assistant });
      return Promise.resolve({});
    });
  });

  it("saves only a dedicated official key and clears it from the form without invoking a model", async () => {
    const user = userEvent.setup();
    const onSaved = vi.fn();
    invoke.mockResolvedValue(assistant);
    render(<SelectionDriverSettings assistant={{ hasCredential: false, modelId: DEFAULT_ASSISTANT_MODEL }} onSaved={onSaved} />);
    expect(screen.getByText("DeepSeek V4.1 Flash 1017")).toBeVisible();
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
    expect(screen.queryByText("官网 API Key")).not.toBeInTheDocument();
    const input = screen.getByLabelText("官网 API Key");
    expect(input).toHaveAttribute("type", "password");
    await user.type(input, "sk-test-only-key");
    await user.click(screen.getByRole("button", { name: "保存 API Key" }));
    await waitFor(() => expect(onSaved).toHaveBeenCalledWith(assistant));
    expect(invoke).toHaveBeenCalledWith("selection_assistant_set", { input: { apiKey: "sk-test-only-key" } });
    expect(input).toHaveValue("");
    expect(invoke.mock.calls.filter(([command]) => command === "selection_extract")).toHaveLength(0);
  });

  it("saves the key on the homepage, restores the composer, and synchronizes settings without exposing the key", async () => {
    const user = userEvent.setup();
    invoke.mockImplementation((command: string) => {
      if (command === "settings_get") return Promise.resolve({ schemaVersion: 1, activeArenaType: "text", connections, selectionAssistant: { ...assistant, hasCredential: false } });
      if (command === "selection_assistant_set") return Promise.resolve(assistant);
      return Promise.resolve(initial());
    });
    render(<App />);
    const input = await screen.findByPlaceholderText("粘贴 DeepSeek 官网 API Key");
    expect(screen.getByText("配置模型选型驱动 AI")).toBeVisible();
    expect(screen.queryByText("DeepSeek V4.1 Flash 1017")).not.toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "你希望 AI 完成什么工作？" })).not.toBeInTheDocument();
    expect(input).toHaveAttribute("type", "password");
    expect(input.closest(".selection-driver-settings")).toHaveClass("is-inline");
    await user.type(input, "sk-homepage-test-only");
    await user.click(screen.getByRole("button", { name: "保存 API Key" }));
    expect(await screen.findByRole("textbox", { name: "你希望 AI 完成什么工作？" })).toHaveAttribute("placeholder", expect.stringContaining("简要描述业务背景"));
    expect(screen.queryByPlaceholderText("粘贴 DeepSeek 官网 API Key")).not.toBeInTheDocument();
    expect(invoke.mock.calls.filter(([command]) => command === "selection_extract")).toHaveLength(0);
    expect(invoke.mock.calls.filter(([command]) => command === "selection_assistant_set")).toHaveLength(1);
    await user.click(screen.getByRole("button", { name: "模型配置" }));
    const driver = await screen.findByRole("region", { name: "模型选型驱动 AI" });
    const savedInput = within(driver).getByLabelText("官网 API Key");
    expect(savedInput).toHaveValue("•".repeat(32));
    expect(savedInput).toHaveAttribute("readonly");
    expect(within(driver).getByRole("button", { name: "更换" })).toBeVisible();
    expect(screen.queryByText("sk-homepage-test-only")).not.toBeInTheDocument();
  });

  it("shows a saved-key mask and only saves a replacement after an explicit edit", async () => {
    const user = userEvent.setup();
    const onSaved = vi.fn();
    invoke.mockRejectedValueOnce(new Error("保存失败")).mockResolvedValueOnce(assistant);
    render(<SelectionDriverSettings assistant={assistant} onSaved={onSaved} />);
    const input = screen.getByLabelText("官网 API Key");
    expect(input).toHaveValue("•".repeat(32));
    expect(input).toHaveAttribute("readonly");
    expect(screen.queryByRole("button", { name: "保存 API Key" })).not.toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "更换" }));
    expect(input).toHaveFocus();
    expect(input).toHaveValue("");
    await user.tab();
    expect(input).toHaveValue("•".repeat(32));
    await user.click(screen.getByRole("button", { name: "更换" }));
    await user.type(input, "sk-replacement-test-only");
    await user.click(screen.getByRole("button", { name: "保存 API Key" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("保存失败");
    expect(input).toHaveValue("sk-replacement-test-only");
    expect(onSaved).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "保存 API Key" }));
    await waitFor(() => expect(onSaved).toHaveBeenCalledWith(assistant));
    expect(input).toHaveValue("•".repeat(32));
    expect(input).toHaveAttribute("readonly");
    expect(invoke).toHaveBeenLastCalledWith("selection_assistant_set", { input: { apiKey: "sk-replacement-test-only" } });
  });

  it("sends a chat message, receives a prefilled card, and keeps the composer available", async () => {
    const user = userEvent.setup();
    const draft = card();
    invoke.mockImplementation((command: string) => Promise.resolve(command === "selection_extract" ? { ...initial(), revision: 1, description: draft.description, draft } : initial()));
    mount();
    const input = screen.getByRole("textbox", { name: "你希望 AI 完成什么工作？" });
    await waitFor(() => expect(input).toBeEnabled());
    expect(input).toHaveAttribute("placeholder", "简要描述业务背景、目标用户、使用场景，以及一个 AI 环节的工作、输入和输出；每次只为一个环节选模型。");
    expect(screen.queryByRole("group", { name: "需求确认卡片" })).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Model Battle" })).toBeVisible();
    expect(input.closest(".selection-page")).toHaveClass("is-landing");
    await user.type(input, draft.description);
    await user.click(screen.getByRole("button", { name: "发送需求" }));
    expect(await screen.findByRole("group", { name: "需求确认卡片" })).toBeVisible();
    const firstCard = screen.getByRole("article", { name: "任务 1 需求确认卡片" });
    expect(firstCard).toHaveClass("selection-card");
    expect(screen.queryByRole("tablist")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "添加任务" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "移除任务" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "合并至上一项" })).not.toBeInTheDocument();
    expect(firstCard.querySelector(".selection-card")).toBeNull();
    expect(screen.getByRole("article", { name: "任务 1 需求确认卡片" }).parentElement).not.toHaveClass("selection-card");
    expect(screen.queryByRole("heading", { name: "Model Battle" })).not.toBeInTheDocument();
    expect(input.closest(".selection-page")).not.toHaveClass("is-landing");
    expect(screen.getByRole("textbox", { name: "业务背景" })).toHaveValue("给运营使用");
    expect(screen.getByRole("textbox", { name: "业务背景" }).tagName).toBe("TEXTAREA");
    expect(within(firstCard).getByRole("textbox", { name: "业务背景" })).toBeVisible();
    expect(screen.queryByRole("heading", { name: "需求确认卡片" })).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "任务 1：希望 AI 完成" })).toHaveAttribute("placeholder", "描述 AI 在这个环节具体要做什么");
    expect(screen.queryByText("业务目标")).not.toBeInTheDocument();
    expect(screen.queryByText("来自原话")).not.toBeInTheDocument();
    expect(screen.queryByText("原话依据")).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "任务 1：预算与用量" })).toHaveValue("");
    expect(screen.getByRole("textbox", { name: "任务 1：模型要求" })).toHaveValue("");
    expect(screen.queryByRole("textbox", { name: "任务 1：速度" })).not.toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "任务 1：准确性" })).not.toBeInTheDocument();
    expect(screen.getByRole("log", { name: "选型对话" })).toHaveTextContent(draft.description);
    expect(input).toBeEnabled();
    expect(input).toHaveValue("");
  });

  it("shows the driver by Send and displays real per-conversation metrics after entering chat", async () => {
    const user = userEvent.setup();
    const draft = card();
    const latest = { modelId: DEFAULT_ASSISTANT_MODEL, connectionName: "DeepSeek", elapsedMs: 3000, firstTokenMs: 1000, usage: { inputTokens: 40000, outputTokens: 10000, totalTokens: 50000, cachedInputTokens: 960 } };
    invoke.mockImplementation((command: string) => Promise.resolve(command === "selection_extract" ? {
      ...initial(), revision: 1, description: draft.description, draft, invocation: latest,
      metrics: { completedCalls: 2, usageCalls: 2, totalTokens: 100000, cacheHitTokens: 1920, cacheInputTokens: 2000, latest },
    } : initial()));
    mount();
    const send = screen.getByRole("button", { name: "发送需求" });
    expect(send.previousElementSibling).toHaveTextContent("DeepSeek V4.1 Flash");
    expect(screen.queryByRole("group", { name: "对话用量统计" })).not.toBeInTheDocument();
    const input = screen.getByRole("textbox", { name: "你希望 AI 完成什么工作？" });
    await waitFor(() => expect(input).toBeEnabled());
    await user.type(input, draft.description);
    await user.click(send);
    const stats = await screen.findByRole("group", { name: "对话用量统计" });
    await waitFor(() => expect(stats).toHaveTextContent("1 轮 · 2 次调用 · 100K tok"));
    expect(within(stats).getByRole("progressbar", { name: "上下文占用" })).toHaveAttribute("aria-valuenow", "5");
    expect(stats).toHaveTextContent("5%");
    expect(stats.children).toHaveLength(2);
    expect(stats).not.toHaveTextContent(/tok\/s|缓存命中|本次输入|秒/);
    expect(send.previousElementSibling).toHaveTextContent("DeepSeek V4.1 Flash");
    expect(stats.parentElement).toContainElement(input.closest(".selection-composer"));
  });

  it("keeps unavailable context unknown and shows partial totals without rounding small usage to zero", async () => {
    const draft = card();
    const latest = { modelId: DEFAULT_ASSISTANT_MODEL, connectionName: "DeepSeek", elapsedMs: 1000, usage: { inputTokens: 100, outputTokens: 10, totalTokens: 110, cachedInputTokens: 0 } };
    invoke.mockResolvedValue({ ...initial(), description: draft.description, draft, metrics: { completedCalls: 2, usageCalls: 1, totalTokens: 110, cacheHitTokens: 0, cacheInputTokens: 100, latest } });
    mount();
    const stats = await screen.findByRole("group", { name: "对话用量统计" });
    expect(stats).toHaveTextContent("已统计 110 tok");
    expect(stats).toHaveTextContent("<0.1%");
    cleanup();
    invoke.mockResolvedValue({ ...initial(), description: draft.description, draft });
    mount();
    const missing = await screen.findByRole("group", { name: "对话用量统计" });
    expect(missing).toHaveTextContent("— tok");
    expect(missing).toHaveTextContent("—%");
    expect(within(missing).getByRole("progressbar")).not.toHaveAttribute("aria-valuenow");
  });

  it.each([[0, 0], [1500000, 100]])("bounds the context ring for %s tokens", async (totalTokens, percent) => {
    const draft = card();
    invoke.mockResolvedValue({ ...initial(), description: draft.description, draft, invocation: { modelId: DEFAULT_ASSISTANT_MODEL, connectionName: "DeepSeek", elapsedMs: 1000, usage: { totalTokens } } });
    mount();
    const ring = await screen.findByRole("progressbar", { name: "上下文占用" });
    expect(ring).toHaveAttribute("aria-valuenow", String(percent));
    expect(ring).toHaveTextContent(`${percent}%`);
  });

  it("keeps the existing card and submits optional budget and model requirements", async () => {
    const user = userEvent.setup();
    const draft = card();
    invoke.mockImplementation((command: string) => command === "selection_confirm"
      ? Promise.reject({ message: "测试保留当前编辑" })
      : Promise.resolve({ ...initial(), revision: 1, draft }));
    mount();
    const budget = await screen.findByRole("textbox", { name: "任务 1：预算与用量" });
    const requirements = screen.getByRole("textbox", { name: "任务 1：模型要求" });
    expect(budget).not.toBeRequired();
    expect(requirements).not.toBeRequired();
    await user.type(budget, "每月200元，每天100次");
    await user.type(requirements, "128K上下文、JSON输出");
    await user.click(screen.getByRole("button", { name: "确认并开始选型" }));
    expect(await screen.findByText("测试保留当前编辑")).toBeVisible();
    const sent = invoke.mock.calls.find(([command]) => command === "selection_confirm")![1].input.card as RequirementCard;
    expect(sent.tasks[0].fields.cost).toMatchObject({ value: "每月200元，每天100次", source: "user" });
    expect(sent.tasks[0].fields.constraints).toMatchObject({ value: "128K上下文、JSON输出", source: "user" });
    expect(sent.tasks[0].fields.businessGoal.value).toBe(draft.tasks[0].fields.businessGoal.value);
    expect(sent.tasks[0].fields.output.value).toBe("分条摘要");
    expect(requirements).toHaveValue("128K上下文、JSON输出");
  });

  it("prefills task difficulty and focus, submits user changes, and preserves them when reopened", async () => {
    const user = userEvent.setup();
    const draft = card();
    draft.suggestions.push(
      { id: "difficulty", taskIndex: 0, field: "taskDifficulty", value: "易", reason: "按既定字段提取", accepted: false },
      { id: "focus", taskIndex: 0, field: "taskFocus", value: "意图分类、信息抽取", reason: "分类并填槽", accepted: false },
    );
    let current: SelectionSession = { ...initial(), draft };
    invoke.mockImplementation((command: string, args: any) => {
      if (command === "selection_confirm") current = { ...current, confirmed: { card: args.input.card, revision: 1, confirmedAtMs: 1 } };
      return Promise.resolve(current);
    });
    mount();
    const difficulty = await screen.findByRole("radiogroup", { name: "任务 1：任务难度" });
    const focus = screen.getByRole("textbox", { name: "任务 1：任务侧重" });
    expect(within(difficulty).getByRole("radio", { name: "易" })).toBeChecked();
    expect(focus).toHaveValue("意图分类、信息抽取");
    expect(within(difficulty).getAllByRole("radio").map((option) => option.getAttribute("value"))).toEqual(["易", "中", "难"]);
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
    await user.click(within(difficulty).getByRole("radio", { name: "难" }));
    await user.clear(focus); await user.type(focus, "代码生成、算法推理");
    await user.click(screen.getByRole("button", { name: "确认并开始选型" }));
    await user.click(await screen.findByRole("button", { name: "修改需求" }));
    expect(within(screen.getByRole("radiogroup", { name: "任务 1：任务难度" })).getByRole("radio", { name: "难" })).toBeChecked();
    expect(screen.getByRole("textbox", { name: "任务 1：任务侧重" })).toHaveValue("代码生成、算法推理");
    const sent = invoke.mock.calls.find(([command]) => command === "selection_confirm")![1].input.card;
    expect(sent.tasks[0].fields.taskDifficulty).toMatchObject({ value: "难", source: "user" });
    expect(sent.tasks[0].fields.taskFocus).toMatchObject({ value: "代码生成、算法推理", source: "user" });
    expect(invoke.mock.calls.filter(([command]) => command === "research_command")).toHaveLength(1);
  });

  it("allows multiple priorities and preserves edits after confirmation failure", async () => {
    const user = userEvent.setup();
    const draft = card();
    draft.suggestions.push({ id: "priority", taskIndex: 0, field: "priorities", value: "准确性", reason: "会议内容需忠实保留，推荐优先准确性。", accepted: false });
    let confirmations = 0;
    let confirmedSession = initial();
    invoke.mockImplementation((command: string, args?: { input: { card: RequirementCard } }) => {
      if (command === "selection_get") return Promise.resolve({ ...initial(), revision: 1, description: draft.description, draft });
      if (command === "selection_confirm") {
        if (++confirmations === 1) return Promise.reject({ message: "确认暂时失败" });
        confirmedSession = { ...initial(), revision: 2, draft: args!.input.card, confirmed: { card: args!.input.card, revision: 2, confirmedAtMs: 1 } };
        return Promise.resolve(confirmedSession);
      }
      if (command === "research_command") return Promise.resolve({ ...confirmedSession, taskRuns: { [confirmedSession.confirmed!.card.tasks[0].id]: { selectedModel: null, research: { status: "failed", revision: 2, sources: [], report: null, error: "资料服务暂不可用" } } } });
      return Promise.resolve({});
    });
    mount();
    const goal = await screen.findByRole("textbox", { name: "任务 1：希望 AI 完成" });
    await user.clear(goal);
    await user.type(goal, "提高整理效率");
    await user.click(screen.getByRole("button", { name: "清除任务 1 的速度要求" }));
    const priorities = screen.getByRole("group", { name: "任务 1 优先级" });
    expect(within(priorities).getByRole("checkbox", { name: "准确性" })).toBeChecked();
    expect(within(priorities).getByRole("checkbox", { name: "准确性" })).toBeChecked();
    expect(within(priorities).getByRole("checkbox", { name: "成本" })).toBeChecked();
    await user.click(within(priorities).getByRole("checkbox", { name: "速度" }));
    expect(within(priorities).getAllByRole("checkbox").every((input) => (input as HTMLInputElement).checked)).toBe(true);
    await user.click(screen.getByRole("button", { name: "确认并开始选型" }));
    expect(await screen.findByText("确认暂时失败")).toBeVisible();
    expect(goal).toHaveValue("提高整理效率");
    await user.click(screen.getByRole("button", { name: "确认并开始选型" }));
    expect(await screen.findByRole("button", { name: "重试生成" })).toBeVisible();
    expect(invoke.mock.calls.filter(([command]) => command === "research_command")).toHaveLength(1);
    expect(screen.getByText("资料服务暂不可用")).toBeVisible();
    const requests = invoke.mock.calls.filter(([command]) => command === "selection_confirm");
    const sent = requests[requests.length - 1][1].input.card as RequirementCard;
    expect(sent.tasks[0].fields.priorities).toMatchObject({ value: "准确性、速度、成本", source: "user" });
    expect(sent.tasks[0].fields.speed).toMatchObject({ value: "", source: "missing" });
    expect(sent.suggestions[0].accepted).toBe(true);
    expect(sent.tasks[0].fields.output.value).toBe("分条摘要");
  });

  it("prevents duplicate paid calls, retains failed input, and offers manual fallback only after failure", async () => {
    const user = userEvent.setup();
    let fail: (reason: unknown) => void = () => undefined;
    invoke.mockImplementation((command: string) => command === "selection_extract"
      ? new Promise((_, reject) => { fail = reject; }) : Promise.resolve(initial()));
    mount();
    const input = screen.getByRole("textbox", { name: "你希望 AI 完成什么工作？" });
    await waitFor(() => expect(input).toBeEnabled());
    expect(screen.queryByRole("button", { name: "手工填写" })).not.toBeInTheDocument();
    await user.type(input, "会议录音转文字");
    await user.dblClick(screen.getByRole("button", { name: "发送需求" }));
    expect(invoke.mock.calls.filter(([command]) => command === "selection_extract")).toHaveLength(1);
    await act(async () => fail({ message: "连接超时" }));
    expect(await screen.findByText("连接超时")).toBeVisible();
    expect(input).toHaveValue("会议录音转文字");
    expect(screen.queryByRole("group", { name: "需求确认卡片" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "手工填写" }));
    expect(screen.getByRole("textbox", { name: "业务背景" })).toHaveValue("");
    expect(screen.getByRole("log")).toHaveTextContent("会议录音转文字");
  });

  it("allows clearing every priority and keeps the confirmed summary balanced", async () => {
    const user = userEvent.setup();
    let current: SelectionSession = { ...initial(), draft: card() };
    current.draft!.tasks[0].fields.priorities = { value: "准确性", source: "user", evidence: "" };
    invoke.mockImplementation((command: string, args: any) => {
      if (command === "selection_confirm") current = { ...current, confirmed: { card: args.input.card, revision: 1, confirmedAtMs: 1 } };
      return Promise.resolve(current);
    });
    mount();
    const accuracy = await screen.findByRole("checkbox", { name: "准确性" });
    expect(accuracy).toBeChecked();
    await user.click(accuracy);
    expect(within(screen.getByRole("group", { name: "任务 1 优先级" })).getAllByRole("checkbox").every((input) => !(input as HTMLInputElement).checked)).toBe(true);
    await user.click(screen.getByRole("button", { name: "确认并开始选型" }));
    await screen.findByRole("button", { name: "修改需求" });
    const sent = invoke.mock.calls.find(([command]) => command === "selection_confirm")![1].input.card;
    expect(sent.tasks[0].fields.priorities).toMatchObject({ value: "", source: "missing" });
    expect(screen.queryByText("希望 AI 完成的工作")).not.toBeInTheDocument();
  });

  it.each(["running", "failed"] as const)("omits source logs while research is %s", async (status) => {
    const draft = card();
    invoke.mockResolvedValue({ ...initial(), running: status === "running", confirmed: { card: draft, revision: 1, confirmedAtMs: 1 }, taskRuns: { [draft.tasks[0].id]: { selectedModel: null, research: {
      status, revision: 1, report: null, error: status === "failed" ? "资料不足，未生成候选。" : "", sources: [
        { id: "arena", area: "ranking", title: "Arena", url: "https://arena.ai/leaderboard/agent", status: "failed", checkedAtMs: 1, error: "来源网站要求访问验证（HTTP 403），自动读取被阻止。" },
        { id: "search", area: "capabilities", title: "DeepSeek 官网搜索", url: "", status: "failed", checkedAtMs: 1, error: "DeepSeek 官网搜索：请求被限流（HTTP 429）。" },
        { id: "docs", area: "cost", title: "DeepSeek 模型与价格", url: "https://api-docs.deepseek.com/quick_start/pricing", status: "read", checkedAtMs: 1, error: "官网读取暂不可用，已通过备用读取取得正文。" },
      ],
    } } } });
    mount();
    await screen.findByRole("button", { name: "修改需求" });
    expect(screen.queryByText(/HTTP 403|HTTP 429|已通过备用读取取得正文|正文已读取/)).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "DeepSeek 模型与价格" })).not.toBeInTheDocument();
    if (status === "failed") {
      expect(screen.getByRole("alert")).toHaveTextContent("资料不足，未生成候选。");
      expect(screen.getByRole("button", { name: "重试生成" })).toBeVisible();
    }
  });

  it("automatically shows concise recommendations with business priority reasoning and citations, without folds or annotations", async () => {
    const user = userEvent.setup();
    const draft = card();
    const reason = "支持会议语音转写，适合将运营会议录音整理成文字。";
    const priorityReason = "运营需要在一分钟内拿到会议文字，因此关注转写耗时，本次优先速度。";
    draft.suggestions.push({ id: "priority", taskIndex: 0, field: "priorities", value: "速度", reason: priorityReason, accepted: false });
    let current: SelectionSession = { ...initial(), draft };
    invoke.mockImplementation((command: string, args: any) => {
      if (command === "selection_confirm") current = { ...current, revision: 1, confirmed: { card: args.input.card, revision: 1, confirmedAtMs: 1 } };
      if (command === "research_command") current = { ...current, taskRuns: { [args.taskId]: { selectedModel: null, research: {
        status: "partial", revision: 1, error: "", sources: [
          { id: "docs", area: "capabilities", title: "模型官方文档", url: "https://example.com/docs", status: "read", checkedAtMs: 1, error: "" },
          { id: "arena", area: "ranking", title: "Arena", url: "https://arena.ai/leaderboard/agent", status: "failed", checkedAtMs: 1, error: "访问验证（HTTP 403）" },
        ], report: { findings: [{ area: "ranking", summary: "Arena 排名暂不可核验", evidence: [] }], groups: [{ taskId: args.taskId, priorityReason, candidates: [{ model: "Model A", role: "main", reason, evidence: [{ sourceId: "docs", quote: "supports transcription" }], test: "记录业务录音的转写耗时与错误", gaps: [] }], gaps: [] }], questions: [] },
      } } } };
      return Promise.resolve(current);
    });
    mount();
    await user.click(await screen.findByRole("button", { name: "确认并开始选型" }));
    await screen.findByRole("heading", { name: /^入围模型（/ });
    const recommendation = screen.getByText(reason).closest("article")!;
    expect(screen.getByText(priorityReason)).toBeVisible();
    expect(within(recommendation).getByText(reason)).toBeVisible();
    expect(within(recommendation).getByRole("link", { name: "模型官方文档" })).toHaveAttribute("href", "https://example.com/docs");
    expect(within(recommendation).getByRole("link")).toHaveAttribute("title", "supports transcription");
    expect(screen.queryByText("调研依据与来源")).not.toBeInTheDocument();
    expect(screen.queryByText("需求详情")).not.toBeInTheDocument();
    expect(screen.queryByText("主力候选")).not.toBeInTheDocument();
    expect(screen.queryByText(/待测：|待核验：/)).not.toBeInTheDocument();
    expect(screen.getByRole("log").querySelectorAll("details")).toHaveLength(0);
    expect(screen.queryByText("已检索到的适合模型（0）")).not.toBeInTheDocument();
    expect(invoke.mock.calls.filter(([command]) => command === "research_command")).toHaveLength(1);
    expect(screen.queryByText("访问验证（HTTP 403）")).not.toBeInTheDocument();
  });

  it("connects business context to priorities and preserves inferred end-user concerns on confirmation", async () => {
    const user = userEvent.setup();
    const draft = mockDraft();
    draft.usageScenario = { value: "线下备课", source: "user", evidence: "" };
    draft.targetUser = { value: "教师", source: "user", evidence: "" };
    draft.tasks[0].userConcerns = ["缺项主动追问", "结构化字段输出", "多轮上下文衔接"];
    let current: SelectionSession = { ...initial(), draft };
    invoke.mockImplementation((command: string, args: any) => {
      if (command === "selection_confirm") current = { ...current, confirmed: { card: args.input.card, revision: 1, confirmedAtMs: 1 } };
      return Promise.resolve(current);
    });
    mount();
    const derivation = await screen.findByRole("group", { name: "业务背景与用户及使用场景共同推导业务优先级" });
    expect(within(derivation).getByRole("textbox", { name: "业务背景" })).toHaveValue(draft.projectContext!.value);
    const userScenario = within(derivation).getByRole("textbox", { name: "用户及使用场景" });
    expect(userScenario).toHaveValue("教师；线下备课");
    expect(within(derivation).queryByRole("textbox", { name: "用户" })).not.toBeInTheDocument();
    expect(within(derivation).queryByRole("textbox", { name: "使用场景" })).not.toBeInTheDocument();
    await user.clear(userScenario);
    await user.type(userScenario, "小学教师，课后在线整理教案");
    expect(within(derivation).queryByText("↓")).not.toBeInTheDocument();
    expect(Array.from(derivation.querySelectorAll('.selection-derivation-inputs textarea')).map((input) => input.getAttribute('id'))).toEqual(['selection-context-projectContext', 'selection-context-usageScenario']);
    const priorities = within(derivation).getByRole("group", { name: "任务 1 优先级" });
    expect(within(priorities).getAllByRole("checkbox")).toHaveLength(6);
    for (const concern of draft.tasks[0].userConcerns!) expect(within(priorities).getByText(concern)).toBeVisible();
    await user.click(screen.getByRole("button", { name: "确认并开始选型" }));
    await screen.findByRole("button", { name: "修改需求" });
    expect(invoke.mock.calls.find(([command]) => command === "selection_confirm")![1].input.card.tasks[0].userConcerns).toEqual(draft.tasks[0].userConcerns);
    expect(invoke.mock.calls.find(([command]) => command === "selection_confirm")![1].input.card.targetUser).toBeNull();
    expect(invoke.mock.calls.find(([command]) => command === "selection_confirm")![1].input.card.usageScenario.value).toBe("小学教师，课后在线整理教案");
  });

  it("shows the shortlist and business-fit reason first even without public benchmark evidence", async () => {
    const draft = card();
    const reason = "建议先用于中文意图识别和结构化填槽，验证教师需求能否被正确理解。";
    invoke.mockResolvedValue({ ...initial(), confirmed: { card: draft, revision: 1, confirmedAtMs: 1 }, taskRuns: { [draft.tasks[0].id]: { selectedModel: null, research: {
      status: "partial", revision: 1, sources: [], error: "", report: { findings: [], groups: [{ taskId: draft.tasks[0].id, priorityReason: "教师需要正确理解且及时反馈。", candidates: [{ model: "Known Text Model", role: "main", basis: "inference", reason, evidence: [], test: "核对意图", gaps: ["没有公开任务成绩"] }], gaps: ["没有业务样本"] }], questions: ["请先提供专业指标"] },
    } } } });
    mount();
    expect(await screen.findByRole("heading", { name: /^入围模型（/ })).toBeVisible();
    const model = screen.getByText("Known Text Model");
    expect(model).toBeVisible();
    expect(screen.getByText(reason)).toBeVisible();
    expect(within(screen.getByRole("region", { name: "选型调研" })).getByText("业务优先级").compareDocumentPosition(model) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(screen.queryByText(/暂未找到有依据|没有业务样本|请先提供专业指标/)).not.toBeInTheDocument();
    expect(within(screen.getByRole("region", { name: "选型调研" })).queryByRole("radio")).not.toBeInTheDocument();
    expect(screen.getByRole("img", { name: "主力模型：待评级" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "选用此模型" })).not.toBeInTheDocument();
  });

  it("only allows selecting, deleting and adding business tags, with no editable saved names", async () => {
    const user = userEvent.setup();
    const draft = card();
    draft.tasks[0].userConcerns = ["缺项主动追问", "结构化字段输出"];
    draft.tasks[0].fields.priorities = { value: "准确性、速度", source: "user", evidence: "" };
    let current: SelectionSession = { ...initial(), draft };
    invoke.mockImplementation((command: string, args: any) => {
      if (command === "selection_confirm") current = { ...current, confirmed: { card: args.input.card, revision: 1, confirmedAtMs: 1 } };
      return Promise.resolve(current);
    });
    mount();
    const tag = await screen.findByText("结构化字段输出");
    expect(tag.tagName).toBe("LABEL");
    expect(screen.queryByDisplayValue("结构化字段输出")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "删除业务标签 结构化字段输出" }));
    await user.click(screen.getByRole("button", { name: "＋ 添加标签" }));
    await user.type(screen.getByRole("textbox", { name: "新业务标签" }), "结构化 JSON 输出{Enter}");
    expect(screen.getByText("结构化 JSON 输出").tagName).toBe("LABEL");
    expect(screen.queryByRole("textbox", { name: "新业务标签" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "删除业务标签 缺项主动追问" }));
    expect(screen.queryByText("缺项主动追问")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "＋ 添加标签" }));
    const added = screen.getByRole("textbox", { name: "新业务标签" });
    expect(added).toHaveFocus();
    await user.type(added, "多轮上下文衔接{Enter}");
    const priorities = screen.getByRole("group", { name: "任务 1 优先级" });
    expect(within(priorities).getAllByRole("checkbox")).toHaveLength(5);
    expect(within(priorities).getByRole("checkbox", { name: "速度" })).toBeChecked();
    await user.click(screen.getByRole("button", { name: "确认并开始选型" }));
    await screen.findByRole("button", { name: "修改需求" });
    const confirmed = invoke.mock.calls.find(([command]) => command === "selection_confirm")![1].input.card;
    expect(confirmed.tasks[0].userConcerns).toEqual(["结构化 JSON 输出", "多轮上下文衔接"]);
    expect(confirmed.tasks[0].fields.priorities.value).toBe("准确性、速度");
  });

  it("defaults overseas exclusion off and preserves the user's domestic-only choice on confirmation and editing", async () => {
    const user = userEvent.setup();
    let current: SelectionSession = { ...initial(), draft: card() };
    invoke.mockImplementation((command: string, args: any) => {
      if (command === "selection_confirm") current = { ...current, confirmed: { card: args.input.card, revision: 1, confirmedAtMs: 1 } };
      return Promise.resolve(current);
    });
    mount();
    const filter = await screen.findByRole("checkbox", { name: "不看海外模型" });
    expect(filter).not.toBeChecked();
    await user.click(filter);
    await user.click(screen.getByRole("button", { name: "确认并开始选型" }));
    await user.click(await screen.findByRole("button", { name: "修改需求" }));
    expect(screen.getByRole("checkbox", { name: "不看海外模型" })).toBeChecked();
    expect(invoke.mock.calls.find(([command]) => command === "selection_confirm")![1].input.card.excludeOverseas).toBe(true);
    await user.click(screen.getByRole("checkbox", { name: "不看海外模型" }));
    expect(screen.getByRole("checkbox", { name: "不看海外模型" })).not.toBeChecked();
  });

  it("does not send a blank new tag when users confirm without filling it", async () => {
    const user = userEvent.setup();
    let current: SelectionSession = { ...initial(), draft: card() };
    invoke.mockImplementation((command: string, args: any) => {
      if (command === "selection_confirm") current = { ...current, confirmed: { card: args.input.card, revision: 1, confirmedAtMs: 1 } };
      return Promise.resolve(current);
    });
    mount();
    await user.click(await screen.findByRole("button", { name: "＋ 添加标签" }));
    await user.click(screen.getByRole("button", { name: "确认并开始选型" }));
    await screen.findByRole("button", { name: "修改需求" });
    expect(invoke.mock.calls.find(([command]) => command === "selection_confirm")![1].input.card.tasks[0].userConcerns).toEqual([]);
  });

  it("toggles business tags by name and preserves selection after reopening the confirmed card", async () => {
    const user = userEvent.setup();
    const draft = card();
    draft.tasks[0].userConcerns = ["缺项主动追问", "结构化字段输出"];
    let current: SelectionSession = { ...initial(), draft };
    invoke.mockImplementation((command: string, args: any) => {
      if (command === "selection_confirm") current = { ...current, confirmed: { card: args.input.card, revision: 1, confirmedAtMs: 1 } };
      return Promise.resolve(current);
    });
    mount();
    const choice = await screen.findByRole("checkbox", { name: "选择业务标签 缺项主动追问" });
    expect(choice).toBeChecked();
    await user.click(choice);
    expect(choice).not.toBeChecked();
    expect(choice.closest(".selection-user-concern")).not.toHaveClass("is-selected");
    await user.click(screen.getByText("缺项主动追问"));
    expect(choice).toBeChecked();
    await user.click(screen.getByText("缺项主动追问"));
    expect(choice).not.toBeChecked();
    const selected = screen.getByRole("checkbox", { name: "选择业务标签 结构化字段输出" });
    await user.click(selected);
    expect(selected).not.toBeChecked();
    await user.click(selected);
    expect(selected).toBeChecked();
    await user.click(screen.getByRole("button", { name: "确认并开始选型" }));
    await user.click(await screen.findByRole("button", { name: "修改需求" }));
    expect(screen.getByRole("checkbox", { name: "选择业务标签 缺项主动追问" })).not.toBeChecked();
    expect(screen.getByRole("checkbox", { name: "选择业务标签 结构化字段输出" })).toBeChecked();
    expect(invoke.mock.calls.find(([command]) => command === "selection_confirm")![1].input.card.tasks[0].excludedUserConcerns).toEqual(["缺项主动追问"]);
  });

  it("shows real backend progress while extraction is pending and keeps one paid request", async () => {
    const user = userEvent.setup();
    let finish: (value: SelectionSession) => void = () => undefined;
    let started = false;
    let progress = { message: "正在分析你的需求与任务划分…", startedAtMs: Date.now() - 12000, analysisPreview: "教师线下备课，需要区分意图识别和课件生成环节。", receivedChars: 28, modelStartedAtMs: Date.now() - 12000, lastOutputAtMs: Date.now() };
    invoke.mockImplementation((command: string) => {
      if (command === "selection_extract") { started = true; return new Promise((resolve) => { finish = resolve; }); }
      return Promise.resolve(started ? { ...initial(), running: true, progress } : initial());
    });
    mount();
    const input = screen.getByRole("textbox", { name: "你希望 AI 完成什么工作？" });
    await waitFor(() => expect(input).toBeEnabled());
    await user.type(input, "会议转写");
    await user.click(screen.getByRole("button", { name: "发送需求" }));
    expect(await screen.findByText(progress.analysisPreview)).toBeVisible();
    expect(screen.getByRole("status").querySelector(".selection-progress-spinner")).not.toBeNull();
    expect(screen.getByRole("status").querySelector("svg")).toBeNull();
    expect(screen.queryByText(/已收到.*字/)).not.toBeInTheDocument();
    progress = { ...progress, analysisPreview: "意图识别、课时编排和 HTML 生成需要分别核对输入输出。", receivedChars: 57, lastOutputAtMs: Date.now() };
    expect(await screen.findByText(progress.analysisPreview)).toBeVisible();
    expect(screen.queryByText(/已收到.*字/)).not.toBeInTheDocument();
    expect(screen.queryByText("正在整理需求…")).not.toBeInTheDocument();
    expect(invoke.mock.calls.filter(([command]) => command === "selection_extract")).toHaveLength(1);
    await act(async () => finish({ ...initial(), draft: card() }));
    expect(screen.queryByRole("region", { name: "处理进展" })).not.toBeInTheDocument();
    expect(input).toBeEnabled();
  });

  it("explains a quiet provider without inventing analysis or progress", async () => {
    const stamp = Date.now() - 15000;
    invoke.mockResolvedValue({ ...initial(), description: "教师备课", running: true, progress: { message: "服务已响应，等待模型输出…", startedAtMs: stamp, modelStartedAtMs: stamp, receivedChars: 0, analysisPreview: "" } });
    mount();
    expect(await screen.findByText("已连接，暂未收到分析或结果内容")).toBeVisible();
    expect(screen.queryByText(/已收到.*字/)).not.toBeInTheDocument();
    expect(invoke.mock.calls.filter(([command]) => command === "selection_extract")).toHaveLength(0);
  });

  it("prefills suggestions inline, omits unsupported numeric limits, and lets users clear a draft", async () => {
    const user = userEvent.setup();
    const draft = card();
    draft.suggestions.push({ id: "budget", taskIndex: 0, field: "cost", value: "100元", reason: "猜测", accepted: false });
    invoke.mockResolvedValue({ ...initial(), draft }); mount();
    const output = await screen.findByRole("textbox", { name: "任务 1：输出" });
    expect(output).toHaveValue("分条摘要");
    expect(screen.queryByText("待确认")).not.toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "待确认建议" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "采纳建议" })).not.toBeInTheDocument();
    expect(screen.queryByText("100元")).not.toBeInTheDocument();
    await user.clear(output);
    expect(output).toHaveValue("");
    expect(screen.queryByText("待确认")).not.toBeInTheDocument();
  });

  it("preselects the recommended priority and saves input and output as independent editable fields", async () => {
    const user = userEvent.setup();
    const draft = card();
    draft.suggestions.push(
      { id: "input", taskIndex: 0, field: "input", value: "会议录音", reason: "建议输入", accepted: false },
      { id: "goal", taskIndex: 0, field: "businessGoal", value: "将录音整理成可检索的文字", reason: "根据会议转写用途提出的建议", accepted: false },
      { id: "priority", taskIndex: 0, field: "priorities", value: "速度", reason: "希望一分钟内完成", accepted: false },
    );
    draft.tasks[0].fields.businessGoal = { value: "", source: "missing", evidence: "" };
    let current: SelectionSession = { ...initial(), draft };
    invoke.mockImplementation((command: string, args: any) => {
      if (command === "selection_confirm") current = { ...current, confirmed: { card: args.input.card, revision: 1, confirmedAtMs: 1 } };
      return Promise.resolve(current);
    });
    mount();
    const input = await screen.findByRole("textbox", { name: "任务 1：输入" });
    const output = screen.getByRole("textbox", { name: "任务 1：输出" });
    const goal = screen.getByRole("textbox", { name: "任务 1：希望 AI 完成" });
    expect(goal).toHaveValue("将录音整理成可检索的文字");
    expect(input).toHaveValue("会议录音");
    expect(output).toHaveValue("分条摘要");
    expect(screen.queryByRole("textbox", { name: "任务 1：输入输出" })).not.toBeInTheDocument();
    expect(screen.queryByText("待确认")).not.toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "速度" })).toBeChecked();
    await user.clear(output); await user.type(output, "带时间戳的文字");
    await user.clear(goal); await user.type(goal, "保留会议发言原意并整理成文字");
    expect(input).toHaveValue("会议录音");
    expect(screen.getByRole("checkbox", { name: "成本" })).toBeChecked();
    await user.click(screen.getByRole("button", { name: "确认并开始选型" }));
    const sent = invoke.mock.calls.find(([command]) => command === "selection_confirm")![1].input.card;
    expect(sent.tasks[0].fields.input.value).toBe("会议录音");
    expect(sent.tasks[0].fields.output.value).toBe("带时间戳的文字");
    expect(sent.tasks[0].fields.businessGoal.value).toBe("保留会议发言原意并整理成文字");
    expect(sent.tasks[0].fields.priorities.value).toBe("速度、成本");
    expect(sent.tasks[0].fields).not.toHaveProperty("inputOutput");
  });

  it("preselects both accuracy and speed when the AI recommends both for the user's concerns", async () => {
    const user = userEvent.setup();
    const draft = mockDraft();
    draft.tasks[0].userConcerns = ["正确理解需求", "尽快进入生成"];
    draft.suggestions.push({ id: "both", taskIndex: 0, field: "priorities", value: "准确性、速度", reason: "需要正确理解并减少等待", accepted: false });
    mount();
    const priorities = await screen.findByRole("group", { name: "任务 1 优先级" });
    expect(within(priorities).getByRole("checkbox", { name: "准确性" })).toBeChecked();
    expect(within(priorities).getByRole("checkbox", { name: "速度" })).toBeChecked();
    expect(within(priorities).getByRole("checkbox", { name: "成本" })).toBeChecked();
    await user.click(within(priorities).getByRole("checkbox", { name: "速度" }));
    expect(within(priorities).getByRole("checkbox", { name: "速度" })).not.toBeChecked();
  });

  it.each([
    ["互动课件生成、HTML 课件拼接、神奇新类别", "代码生成"],
    ["意图识别、填槽、文本生成", "文生文"],
  ])("maps %s to a fixed category %s", async (value, expected) => {
    const draft = card(); draft.tasks[0].fields.taskType.value = value;
    invoke.mockResolvedValue({ ...initial(), draft }); mount();
    expect(await screen.findByRole("checkbox", { name: `任务 1：${expected}` })).toBeChecked();
    expect(screen.getAllByRole("checkbox")).toHaveLength(12);
    expect(screen.queryByRole("checkbox", { name: /互动课件生成|HTML|神奇|填槽|意图识别|文本生成/ })).not.toBeInTheDocument();
  });

  it("selects an intent in one click and keeps the choice in chat without extra model calls", async () => {
    const user = userEvent.setup();
    const draft = card();
    const second = emptyTask(); second.name = "课时编排";
    second.fields.businessGoal = { value: "安排课程环节", source: "user", evidence: "" };
    draft.tasks.push(second);
    draft.suggestions.push({ id: "second-priority", taskIndex: 1, field: "priorities", value: "速度", reason: "推荐", accepted: false });
    let current: SelectionSession = { ...initial(), revision: 1, description: draft.description, draft };
    invoke.mockImplementation((command: string, args: any) => {
      if (command === "selection_select_task") {
        const index = draft.tasks.findIndex((task) => task.id === args.taskId);
        current = { ...current, revision: 2, intentSelections: [draft.tasks[index].name], draft: { ...draft, tasks: [draft.tasks[index]], suggestions: draft.suggestions.filter((item) => item.taskIndex === index).map((item) => ({ ...item, taskIndex: 0 })) } };
      }
      return Promise.resolve(current);
    });
    mount();
    const choice = await screen.findByRole("region", { name: "选择选型环节" });
    expect(screen.queryByRole("group", { name: "需求确认卡片" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "确认此环节" })).not.toBeInTheDocument();
    await user.click(within(choice).getByRole("radio", { name: "课时编排" }));
    expect(await screen.findByRole("textbox", { name: "选型场景" })).toHaveValue("课时编排");
    expect(screen.getByText("这次先为哪个 AI 环节选模型？")).toBeVisible();
    expect(screen.getByText("本次选择：课时编排")).toBeVisible();
    expect(screen.getByRole("textbox", { name: "任务 1：希望 AI 完成" })).toHaveValue("安排课程环节");
    expect(screen.getByRole("checkbox", { name: "速度" })).toBeChecked();
    expect(screen.queryByRole("region", { name: "选择选型环节" })).not.toBeInTheDocument();
    expect(screen.queryByRole("tablist")).not.toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith("selection_select_task", { sessionId: "session", revision: 1, taskId: second.id });
    expect(invoke.mock.calls.filter(([command]) => command === "selection_extract" || command === "research_command")).toHaveLength(0);
  });

  it("retains pending choices after a selection error", async () => {
    const user = userEvent.setup();
    const draft = card(); draft.tasks.push({ ...emptyTask(), name: "HTML 生成" });
    invoke.mockImplementation((command: string) => command === "selection_select_task" ? Promise.reject({ message: "选择暂时失败" }) : Promise.resolve({ ...initial(), draft }));
    mount();
    await user.click(await screen.findByRole("radio", { name: "HTML 生成" }));
    expect(await screen.findByText("选择暂时失败")).toBeVisible();
    expect(screen.getByRole("radio", { name: "HTML 生成" })).toBeChecked();
    expect(screen.getByRole("radio", { name: "HTML 生成" })).toBeEnabled();
    expect(screen.queryByRole("group", { name: "需求确认卡片" })).not.toBeInTheDocument();
    expect(screen.queryByText("本次选择：HTML 生成")).not.toBeInTheDocument();
    await user.click(screen.getByRole("radio", { name: "HTML 生成" }));
    expect(invoke.mock.calls.filter(([command]) => command === "selection_select_task")).toHaveLength(2);
  });

  it("restores the choice record from the session when reopening the page", async () => {
    invoke.mockResolvedValue({ ...initial(), draft: card(), intentSelections: ["会议转写"] });
    mount();
    expect(await screen.findByText("本次选择：会议转写")).toBeVisible();
    expect(screen.getByText("这次先为哪个 AI 环节选模型？")).toBeVisible();
    expect(screen.getByRole("textbox", { name: "选型场景" })).toHaveValue("会议转写");
  });

  it.each([
    ["需要使用 AI", ""],
    ["需要使用 AI；学生数据不得出境", "学生数据不得出境"],
  ])("cleans generic constraints from %s", async (value, expected) => {
    const draft = card(); draft.tasks[0].fields.constraints = { value, source: "user", evidence: "" };
    invoke.mockResolvedValue({ ...initial(), draft }); mount();
    await screen.findByRole("textbox", { name: "选型场景" });
    expect(screen.getByRole("textbox", { name: "任务 1：模型要求" })).toHaveValue(expected);
  });

  it("keeps all priorities from older cards and shows the driver beside Send", async () => {
    const draft = card();
    draft.tasks[0].fields.priorities = { value: "准确性 > 速度 > 成本", source: "provided", evidence: "准确性、速度、成本" };
    invoke.mockResolvedValue({ ...initial(), draft, invocation: { modelId: DEFAULT_ASSISTANT_MODEL, connectionName: "test", elapsedMs: 1 } });
    mount();
    const priorities = await screen.findByRole("group", { name: "任务 1 优先级" });
    expect(within(priorities).getByRole("checkbox", { name: "准确性" })).toBeChecked();
    expect(within(priorities).getByRole("checkbox", { name: "速度" })).toBeChecked();
    expect(within(priorities).getByRole("checkbox", { name: "成本" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "发送需求" }).previousElementSibling).toHaveTextContent("DeepSeek V4.1 Flash");
    expect(screen.queryByText(/整理模型/)).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "模型选型" })).not.toBeInTheDocument();
  });

  it("sends follow-up messages with reviewed edits and retains the current card on failure", async () => {
    const user = userEvent.setup();
    const draft = mockDraft();
    draft.tasks[0].fields.priorities = { value: "准确性", source: "user", evidence: "" };
    draft.usageScenario = { value: "会议后整理", source: "user", evidence: "" };
    draft.targetUser = { value: "运营", source: "user", evidence: "" };
    draft.tasks[0].userConcerns = ["结构化字段输出"];
    invoke.mockImplementation((command: string) => command === "selection_extract" ? Promise.reject({ message: "更新失败" }) : Promise.resolve({ ...initial(), revision: 2, description: draft.description, draft }));
    mount();
    await user.clear(await screen.findByRole("textbox", { name: "任务 1：希望 AI 完成" }));
    const priorities = screen.getByRole("group", { name: "任务 1 优先级" });
    expect(within(priorities).getByRole("checkbox", { name: "准确性" })).toBeChecked();
    await user.type(screen.getByRole("textbox", { name: "你希望 AI 完成什么工作？" }), "只需要处理中文");
    await user.click(screen.getByRole("button", { name: "发送需求" }));
    expect(await screen.findByText("更新失败")).toBeVisible();
    const sent = invoke.mock.calls.find(([command]) => command === "selection_extract")![1].input;
    expect(sent.description).toContain("最新补充：只需要处理中文");
    expect(sent.description).toContain("当前用户及使用场景：运营；会议后整理");
    expect(sent.description).toContain("当前额外业务标签（以此列表为准，删除项不要恢复）：结构化字段输出");
    expect(sent.description).toContain("业务优先级：准确性");
    expect(sent.description).toContain("希望 AI 完成：未提供");
    expect(screen.getByRole("textbox", { name: "任务 1：希望 AI 完成" })).toHaveValue("");
    expect(within(priorities).getByRole("checkbox", { name: "准确性" })).toBeChecked();
  });

  it("defaults only cost when no business priority was provided or recommended", async () => {
    mockDraft();
    mount();
    const priorities = await screen.findByRole("group", { name: "任务 1 优先级" });
    expect(within(priorities).getAllByRole("checkbox").filter((input) => (input as HTMLInputElement).checked).map((input) => input.getAttribute("aria-label"))).toEqual(["成本"]);
  });

  it("puts selection first, moves driver configuration to settings, and keeps drafts across tabs", async () => {
    const user = userEvent.setup();
    const draft = card();
    invoke.mockImplementation((command: string) => Promise.resolve(command === "settings_get" ? { schemaVersion: 1, activeArenaType: "text", connections, selectionAssistant: assistant } : { ...initial(), draft }));
    render(<App />);
    const navigation = screen.getByRole("navigation", { name: "主页面" });
    expect(within(navigation).getAllByRole("button").map((button) => button.textContent)).toEqual(["模型选型", "模型 Battle", "模型配置"]);
    await user.click(screen.getByRole("button", { name: "模型选型" }));
    await user.type(await screen.findByRole("textbox", { name: "业务背景" }), "只保留本次会话");
    expect(screen.queryByRole("button", { name: "助手配置" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "模型配置" }));
    const driverSettings = await screen.findByRole("region", { name: "模型选型驱动 AI" });
    expect(driverSettings).toBeVisible();
    expect(screen.getByRole("heading", { name: "模型 Battle AI" }).compareDocumentPosition(driverSettings) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "模型 Battle" }));
    await user.click(screen.getByRole("button", { name: "模型选型" }));
    expect(screen.getByRole("textbox", { name: "业务背景" })).toHaveValue("给运营使用只保留本次会话");
    expect(invoke.mock.calls.filter(([command]) => command === "selection_get")).toHaveLength(1);
    expect(invoke.mock.calls.filter(([command]) => command === "selection_extract")).toHaveLength(0);
  });

  it("opens task-specific official case citations in the system browser and hides unread sources", async () => {
    const draft = card();
    render(<SelectionResearch card={draft} busy={false} onRetry={vi.fn()} result={{
      status: "complete", revision: 1, error: "", sources: [
        { id: "case", area: "cases", title: "Vendor long page title", url: "https://docs.vendor.test/cases/intent", status: "read", checkedAtMs: 1, error: "" },
        { id: "unread", area: "cases", title: "不可使用的来源", url: "https://vendor.test/missing", status: "failed", checkedAtMs: 1, error: "" },
      ], report: { findings: [], questions: [], groups: [{ taskId: draft.tasks[0].id, gaps: [], candidates: [{ model: "model-a", role: "main", reason: "适合意图识别", evidence: [
        { sourceId: "case", quote: "Model A supports intent classification", kind: "official_case", label: "意图识别与填槽示例" },
        { sourceId: "unread", quote: "not read", kind: "official_case", label: "未核验案例" },
      ], test: "", gaps: [] }] }] },
    }} />);
    const link = screen.getByRole("link", { name: "官方使用案例：意图识别与填槽示例" });
    expect(link).toHaveAttribute("href", "https://docs.vendor.test/cases/intent");
    expect(link).toHaveAttribute("title", "Model A supports intent classification");
    expect(screen.queryByRole("link", { name: /未核验案例/ })).not.toBeInTheDocument();
    await userEvent.setup().click(link);
    expect(invoke).toHaveBeenCalledWith("selection_open_api", { url: "https://docs.vendor.test/cases/intent" });
  });

  it.each([{ suitableModels: undefined }, { suitableModels: [] }])("hides the empty suitable-model section: $suitableModels", ({ suitableModels }) => {
    const draft = card();
    render(<SelectionResearch card={draft} busy={false} onRetry={vi.fn()} result={{
      status: "complete", revision: 1, sources: [], error: "",
      report: { findings: [], questions: [], groups: [{ taskId: draft.tasks[0].id, candidates: [{ model: "model-a", origin: "overseas", reason: "任务适配理由", role: "main", evidence: [], test: "", gaps: [] }], suitableModels, gaps: [] }] },
    }} />);
    expect(screen.getByText("任务适配理由")).toBeVisible();
    expect(screen.queryByText(/已检索到的适合模型/)).not.toBeInTheDocument();
    expect(screen.queryByText("本轮未检索到其他输入输出匹配的型号。")).not.toBeInTheDocument();
  });

  it("defaults cost on new cards while preserving explicit choices and follow-up cancellations", () => {
    const draft = card();
    draft.suggestions.push({ id: "priority", taskIndex: 0, field: "priorities", value: "准确性", reason: "保留原意", accepted: false });
    expect(prefillSuggestions(draft).tasks[0].fields.priorities.value).toBe("准确性、成本");
    draft.tasks[0].fields.priorities = { value: "准确性", source: "user", evidence: "" };
    expect(prefillSuggestions(draft).tasks[0].fields.priorities.value).toBe("准确性");
    draft.tasks[0].fields.priorities = { value: "速度", source: "provided", evidence: "速度" };
    expect(prefillSuggestions(draft).tasks[0].fields.priorities.value).toBe("速度");
    draft.description += "本次只为以下一个 AI 环节选型";
    draft.tasks[0].fields.priorities = { value: "", source: "missing", evidence: "" };
    draft.suggestions = [];
    expect(prefillSuggestions(draft).tasks[0].fields.priorities.value).toBe("");
  });

  it("shows at most ten unique models in one expanded list with API links and bold literal keywords", async () => {
    const draft = card();
    const models = Array.from({ length: 8 }, (_, index) => ({ model: `m${index}`, reason: "支持 C++ 与结构化输出，适合当前输入。", keywords: ["C++", "结构化输出", "不存在的结论"], role: "candidate", apiUrl: "https://example.com/api", evidence: [], test: "", gaps: [] }));
    render(<SelectionResearch card={draft} busy={false} onRetry={vi.fn()} onSettings={vi.fn()} result={{
      status: "complete", revision: 1, asOfDate: "2026-10-22", sources: [], error: "",
      report: { findings: [], questions: [], groups: [{ taskId: draft.tasks[0].id, candidates: models, suitableModels: [models[0], ...models.map((model) => ({ ...model, model: `extra-${model.model}` }))], gaps: [] }] },
    }} />);
    expect(screen.getByRole("heading", { name: "入围模型（10 个）" })).toBeVisible();
    expect(screen.getAllByRole("article")).toHaveLength(10);
    expect(document.querySelector("details")).toBeNull();
    const row = screen.getByRole("heading", { name: "m0" }).closest("article")!;
    expect(row.querySelector("p")).toHaveTextContent(models[0].reason);
    expect(Array.from(row.querySelectorAll("strong")).map((node) => node.textContent)).toEqual(["C++", "结构化输出"]);
    const link = within(row).getByRole("link", { name: "m0 API 官网" });
    expect(link).toHaveAttribute("href", "https://example.com/api");
    expect(link).toHaveAttribute("target", "_blank");
    await userEvent.setup().click(link);
    expect(invoke).toHaveBeenCalledWith("selection_open_api", { url: "https://example.com/api" });
    invoke.mockRejectedValueOnce(new Error("浏览器未打开"));
    await userEvent.setup().click(link);
    expect(await screen.findByRole("alert")).toHaveTextContent("无法打开浏览器，请复制链接访问。");
    expect(screen.getByText("检索日期：2026-10-22").parentElement).toContainElement(screen.getByRole("button", { name: "前往模型配置" }));
  });

  it("shows AI main/fallback recommendations without manual choices and opens real settings", async () => {
    const user = userEvent.setup();
    const draft = card();
    let current: SelectionSession = { ...initial(), draft };
    invoke.mockImplementation((command: string, args: any) => {
      if (command === "settings_get") return Promise.resolve({ schemaVersion: 1, activeArenaType: "text", connections, selectionAssistant: assistant });
      if (command === "selection_confirm") current = { ...current, revision: 1, taskRuns: {}, confirmed: { card: args.input.card, revision: 1, confirmedAtMs: 1 } };
      if (command === "research_command") current = { ...current, taskRuns: { [args.taskId]: { selectedModel: null, fallbackModel: null, research: { status: "complete", revision: 1, researchedAtMs: new Date("2026-10-22T12:00:00").getTime(), sources: [
        { id: "board", area: "ranking", title: "同一份文本榜", url: "https://example.com/leaderboard", status: "read", checkedAtMs: 1, error: "" },
        { id: "docs", area: "capabilities", title: "模型能力文档", url: "https://example.com/docs", status: "read", checkedAtMs: 1, error: "" },
      ], error: "", report: { findings: [], groups: [{ taskId: args.taskId, leaderboard: { sourceId: "board", quote: "Text leaderboard" }, candidates: Array.from({ length: 6 }, (_, index) => ({ model: `model-${index}`, origin: index < 3 ? "china" : "overseas", displayName: `当前模型 ${index + 1}`, role: index === 3 ? "main" : index === 5 ? "fallback" : "candidate", mainRank: index === 3 ? 1 : index === 2 ? 2 : 0, fallbackRank: index === 5 ? 1 : index === 4 ? 2 : 0, mainStars: index === 3 ? 3 : index === 2 ? 2 : 1, fallbackStars: index === 5 ? 3 : index === 4 ? 2 : 1, reason: `任务适配理由 ${index + 1}`, apiUrl: "https://example.com/api", evidence: [{ sourceId: "docs", quote: "supports structured output" }], test: "验证任务", gaps: [] })), suitableModels: [{ model: "older-model", origin: "china", displayName: "已检索的旧版本", reason: "接受录音，输出转写文本", evidence: [{ sourceId: "docs", quote: "supports transcription" }] }], gaps: [] }], questions: [] } } } } };

      return Promise.resolve(current);
    });
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "确认并开始选型" }));
    expect(await screen.findByRole("heading", { name: "入围模型（7 个）" })).toBeVisible();
    expect(screen.queryByText("6 个候选")).not.toBeInTheDocument();
    expect(screen.getByText("检索日期：2026/10/22")).toBeVisible();
    expect(screen.getByRole("link", { name: "同一份文本榜" })).toHaveAttribute("href", "https://example.com/leaderboard");
    expect(document.querySelectorAll(".research-candidate")).toHaveLength(7);
    for (let index = 0; index < 6; index++) {
      const row = screen.getByRole("heading", { name: new RegExp(`^当前模型 ${index + 1} ${index < 3 ? "国内模型" : "海外模型"}`) }).closest("article")!;
      expect(within(row).getByText(`任务适配理由 ${index + 1}`)).toBeVisible();
      expect(within(row).getByRole("link", { name: "模型能力文档" })).toBeVisible();
    }
    expect(screen.queryByText(/已检索到的适合模型/)).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "已检索的旧版本 国内模型" })).toBeVisible();
    expect(screen.getByRole("button", { name: "前往模型配置" })).toBeEnabled();
    expect(within(screen.getByRole("region", { name: "选型调研" })).queryByRole("radio")).not.toBeInTheDocument();
    expect(screen.getByText("主力模型")).toHaveTextContent("主力模型（性价比更高）");
    expect(screen.getByText("兜底模型")).toHaveTextContent("兜底模型（质量更好）");
    expect(screen.queryByText("AI 最推荐 · 主力模型")).not.toBeInTheDocument();
    expect(screen.getAllByRole("img", { name: "主力模型推荐程度：3 星" })).toHaveLength(1);
    expect(screen.getAllByRole("img", { name: "兜底模型推荐程度：3 星" })).toHaveLength(1);
    expect(screen.getByRole("img", { name: "主力模型推荐程度：3 星" }).closest("article")).toHaveTextContent("当前模型 4");
    expect(screen.getByRole("img", { name: "兜底模型推荐程度：3 星" }).closest("article")).toHaveTextContent("当前模型 6");
    expect(screen.getByRole("img", { name: "主力模型推荐程度：3 星" }).querySelector("path")).toHaveAttribute("fill", "#000");
    const unselectedRow = screen.getByRole("heading", { name: /^当前模型 1 / }).closest("article")!;
    expect(Array.from(within(unselectedRow).getByRole("img", { name: "主力模型推荐程度：1 星" }).querySelectorAll("path")).map((path) => path.getAttribute("fill"))).toEqual(["#000", "#d5d5d5", "#d5d5d5"]);
    expect(Array.from(within(unselectedRow).getByRole("img", { name: "兜底模型推荐程度：1 星" }).querySelectorAll("path")).map((path) => path.getAttribute("fill"))).toEqual(["#000", "#d5d5d5", "#d5d5d5"]);
    expect(screen.getAllByRole("img", { name: /推荐程度：1 星$/ })).toHaveLength(8);
    const mainSecond = screen.getByRole("img", { name: "主力模型推荐程度：2 星" });
    const fallbackSecond = screen.getByRole("img", { name: "兜底模型推荐程度：2 星" });
    expect(mainSecond.closest("article")).toHaveTextContent("当前模型 3");
    expect(fallbackSecond.closest("article")).toHaveTextContent("当前模型 5");
    for (const icon of [mainSecond, fallbackSecond]) {
      expect(Array.from(icon.querySelectorAll("path")).map((path) => path.getAttribute("fill"))).toEqual(["#000", "#000", "#d5d5d5"]);
      expect(icon.querySelector("linearGradient")).toBeNull();
    }
    const title = screen.getByRole("heading", { name: "入围模型（7 个）" });
    expect(within(screen.getByRole("region", { name: "选型调研" })).getByText("业务优先级").compareDocumentPosition(title) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(screen.getByText("任务适配理由 6").compareDocumentPosition(screen.getByText("检索日期：2026/10/22")) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(screen.queryByRole("button", { name: "前往模型 Battle" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "前往模型配置" }));
    expect(await screen.findByRole("heading", { name: "模型 Battle AI" })).toBeVisible();
    await user.click(screen.getByRole("button", { name: "模型选型" }));
    expect(screen.getByRole("img", { name: "主力模型推荐程度：3 星" }).closest("article")).toHaveTextContent("当前模型 4");
    expect(screen.getByRole("img", { name: "兜底模型推荐程度：3 星" }).closest("article")).toHaveTextContent("当前模型 6");
    expect(invoke.mock.calls.filter(([command]) => command === "selection_choose")).toHaveLength(0);
    expect(invoke.mock.calls.filter(([command]) => command === "research_command").map(([, args]) => args.taskId)).toEqual([draft.tasks[0].id]);
    expect(invoke.mock.calls.find(([command]) => command === "research_command")![1].asOfDate).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  });

  it.each(["running", "complete"] as const)("keeps the full confirmed card above the next phase while research is %s", async (status) => {
    const user = userEvent.setup();
    const draft = card();
    draft.tasks[0].fields.taskDifficulty = { value: "易", source: "user", evidence: "" };
    draft.tasks[0].userConcerns = ["缺项追问"];
    invoke.mockResolvedValue({ ...initial(), running: status === "running", confirmed: { card: draft, revision: 1, confirmedAtMs: 1 }, taskRuns: { [draft.tasks[0].id]: { selectedModel: null, research: { status, revision: 1, sources: [], error: "", report: status === "complete" ? { findings: [], questions: [], groups: [{ taskId: draft.tasks[0].id, candidates: [{ model: "recommended-model", reason: "本次推荐", role: "main", evidence: [], test: "", gaps: [] }], gaps: [] }] } : null } } } });
    const originalScroll = HTMLElement.prototype.scrollIntoView;
    const scrolled: HTMLElement[] = [];
    HTMLElement.prototype.scrollIntoView = function () { scrolled.push(this); };
    try {
      mount();
      const requirements = await screen.findByRole("group", { name: "需求确认卡片" });
      expect(requirements).toBeVisible();
      expect(requirements.closest("details")).toBeNull();
      expect(within(requirements).getByRole("textbox", { name: "任务 1：希望 AI 完成" })).toHaveValue(draft.tasks[0].fields.businessGoal.value);
      expect(within(requirements).getByRole("textbox", { name: "任务 1：输入" })).toBeDisabled();
      expect(within(requirements).getByRole("checkbox", { name: "选择业务标签 缺项追问" })).toBeDisabled();
      expect(within(requirements).getByRole("radio", { name: "易" })).toBeChecked();
      expect(within(requirements).getByRole("radio", { name: "易" }).closest(".requirement-field")).toHaveClass("is-difficulty");
      expect(screen.queryByRole("button", { name: "确认并开始选型" })).not.toBeInTheDocument();
      if (status === "running") {
        const pending = screen.getByRole("region", { name: "处理进展" });
        expect(requirements.compareDocumentPosition(pending) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
        expect(scrolled[scrolled.length - 1]).toHaveClass("selection-pending");
      } else {
        const result = screen.getByRole("region", { name: "选型调研" });
        expect(requirements.compareDocumentPosition(result) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
        expect(scrolled[scrolled.length - 1]).toHaveClass("selection-confirmed");
        await user.click(screen.getByRole("button", { name: "修改需求" }));
        expect(within(requirements).getByRole("textbox", { name: "任务 1：输入" })).toBeEnabled();
        expect(scrolled[scrolled.length - 1]).toHaveClass("selection-card");
      }
    } finally { HTMLElement.prototype.scrollIntoView = originalScroll; }
  });

  it("creates a separate conversation, preserves the old report and draft, and restores them from the sidebar", async () => {
    const user = userEvent.setup();
    const draft = card();
    let current: SelectionSession = { ...initial(), description: draft.description, draft, confirmed: { card: draft, revision: 1, confirmedAtMs: 1 }, taskRuns: { [draft.tasks[0].id]: {
      selectedModel: "old-model", research: { status: "complete", revision: 1, sources: [], error: "", report: { findings: [], questions: [], groups: [{ taskId: draft.tasks[0].id, candidates: [{ model: "old-model", reason: "旧需求推荐", role: "main", evidence: [], test: "", gaps: [] }], gaps: [] }] } },
    } } };
    const oldSession = current;
    let oldView: any = null;
    const conversations = [{ id: "session", title: "会议转写", updatedAtMs: 1 }, { id: "new-session", title: "新选型", updatedAtMs: 2 }];
    invoke.mockImplementation((command: string, args: any) => {
      if (command === "selection_conversations") return Promise.resolve({ conversations: conversations.slice(0, 1), view: null });
      if (command === "selection_conversation_save") return Promise.resolve(conversations);
      if (command === "selection_conversation_open") {
        if (args.targetId === "session") { current = oldSession; return Promise.resolve({ session: current, view: oldView, conversations }); }
        oldView = args.view;
        current = { ...initial(), id: "new-session" };
        return Promise.resolve({ session: current, view: null, conversations });
      }
      return Promise.resolve(current);
    });
    mount();
    await screen.findByText("old-model");
    await user.type(screen.getByRole("textbox", { name: "你希望 AI 完成什么工作？" }), "保留这段未发送的补充");
    await user.click(screen.getByRole("button", { name: "＋ 新建选型" }));
    const input = screen.getByRole("textbox", { name: "你希望 AI 完成什么工作？" });
    await waitFor(() => expect(input).toHaveFocus());
    expect(input).toHaveValue("");
    expect(input.closest(".selection-page")).toHaveClass("is-landing");
    expect(screen.queryByText("old-model")).not.toBeInTheDocument();
    expect(screen.queryByText(/清空本轮/)).not.toBeInTheDocument();
    expect(invoke.mock.calls.filter(([command]) => ["selection_reset", "selection_extract", "research_command"].includes(command))).toHaveLength(0);
    const history = screen.getByRole("navigation", { name: "历史选型" });
    expect(within(history).getAllByRole("button")).toHaveLength(6);
    await user.click(within(history).getByRole("button", { name: "会议转写" }));
    expect(await screen.findByText("old-model")).toBeVisible();
    expect(input).toHaveValue("保留这段未发送的补充");
    expect(screen.getByRole("group", { name: "需求确认卡片" })).toBeVisible();
    expect(within(history).getByRole("button", { name: "会议转写" })).toHaveAttribute("aria-current", "page");
  });

  it("restores a locally saved editable card and input draft when the app is reopened", async () => {
    const draft = card();
    draft.tasks[0].fields.input = { value: "尚未提交的新录音要求", source: "user", evidence: "" };
    invoke.mockImplementation((command: string) => Promise.resolve(command === "selection_conversations" ? {
      conversations: [{ id: "session", title: "会议转写", updatedAtMs: 1 }],
      view: { card: draft, description: "尚未发送的文字", messages: ["原始对话"], editing: true },
    } : initial()));
    mount();
    expect(await screen.findByRole("textbox", { name: "任务 1：输入" })).toHaveValue("尚未提交的新录音要求");
    expect(screen.getByRole("textbox", { name: "你希望 AI 完成什么工作？" })).toHaveValue("尚未发送的文字");
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("selection_conversation_save", expect.objectContaining({ sessionId: "session", view: expect.objectContaining({ description: "尚未发送的文字" }) })));
  });

  it("pins, unpins and deletes conversations without triggering a conversation-open click", async () => {
    const user = userEvent.setup();
    let current = { ...initial(), draft: card() };
    let items = [{ id: "other", title: "其他选型", updatedAtMs: 2, pinned: false }, { id: "session", title: "会议转写", updatedAtMs: 1, pinned: false }];
    invoke.mockImplementation((command: string, args: any) => {
      if (command === "selection_conversations") return Promise.resolve({ view: null, conversations: items });
      if (command === "selection_conversation_save") return Promise.resolve(items);
      if (command === "selection_conversation_pin") {
        items = items.map((item) => item.id === args.targetId ? { ...item, pinned: args.pinned } : item).sort((a, b) => Number(b.pinned) - Number(a.pinned));
        return Promise.resolve({ session: current, view: args.view, conversations: items });
      }
      if (command === "selection_conversation_delete") {
        items = items.filter((item) => item.id !== args.targetId);
        current = { ...initial(), id: "other", draft: null as any };
        return Promise.resolve({ session: current, view: { card: null, description: "其他对话的草稿", messages: [], editing: true }, conversations: items });
      }
      return Promise.resolve(current);
    });
    mount();
    const pin = await screen.findByRole("button", { name: "置顶对话 会议转写" });
    await user.hover(pin.closest(".selection-conversation-row")!);
    await user.click(pin);
    expect(await screen.findByRole("button", { name: "取消置顶 会议转写" })).toHaveAttribute("aria-pressed", "true");
    const history = screen.getByRole("navigation", { name: "历史选型" });
    expect(history.firstElementChild).toHaveTextContent("会议转写");
    await user.click(screen.getByRole("button", { name: "取消置顶 会议转写" }));
    expect(await screen.findByRole("button", { name: "置顶对话 会议转写" })).toHaveAttribute("aria-pressed", "false");
    await user.click(screen.getByRole("button", { name: "删除对话 会议转写" }));
    await waitFor(() => expect(screen.queryByRole("button", { name: "删除对话 会议转写" })).not.toBeInTheDocument());
    expect(screen.getByRole("textbox", { name: "你希望 AI 完成什么工作？" })).toHaveValue("其他对话的草稿");
    expect(invoke.mock.calls.filter(([command]) => command === "selection_conversation_open")).toHaveLength(0);
  });

  it("appends five models only once and still disables the action after restoring the conversation", async () => {
    const user = userEvent.setup();
    const draft = card();
    const models = Array.from({ length: 10 }, (_, index) => ({ model: `original-${index}`, reason: "原有推荐理由", role: "candidate", mainStars: index < 3 ? 3 : 2, fallbackStars: index < 3 ? 2 : 3, evidence: [], test: "", gaps: [] }));
    let current: SelectionSession = { ...initial(), revision: 1, draft, confirmed: { card: draft, revision: 1, confirmedAtMs: 1 }, taskRuns: { [draft.tasks[0].id]: { selectedModel: "original-0", research: { status: "complete", revision: 1, sources: [], error: "", report: { findings: [], questions: [], groups: [{ taskId: draft.tasks[0].id, candidates: models, gaps: [] }] } } } } };
    invoke.mockImplementation((command: string, args: any) => {
      if (command === "research_command" && args.more) {
        const result = current.taskRuns![draft.tasks[0].id].research!;
        current = { ...current, taskRuns: { [draft.tasks[0].id]: { selectedModel: "original-0", research: { ...result, moreRequested: true, moreAddedCount: 5, report: { ...result.report!, groups: [{ taskId: draft.tasks[0].id, gaps: [], candidates: [...models, ...models.slice(0, 5).map((model, index) => ({ ...model, model: `additional-${index}`, reason: "新增推荐理由" }))] }] } } } } };
      }
      return Promise.resolve(current);
    });
    mount();
    await user.click(await screen.findByRole("button", { name: "再来 5 条" }));
    expect(await screen.findByRole("heading", { name: "入围模型（15 个）" })).toBeVisible();
    expect(screen.getAllByRole("article").filter((row) => row.classList.contains("research-candidate"))).toHaveLength(15);
    expect(screen.getByRole("button", { name: "已补充 5 条" })).toBeDisabled();
    expect(invoke.mock.calls.filter(([command]) => command === "research_command")).toHaveLength(1);
    cleanup(); mount();
    expect(await screen.findByRole("button", { name: "已补充 5 条" })).toBeDisabled();
  });

  it("allows multiple models in every star tier, always renders three slots and explains role ratings", () => {
    const draft = card();
    const models = [3, 3, 2, 2, 1, 1].map((stars, index) => ({ model: `rating-${index}`, role: "candidate", mainStars: stars, fallbackStars: stars, reason: "当前任务适配理由", mainReason: "主力成本与时延符合当前输入规模", fallbackReason: "复杂结果的校验质量适合兜底", evidence: [], test: "", gaps: [] }));
    render(<SelectionResearch card={draft} result={{ status: "complete", revision: 1, sources: [], error: "", report: { findings: [], questions: [], groups: [{ taskId: draft.tasks[0].id, candidates: models, gaps: [] }] } }} busy={false} onRetry={vi.fn()} />);
    for (const role of ["主力模型", "兜底模型"]) {
      for (const stars of [1, 2, 3]) {
        const cells = screen.getAllByRole("img", { name: `${role}推荐程度：${stars} 星` });
        expect(cells).toHaveLength(2);
        for (const cell of cells) {
          const paths = Array.from(cell.querySelectorAll("path"));
          expect(paths).toHaveLength(3);
          expect(paths.filter((path) => path.getAttribute("fill") === "#000")).toHaveLength(stars);
        }
      }
    }
    expect(screen.getAllByText("主力成本与时延符合当前输入规模")).toHaveLength(6);
    expect(screen.getAllByText("复杂结果的校验质量适合兜底")).toHaveLength(6);
  });

  it.each([false, true])("regrades a saved legacy list once, including an interrupted attempt (%s), without changing models or extra-model quota", async (attempted) => {
    const draft = card();
    const models = ["a", "b", "c", "d"].map((model, index) => ({ model, role: "candidate", mainRank: index === 0 ? 1 : index === 1 ? 2 : 0, fallbackRank: index === 2 ? 1 : index === 3 ? 2 : 0, mainStars: 0, fallbackStars: 0, reason: "原有需求适配理由", evidence: [], test: "", gaps: [] }));
    const result = { status: "complete" as const, revision: 1, sources: [], error: "", needsRatings: true, ratingAttempted: attempted, moreRequested: true, moreAddedCount: 5, report: { findings: [], questions: [], groups: [{ taskId: draft.tasks[0].id, candidates: models, gaps: [] }] } };
    let current: SelectionSession = { ...initial(), revision: 1, draft, confirmed: { card: draft, revision: 1, confirmedAtMs: 1 }, taskRuns: { [draft.tasks[0].id]: { selectedModel: "a", research: result } } };
    invoke.mockImplementation((command: string) => {
      if (command === "selection_rate") current = { ...current, taskRuns: { [draft.tasks[0].id]: { selectedModel: "a", research: { ...result, needsRatings: false, ratingAttempted: true, report: { ...result.report, groups: [{ ...result.report.groups[0], candidates: models.map((model, index) => ({ ...model, mainStars: index < 2 ? 3 : 2, fallbackStars: index < 2 ? 2 : 3, mainReason: "符合需求卡低成本结构化输出", fallbackReason: "需求卡歧义场景仍需专项校验" })) }] } } } } };
      return Promise.resolve(current);
    });
    mount();
    await waitFor(() => expect(screen.getAllByRole("img", { name: "主力模型推荐程度：3 星" })).toHaveLength(2));
    expect(screen.getAllByRole("img", { name: "兜底模型推荐程度：3 星" })).toHaveLength(2);
    expect(screen.getByRole("heading", { name: "入围模型（4 个）" })).toBeVisible();
    expect(screen.getByRole("button", { name: "已补充 5 条" })).toBeDisabled();
    expect(invoke.mock.calls.filter(([command]) => command === "selection_rate")).toHaveLength(1);
    expect(invoke.mock.calls.filter(([command]) => command === "research_command")).toHaveLength(0);
  });

  it("keeps the current card if a new conversation cannot be saved", async () => {
    const user = userEvent.setup();
    const draft = card();
    invoke.mockImplementation((command: string) => command === "selection_conversation_open" ? Promise.reject({ message: "无法保存选型对话" }) : Promise.resolve({ ...initial(), draft }));
    mount();
    await screen.findByRole("group", { name: "需求确认卡片" });
    await user.click(screen.getByRole("button", { name: "＋ 新建选型" }));
    expect(await screen.findByText("无法保存选型对话")).toBeVisible();
    expect(screen.getByRole("textbox", { name: "任务 1：希望 AI 完成" })).toHaveValue(draft.tasks[0].fields.businessGoal.value);
  });

  it("retries only the single failed use case", async () => {
    const user = userEvent.setup();
    const draft = card();
    const current: SelectionSession = { ...initial(), revision: 1, draft, confirmed: { card: draft, revision: 1, confirmedAtMs: 1 }, taskRuns: { [draft.tasks[0].id]: { selectedModel: null, research: { status: "failed", revision: 1, sources: [], report: null, error: "本次调研失败" } } } };
    invoke.mockResolvedValue(current); mount();
    expect(await screen.findByText("本次调研失败")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "重试生成" }));
    expect(invoke.mock.calls.filter(([command]) => command === "research_command").map(([, args]) => args)).toEqual([{ sessionId: current.id, revision: 1, taskId: draft.tasks[0].id, retry: true, asOfDate: expect.stringMatching(/^\d{4}-\d{2}-\d{2}$/) }]);
    expect(screen.queryByRole("tablist")).not.toBeInTheDocument();
  });
});
