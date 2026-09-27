import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke, convertFileSrc: (path: string) => `asset://${path}` }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => undefined),
}));

const emptyConfig = { schemaVersion: 1, activeArenaType: "text", connections: [] };

describe("App", () => {
  afterEach(cleanup);

  beforeEach(() => {
    invoke.mockReset();
    invoke.mockImplementation((command: string) => {
      if (command === "settings_get") return Promise.resolve(emptyConfig);
      return Promise.resolve({});
    });
  });

  it("opens the homepage without requesting credential access", async () => {
    render(<App />);

    expect(await screen.findByRole("button", { name: "模型 Battle" })).toHaveAttribute("aria-current", "page");
    expect(invoke).not.toHaveBeenCalledWith("keychain_access_prepare");
  });

  it("switches empty arena types and opens the matching settings category", async () => {
    let activeArenaType = { value: "text" };
    invoke.mockImplementation((command: string, args?: { outputType?: string }) => {
      if (command === "settings_get") {
        return Promise.resolve({ ...emptyConfig, activeArenaType: activeArenaType.value });
      }
      if (command === "arena_type_set") {
        activeArenaType.value = args?.outputType ?? "text";
        return Promise.resolve({ ...emptyConfig, activeArenaType: activeArenaType.value });
      }
      return Promise.resolve({});
    });
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "视频生成模型" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "视频生成模型" })).toHaveAttribute("aria-pressed", "true"));
    const addCard = screen.getByRole("button", { name: /添加模型连接/ });
    expect(addCard).toHaveClass("add-card");
    await user.click(addCard);

    expect(screen.getByRole("button", { name: "模型配置" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("button", { name: "视频生成模型" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByLabelText("视频生成模型模型池")).toBeInTheDocument();
  });

  it("keeps every connection path on one settings page", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: /添加模型连接/ }));

    expect(screen.getByRole("button", { name: "模型配置" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("button", { name: /AIHubMix/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /硅基流动/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /自定义单模型/ })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /AIHubMix/ }));
    expect(screen.getByRole("heading", { name: "连接 AIHubMix" })).toBeInTheDocument();
    expect(screen.getByText(/梯子必须开启“全局模式”/)).toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "模型目录筛选" })).not.toBeInTheDocument();
    expect(screen.getByText(/验证连接后，才会显示模型类型筛选/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "模型配置" })).toHaveAttribute("aria-current", "page");
    expect(screen.queryByRole("button", { name: "返回" })).not.toBeInTheDocument();
  });

  it("loads and saves multiple models from one platform key", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "settings_get") return Promise.resolve(emptyConfig);
      if (command === "provider_models") {
        return Promise.resolve({
          validationToken: "validated",
          models: [
            { modelId: "gpt-5", supportsReferenceImage: true },
            { modelId: "claude-sonnet", supportsReferenceImage: false },
          ],
        });
      }
      if (command === "connection_save") return Promise.resolve({});
      return Promise.resolve({});
    });
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: /添加模型连接/ }));
    await user.click(screen.getByRole("button", { name: /AIHubMix/ }));
    await user.type(screen.getByLabelText(/API Key/), "test-key");
    await user.click(screen.getByRole("button", { name: "验证连接并加载模型" }));
    expect(screen.getAllByText("文本生成")).toHaveLength(2);
    expect(screen.getByText("支持图片理解")).toBeInTheDocument();
    await user.click(await screen.findByLabelText("gpt-5"));
    await user.click(screen.getByLabelText("claude-sonnet"));
    await user.click(screen.getByRole("button", { name: "保存模型配置" }));

    expect(invoke).toHaveBeenCalledWith("connection_save", {
      input: expect.objectContaining({
        providerKind: "aihubmix",
        baseUrl: "https://aihubmix.com/v1",
        outputType: "text",
        supportsReferenceImage: true,
        validationToken: "validated",
        models: [
          { modelId: "gpt-5", displayName: "gpt-5", outputType: "text", supportsReferenceImage: true, enabled: true },
          { modelId: "claude-sonnet", displayName: "claude-sonnet", outputType: "text", supportsReferenceImage: false, enabled: true },
        ],
      }),
    });
  });

  it("filters text model candidates by an editable parameter limit without changing their category", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "settings_get") return Promise.resolve(emptyConfig);
      if (command === "provider_models") {
        return Promise.resolve({
          validationToken: "validated",
          models: [
            { modelId: "Qwen/Qwen2.5-7B-Instruct", outputType: "text", supportsReferenceImage: false },
            { modelId: "Qwen/Qwen2.5-14B-Instruct", outputType: "text", supportsReferenceImage: false },
            { modelId: "Qwen/Qwen3-30B-A3B", outputType: "text", supportsReferenceImage: false },
            { modelId: "mistralai/Mixtral-8x7B-Instruct", outputType: "text", supportsReferenceImage: false },
            { modelId: "deepseek-ai/DeepSeek-R1-Distill-Qwen-1.5B", outputType: "text", supportsReferenceImage: false },
            { modelId: "gpt-4o-mini", outputType: "text", supportsReferenceImage: false },
          ],
        });
      }
      if (command === "connection_save") return Promise.resolve({});
      return Promise.resolve({});
    });
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: /添加模型连接/ }));
    await user.click(screen.getByRole("button", { name: /AIHubMix/ }));
    await user.type(screen.getByLabelText(/API Key/), "test-key");
    await user.click(screen.getByRole("button", { name: "验证连接并加载模型" }));

    expect(await screen.findByLabelText("Qwen/Qwen3-30B-A3B")).toBeInTheDocument();
    expect(screen.getByLabelText("gpt-4o-mini")).toBeInTheDocument();
    expect(screen.getByText("7B")).toBeInTheDocument();
    expect(screen.getByText("14B")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "推理模型" }));
    expect(screen.getByLabelText("deepseek-ai/DeepSeek-R1-Distill-Qwen-1.5B")).toBeInTheDocument();
    expect(screen.queryByLabelText("Qwen/Qwen2.5-7B-Instruct")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "非推理模型" }));
    expect(screen.queryByLabelText("deepseek-ai/DeepSeek-R1-Distill-Qwen-1.5B")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Qwen/Qwen2.5-7B-Instruct")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "全部" }));

    await user.click(screen.getByRole("checkbox", { name: "启用参数量筛选" }));

    expect(screen.getByLabelText("Qwen/Qwen2.5-7B-Instruct")).toBeInTheDocument();
    expect(screen.getByLabelText("Qwen/Qwen2.5-14B-Instruct")).toBeInTheDocument();
    expect(screen.queryByLabelText("Qwen/Qwen3-30B-A3B")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("mistralai/Mixtral-8x7B-Instruct")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("gpt-4o-mini")).not.toBeInTheDocument();

    const parameterLimit = screen.getByRole("spinbutton", { name: "参数量上限" });
    expect(parameterLimit).toHaveValue(14);
    await user.clear(parameterLimit);
    await user.type(parameterLimit, "7");
    expect(screen.getByLabelText("Qwen/Qwen2.5-7B-Instruct")).toBeInTheDocument();
    expect(screen.queryByLabelText("Qwen/Qwen2.5-14B-Instruct")).not.toBeInTheDocument();

    await user.click(screen.getByLabelText("Qwen/Qwen2.5-7B-Instruct"));
    await user.click(screen.getByRole("button", { name: "保存模型配置" }));
    expect(invoke).toHaveBeenCalledWith("connection_save", {
      input: expect.objectContaining({
        outputType: "text",
        models: [expect.objectContaining({ modelId: "Qwen/Qwen2.5-7B-Instruct", outputType: "text" })],
      }),
    });
  });

  it("saves mixed model types selected from the all catalog", async () => {
    invoke.mockImplementation((command: string, args?: { input?: { outputType?: string } }) => {
      if (command === "settings_get") return Promise.resolve(emptyConfig);
      if (command === "provider_models") {
        const outputType = args?.input?.outputType ?? "text";
        return Promise.resolve({
          validationToken: `validated-${outputType}`,
          models: outputType === "all" ? [
            { modelId: "chat-3B", outputType: "text", supportsReferenceImage: false },
            { modelId: "image-2B", outputType: "image", supportsReferenceImage: true },
          ] : [{ modelId: "chat-3B", outputType: "text", supportsReferenceImage: false }],
        });
      }
      if (command === "connection_save") return Promise.resolve({});
      return Promise.resolve({});
    });
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: /添加模型连接/ }));
    await user.click(screen.getByRole("button", { name: /硅基流动/ }));
    await user.type(screen.getByLabelText(/API Key/), "test-key");
    await user.click(screen.getByRole("button", { name: "验证连接并加载模型" }));
    await user.click(screen.getByRole("button", { name: "全部模型" }));
    await user.click(await screen.findByLabelText("chat-3B"));
    await user.click(screen.getByLabelText("image-2B"));
    await user.click(screen.getByRole("button", { name: "保存模型配置" }));

    expect(invoke).toHaveBeenCalledWith("connection_save", {
      input: expect.objectContaining({
        outputType: "all",
        validationToken: "validated-all",
        models: [
          expect.objectContaining({ modelId: "chat-3B", outputType: "text" }),
          expect.objectContaining({ modelId: "image-2B", outputType: "image" }),
        ],
      }),
    });
  });

  it("keeps saved credentials out of the connection editor", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "settings_get") {
        return Promise.resolve({
          schemaVersion: 1,
          activeArenaType: "text",
          connections: [{
            id: "connection-1",
            displayName: "AIHubMix",
            providerKind: "aihubmix",
            baseUrl: "https://aihubmix.com/v1",
            hasCredential: true,
            models: [
              {
                id: "model-1",
                modelId: "gpt-5",
                displayName: "gpt-5",
                outputType: "text",
                supportsReferenceImage: false,
                enabled: true,
              },
              {
                id: "model-2",
                modelId: "vision-model",
                displayName: "vision-model",
                outputType: "text",
                supportsReferenceImage: true,
                enabled: false,
              },
            ],
          }],
        });
      }
      return Promise.resolve({});
    });
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "模型配置" }));
    expect(await screen.findByText("支持图片理解")).toBeInTheDocument();
    await user.click(await screen.findByRole("button", { name: "编辑AIHubMix" }));
    expect(screen.getByRole("heading", { name: "编辑连接" })).toBeInTheDocument();
    expect(screen.getByLabelText(/API Key/)).toHaveAttribute("type", "password");
    const apiKeyInput = screen.getByLabelText(/API Key/);
    expect(apiKeyInput).toHaveValue("••••••••");
    expect(apiKeyInput).toHaveAttribute("placeholder", "已保存在本软件；点击可替换");
    await user.click(apiKeyInput);
    expect(apiKeyInput).toHaveValue("");
    await user.tab();
    expect(apiKeyInput).toHaveValue("••••••••");
    expect(invoke).not.toHaveBeenCalledWith("connection_key_get", expect.anything());
    expect(screen.queryByText(/AIHubMix · 1 个模型/)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "保存模型配置" })).not.toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalledWith("model_set_enabled", expect.anything());
  });

  it("starts a replacement when adding an integrated platform again", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "settings_get") return Promise.resolve({
        schemaVersion: 1,
        activeArenaType: "text",
        connections: [{
          id: "old-aihubmix",
          displayName: "AIHubMix",
          providerKind: "aihubmix",
          baseUrl: "https://aihubmix.com/v1",
          hasCredential: true,
          models: [],
        }],
      });
      return Promise.resolve({});
    });
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: /添加模型连接/ }));
    await user.click(screen.getByRole("button", { name: /AIHubMix（梯子必须开全局模式）/ }));

    expect(screen.getByRole("heading", { name: "连接 AIHubMix" })).toBeInTheDocument();
    expect(screen.getByLabelText(/API Key/)).toBeRequired();
  });

  it("removes a model and confirms before removing its connection", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "settings_get") {
        return Promise.resolve({
          schemaVersion: 1,
          activeArenaType: "text",
          connections: [{
            id: "connection-1",
            displayName: "AIHubMix",
            providerKind: "aihubmix",
            baseUrl: "https://aihubmix.com/v1",
            hasCredential: true,
            models: [{
              id: "model-1",
              modelId: "gpt-5",
              displayName: "gpt-5",
              outputType: "text",
              supportsReferenceImage: false,
              enabled: true,
            }],
          }],
        });
      }
      return Promise.resolve(true);
    });
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "模型配置" }));

    expect((await screen.findAllByText("文本生成模型")).length).toBeGreaterThan(0);
    await user.click(screen.getByRole("button", { name: "删除gpt-5" }));
    expect(invoke).toHaveBeenCalledWith("model_remove", {
      connectionId: "connection-1",
      modelConfigId: "model-1",
    });

    await user.click(screen.getByRole("button", { name: "删除AIHubMix" }));
    expect(screen.getByRole("button", { name: "确认删除AIHubMix" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "确认删除AIHubMix" }));
    expect(invoke).toHaveBeenCalledWith("connection_remove", { connectionId: "connection-1" });
  });

  it("shows only the active image type with upload and ratio controls", async () => {
    const user = userEvent.setup();
    invoke.mockImplementation((command: string) => {
      if (command === "settings_get") {
        return Promise.resolve({
          schemaVersion: 1,
          activeArenaType: "image",
          connections: [{
            id: "connection-1",
            displayName: "AIHubMix",
            providerKind: "aihubmix",
            baseUrl: "https://aihubmix.com/v1",
            hasCredential: true,
            models: [
              { id: "text-1", modelId: "gpt-5", displayName: "gpt-5", outputType: "text", supportsReferenceImage: false, enabled: true },
              { id: "image-1", modelId: "gpt-image-1", displayName: "gpt-image-1", outputType: "image", supportsReferenceImage: true, enabled: true },
            ],
          }],
        });
      }
      return Promise.resolve({});
    });
    render(<App />);

    expect(await screen.findByRole("heading", { name: "gpt-image-1" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "gpt-5" })).not.toBeInTheDocument();
    expect(screen.getByLabelText(/添加图片/)).toBeInTheDocument();
    const ratioMenu = screen.getByLabelText("生图比例");
    expect(ratioMenu).toBeInTheDocument();
    await user.click(ratioMenu);
    await user.click(screen.getByRole("option", { name: "4:3" }));
    expect(ratioMenu).toHaveTextContent("4:3");
    await user.click(screen.getByRole("button", { name: "文本生成模型" }));
    expect(invoke).toHaveBeenCalledWith("arena_type_set", { outputType: "text" });
    await user.click(screen.getByRole("button", { name: "模型配置" }));
    expect(screen.getAllByText("图片生成模型").length).toBeGreaterThan(0);
    await user.click(screen.getByRole("switch", { name: "关闭gpt-image-1" }));
    expect(invoke).toHaveBeenCalledWith("model_set_enabled", {
      connectionId: "connection-1",
      modelConfigId: "image-1",
      enabled: false,
    });
  });

  it("pools the same model type across sources and filters sources without changing models", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "settings_get") {
        return Promise.resolve({
          schemaVersion: 1,
          activeArenaType: "text",
          connections: [
            {
              id: "aihubmix",
              displayName: "AIHubMix",
              providerKind: "aihubmix",
              baseUrl: "https://aihubmix.com/v1",
              hasCredential: true,
              models: [{ id: "gpt", modelId: "gpt-5", displayName: "gpt-5", outputType: "text", supportsReferenceImage: false, enabled: true }],
            },
            {
              id: "siliconflow",
              displayName: "硅基流动",
              providerKind: "siliconflow",
              baseUrl: "https://api.siliconflow.cn/v1",
              hasCredential: true,
              models: [{ id: "deepseek", modelId: "deepseek-v3", displayName: "deepseek-v3", outputType: "text", supportsReferenceImage: false, enabled: true }],
            },
            {
              id: "custom",
              displayName: "公司内部模型",
              providerKind: "openai_compatible",
              baseUrl: "https://models.example.com/v1",
              hasCredential: true,
              models: [{ id: "internal", modelId: "internal-chat", displayName: "internal-chat", outputType: "text", supportsReferenceImage: false, enabled: true }],
            },
          ],
        });
      }
      return Promise.resolve({});
    });
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "模型配置" }));

    expect(screen.getByText("gpt-5")).toBeInTheDocument();
    expect(screen.getByText("deepseek-v3")).toBeInTheDocument();
    expect(screen.getByText("internal-chat")).toBeInTheDocument();
    expect(screen.getAllByText("公司内部模型").length).toBeGreaterThan(0);
    expect(screen.getByRole("checkbox", { name: "筛选来源：AIHubMix" })).toBeChecked();
    expect(screen.getByRole("checkbox", { name: "筛选来源：硅基流动" })).toBeChecked();
    expect(screen.getByRole("checkbox", { name: "筛选来源：自定义单模型" })).toBeChecked();
    await user.click(screen.getByRole("checkbox", { name: "筛选来源：AIHubMix" }));
    expect(screen.queryByText("gpt-5")).not.toBeInTheDocument();
    expect(screen.getByText("deepseek-v3")).toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalledWith("model_set_enabled", expect.anything());
    await user.click(screen.getByRole("button", { name: "清空：文本生成模型" }));
    expect(screen.getByRole("dialog", { name: "确认清空模型？" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "确认清空" }));
    expect(invoke).toHaveBeenCalledWith("models_clear", {
      outputType: "text",
      providerKinds: ["siliconflow", "openai_compatible"],
    });
  });

  it("filters a platform catalog by the selected model type", async () => {
    invoke.mockImplementation((command: string, args?: { input?: { outputType?: string } }) => {
      if (command === "settings_get") return Promise.resolve(emptyConfig);
      if (command === "provider_models") {
        const outputType = args?.input?.outputType ?? "text";
        return Promise.resolve({
          validationToken: "validated",
          models: outputType === "all"
            ? [
              { modelId: "text-gen-3B", outputType: "text", supportsReferenceImage: false },
              { modelId: "image-gen-2B", outputType: "image", supportsReferenceImage: true },
              { modelId: "tts-1", outputType: "audio", supportsReferenceImage: false },
              { modelId: "whisper-1", outputType: "audio_to_text", supportsReferenceImage: false },
              { modelId: "video-gen-5B", outputType: "video", supportsReferenceImage: true },
            ]
            : outputType === "audio"
            ? [{ modelId: "tts-1", outputType: "audio", supportsReferenceImage: false }]
            : outputType === "audio_to_text"
              ? [{ modelId: "whisper-1", outputType: "audio_to_text", supportsReferenceImage: false }]
              : [{ modelId: "gpt-image-1", outputType, supportsReferenceImage: true }],
        });
      }
      return Promise.resolve({});
    });
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: /添加模型连接/ }));
    await user.click(screen.getByRole("button", { name: /AIHubMix/ }));
    expect(screen.queryByRole("group", { name: "模型目录筛选" })).not.toBeInTheDocument();
    await user.type(screen.getByLabelText(/API Key/), "test-key");
    await user.click(screen.getByRole("button", { name: "验证连接并加载模型" }));
    expect(screen.getByRole("button", { name: "全部模型" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "文本生成模型" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "文本转音频" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "音频转文本" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "视频生成模型" })).toBeEnabled();
    expect(screen.queryByRole("button", { name: "加载当前分类模型" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "全部模型" }));
    expect(invoke).toHaveBeenCalledWith("provider_models", {
      input: expect.objectContaining({ outputType: "all" }),
    });
    expect(await screen.findByText("图片生成模型 · 1 个")).toBeInTheDocument();
    expect(screen.getByText("视频生成模型 · 1 个")).toBeInTheDocument();
    expect(screen.getByText("2B")).toBeInTheDocument();
    expect(screen.getByText("5B")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "文本转音频" }));
    expect(invoke).toHaveBeenCalledWith("provider_models", {
      input: expect.objectContaining({ outputType: "audio" }),
    });
    expect(await screen.findByLabelText("tts-1")).toBeInTheDocument();
    expect(screen.queryByLabelText("whisper-1")).not.toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("button", { name: "音频转文本" })).toBeEnabled());
    await user.click(screen.getByRole("button", { name: "音频转文本" }));
    expect(invoke).toHaveBeenCalledWith("provider_models", {
      input: expect.objectContaining({ outputType: "audio_to_text" }),
    });
    expect(await screen.findByLabelText("whisper-1")).toBeInTheDocument();
    expect(screen.queryByLabelText("tts-1")).not.toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("button", { name: "视频生成模型" })).toBeEnabled());
    await user.click(screen.getByRole("button", { name: "视频生成模型" }));
    expect(invoke).toHaveBeenCalledWith("provider_models", {
      input: expect.objectContaining({ outputType: "video" }),
    });
    await waitFor(() => expect(screen.getByRole("button", { name: "图片生成模型" })).toBeEnabled());
    await user.click(screen.getByRole("button", { name: "图片生成模型" }));

    expect(invoke).toHaveBeenCalledWith("provider_models", {
      input: expect.objectContaining({ outputType: "image" }),
    });
    expect(await screen.findByLabelText("gpt-image-1")).toBeInTheDocument();
  });

  it("offers every model category for SiliconFlow and custom connections", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "settings_get") return Promise.resolve(emptyConfig);
      if (command === "provider_models") return Promise.resolve({ validationToken: "validated", models: [{ modelId: "test-model", supportsReferenceImage: false }] });
      return Promise.resolve({});
    });
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: /添加模型连接/ }));

    await user.click(screen.getByRole("button", { name: /硅基流动/ }));
    expect(screen.queryByRole("group", { name: "模型目录筛选" })).not.toBeInTheDocument();
    await user.type(screen.getByLabelText(/API Key/), "test-key");
    await user.click(screen.getByRole("button", { name: "验证连接并加载模型" }));
    expect(screen.getByRole("button", { name: "文本转音频" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "音频转文本" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "视频生成模型" })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "取消" }));

    await user.click(screen.getByRole("button", { name: /自定义单模型/ }));
    expect(screen.getByRole("button", { name: "文本生成模型" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "图片生成模型" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "音频模型" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "视频生成模型" })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "音频模型" }));
    expect(screen.getByRole("group", { name: "音频任务方向" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "音频转文本" }));
    expect(screen.getByRole("button", { name: "测试连接（转写音频）" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "视频生成模型" }));
    expect(screen.getByRole("button", { name: "验证并保存视频配置" })).toBeInTheDocument();
  });

  it("shows audio generation controls for an audio arena", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "settings_get") return Promise.resolve({
        schemaVersion: 1,
        activeArenaType: "audio",
        connections: [{
          id: "connection-1",
          displayName: "AIHubMix",
          providerKind: "aihubmix",
          baseUrl: "https://aihubmix.com/v1",
          hasCredential: true,
          models: [{ id: "audio-1", modelId: "tts-1", displayName: "tts-1", outputType: "audio", supportsReferenceImage: false, enabled: true }],
        }],
      });
      return Promise.resolve({});
    });
    const user = userEvent.setup();
    render(<App />);

    expect(await screen.findByRole("button", { name: "文本转音频" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "音频转文本" })).toHaveAttribute("aria-pressed", "false");
    expect(screen.queryByRole("button", { name: "音频模型" })).not.toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "音频任务类型" })).not.toBeInTheDocument();
    const voiceSelect = await screen.findByLabelText("生成音色");
    expect(voiceSelect).toBeInTheDocument();
    await user.click(voiceSelect);
    expect(screen.getByRole("option", { name: "alloy（中性）" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("option", { name: "shimmer（轻柔）" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "生成全部音频" })).toBeInTheDocument();
    expect(screen.queryByText("输入 --")).not.toBeInTheDocument();
    expect(screen.queryByText("输出 --")).not.toBeInTheDocument();
    expect(screen.queryByText("总计 --")).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/添加图片/)).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "模型配置" }));
    expect(screen.getAllByText("文本转音频").length).toBeGreaterThan(0);
  });

  it("accepts one audio file for an audio-to-text arena", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "settings_get") return Promise.resolve({
        schemaVersion: 1,
        activeArenaType: "audio_to_text",
        connections: [{
          id: "connection-1",
          displayName: "AIHubMix",
          providerKind: "aihubmix",
          baseUrl: "https://aihubmix.com/v1",
          hasCredential: true,
          models: [{ id: "stt-1", modelId: "whisper-1", displayName: "whisper-1", outputType: "audio_to_text", supportsReferenceImage: false, enabled: true }],
        }],
      });
      if (command === "text_run_start") return Promise.resolve({ runId: "run-1" });
      return Promise.resolve({});
    });
    const user = userEvent.setup();
    render(<App />);

    expect(await screen.findByText("添加一段音频，让所有语音识别模型同时转写")).toHaveClass("is-placeholder");
    const upload = await screen.findByLabelText(/添加音频/);
    expect(screen.getByRole("button", { name: "转写全部音频" })).toBeDisabled();
    await user.upload(upload, new File(["audio"], "sample.mpga"));
    expect(screen.getByText("sample.mpga")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "移除音频 sample.mpga" })).toBeInTheDocument();
    expect(screen.queryByText("移除音频")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "转写全部音频" }));
    expect(invoke).toHaveBeenCalledWith("text_run_start", expect.objectContaining({
      input: expect.objectContaining({ audioInput: expect.stringMatching(/^data:audio\/mpeg;base64,/) }),
    }));
  });

  it("rejects unsupported audio files before a transcription run", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "settings_get") return Promise.resolve({
        schemaVersion: 1,
        activeArenaType: "audio_to_text",
        connections: [{
          id: "connection-1",
          displayName: "AIHubMix",
          providerKind: "aihubmix",
          baseUrl: "https://aihubmix.com/v1",
          hasCredential: true,
          models: [{ id: "stt-1", modelId: "whisper-1", displayName: "whisper-1", outputType: "audio_to_text", supportsReferenceImage: false, enabled: true }],
        }],
      });
      return Promise.resolve({});
    });
    const user = userEvent.setup({ applyAccept: false });
    render(<App />);

    await user.upload(await screen.findByLabelText(/添加音频/), new File(["audio"], "sample.ogg", { type: "audio/ogg" }));

    expect(screen.getByRole("alert")).toHaveTextContent("仅支持 mp3、mp4、mpeg、mpga、m4a、wav 或 webm 音频");
    expect(screen.getByRole("button", { name: "转写全部音频" })).toBeDisabled();
    expect(invoke).not.toHaveBeenCalledWith("text_run_start", expect.anything());
  });

  it("shows video generation controls for a video arena", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "settings_get") return Promise.resolve({
        schemaVersion: 1,
        activeArenaType: "video",
        connections: [{
          id: "connection-1",
          displayName: "AIHubMix",
          providerKind: "aihubmix",
          baseUrl: "https://aihubmix.com/v1",
          hasCredential: true,
          models: [{ id: "video-1", modelId: "sora-2", displayName: "sora-2", outputType: "video", supportsReferenceImage: true, enabled: true }],
        }],
      });
      return Promise.resolve({});
    });
    render(<App />);

    expect(await screen.findByLabelText("视频生成比例")).toBeInTheDocument();
    expect(screen.getByLabelText("视频时长")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "生成全部视频" })).toBeInTheDocument();
  });

  it("runs with Enter and keeps Shift+Enter for a new line", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "settings_get") return Promise.resolve({
        schemaVersion: 1,
        activeArenaType: "text",
        connections: [{
          id: "connection-1",
          displayName: "硅基流动",
          providerKind: "siliconflow",
          baseUrl: "https://api.siliconflow.cn/v1",
          hasCredential: true,
          models: [{ id: "model-1", modelId: "deepseek", displayName: "deepseek", outputType: "text", supportsReferenceImage: false, enabled: true }],
        }],
      });
      if (command === "text_run_start") return Promise.resolve({ runId: "run-1" });
      return Promise.resolve({});
    });
    const user = userEvent.setup();
    render(<App />);

    const prompt = await screen.findByPlaceholderText(/输入同一个问题/);
    await user.type(prompt, "你好");
    await user.keyboard("{Shift>}{Enter}{/Shift}");
    expect(invoke).not.toHaveBeenCalledWith("text_run_start", expect.anything());
    await user.keyboard("{Enter}");
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("text_run_start", expect.anything()));
  });

  it("marks the system prompt as unset then set, and saves it for the current arena type", async () => {
    invoke.mockImplementation((command: string, args?: { systemPrompt?: string; outputType?: string }) => {
      if (command === "settings_get") return Promise.resolve(emptyConfig);
      if (command === "system_prompt_set") {
        const value = args?.systemPrompt?.trim();
        return Promise.resolve({
          ...emptyConfig,
          systemPrompts: value ? { [args?.outputType ?? "text"]: value } : {},
        });
      }
      return Promise.resolve({});
    });
    const user = userEvent.setup();
    render(<App />);

    const status = await screen.findByRole("button", { name: /系统提示词/ });
    expect(status.querySelector(".system-prompt-dot")).not.toHaveClass("is-set");

    await user.click(status);
    await user.type(screen.getByPlaceholderText(/你是严谨的技术编辑/), "只输出结论");
    await user.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("system_prompt_set", {
      outputType: "text",
      systemPrompt: "只输出结论",
    }));
    await waitFor(() => expect(document.querySelector(".system-prompt-dot")).toHaveClass("is-set"));
  });

  it("keeps the system prompt control on every arena type and reads its own value", async () => {
    let activeArenaType = { value: "text" };
    invoke.mockImplementation((command: string, args?: { outputType?: string }) => {
      if (command === "settings_get") {
        return Promise.resolve({ ...emptyConfig, activeArenaType: activeArenaType.value, systemPrompts: { text: "只输出结论" } });
      }
      if (command === "arena_type_set") {
        activeArenaType.value = args?.outputType ?? "text";
        return Promise.resolve({ ...emptyConfig, activeArenaType: activeArenaType.value, systemPrompts: { text: "只输出结论" } });
      }
      return Promise.resolve({});
    });
    const user = userEvent.setup();
    render(<App />);

    // 文本模式：显示已设置
    const textStatus = await screen.findByRole("button", { name: /系统提示词/ });
    expect(textStatus.querySelector(".system-prompt-dot")).toHaveClass("is-set");

    // 切到图片生成：控件仍在，且因为这类没设过而回到未设置
    await user.click(screen.getByRole("button", { name: "图片生成模型" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "图片生成模型" })).toHaveAttribute("aria-pressed", "true"));
    const imageStatus = screen.getByRole("button", { name: /系统提示词/ });
    expect(imageStatus.querySelector(".system-prompt-dot")).not.toHaveClass("is-set");

    await user.click(imageStatus);
    expect(screen.getByText(/会拼接在提示词前面/)).toBeInTheDocument();
    expect(screen.getByPlaceholderText(/你是严谨的技术编辑/)).toHaveValue("");
  });

  it("hides the source filter when only one source is configured", async () => {
    invoke.mockImplementation((command: string) => {
      if (command === "settings_get") return Promise.resolve({
        schemaVersion: 1,
        activeArenaType: "text",
        connections: [{
          id: "siliconflow",
          displayName: "硅基流动",
          providerKind: "siliconflow",
          baseUrl: "https://api.siliconflow.cn/v1",
          hasCredential: true,
          models: [{ id: "deepseek", modelId: "deepseek-v3", displayName: "deepseek-v3", outputType: "text", supportsReferenceImage: false, enabled: true }],
        }],
      });
      return Promise.resolve({});
    });
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "模型配置" }));

    expect(screen.queryByRole("group", { name: "来源筛选" })).not.toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: /筛选来源/ })).not.toBeInTheDocument();
    // 筛选行藏起来时，唯一来源的模型依旧要全部列出
    expect(screen.getByText("deepseek-v3")).toBeInTheDocument();
  });
});
