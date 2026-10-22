import { useEffect, useRef, useState } from "react";
import modelBattleLogo from "../src-tauri/icons/128x128.png";
import SelectionResearch from "./SelectionResearch";
import SelectionDriverSettings from "./SelectionDriverSettings";
import {
  AssistantSettings, ConversationSummary, ConversationView, emptyTask, FieldKey, RequirementCard,
  prefillSuggestions, priorityOptions, selectedPriorities, priorityLabel, requirementFields, selectionContextFields, selectionApi, selectionError, SelectionSession, taskTypes, userScenarioValue,
} from "./selection";

type EditableConcern = { label: string; selected: boolean };

function resizeContextField(field: HTMLTextAreaElement | null) {
  if (!field) return;
  field.style.height = "auto";
  field.style.height = `${field.scrollHeight}px`;
}

export default function ModelSelection({ hidden, assistant, onAssistantSaved, onOpenSettings }: {
  hidden: boolean;
  assistant?: AssistantSettings;
  onAssistantSaved: (settings: AssistantSettings) => void;
  onOpenSettings?: () => void;
}) {
  const [session, setSession] = useState<SelectionSession>();
  const [description, setDescription] = useState("");
  const [messages, setMessages] = useState<string[]>([]);
  const transcriptRef = useRef<HTMLDivElement>(null);
  const analysisRef = useRef<HTMLParagraphElement>(null);
  const composerRef = useRef<HTMLTextAreaElement>(null);
  const [card, setCard] = useState<RequirementCard>();
  const [newConcern, setNewConcern] = useState<{ taskId: string; value: string } | null>(null);
  const [editing, setEditing] = useState(false);
  const [intentId, setIntentId] = useState("");
  const [busy, setBusy] = useState<"loading" | "extracting" | "confirming" | "researching" | "switching" | "">("loading");
  const busyRef = useRef(false);
  const confirmationRef = useRef<{ signature: string; requestId: string } | undefined>(undefined);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [conversations, setConversations] = useState<ConversationSummary[]>([]);
  const [historyError, setHistoryError] = useState("");
  const [waitingStartedAt, setWaitingStartedAt] = useState(0);
  const [now, setNow] = useState(Date.now());
  const working = busy === "extracting" || busy === "researching" || Boolean(session?.running);
  const confirmed = Boolean(session?.confirmed) && !editing;
  const activeTask = card?.tasks[0];
  const activeRun = activeTask ? session?.taskRuns?.[activeTask.id] : undefined;
  const intents = !session?.confirmed && (session?.draft?.tasks.length ?? 0) > 1 ? session!.draft!.tasks : [];
  const sharedContext = card?.projectContext?.value ?? "";
  const modelWaitSince = session?.progress?.lastOutputAtMs ?? session?.progress?.modelStartedAtMs;
  const silentSeconds = modelWaitSince ? Math.max(0, Math.floor((now - modelWaitSince) / 1000)) : 0;
  const landing = !card && intents.length === 0 && messages.length === 0 && !session?.running;
  const connected = Boolean(assistant?.hasCredential);

  function applySession(value: SelectionSession) {
    setNewConcern(null);
    setSession(value);
    const next = value.confirmed?.card ?? (value.draft?.tasks.length === 1 ? prefillSuggestions(value.draft) : undefined);
    if (value.draft && value.draft.tasks.length > 1) setIntentId("");
    setCard(next);
    setEditing(!value.confirmed);
  }

  function restoreView(view: ConversationView | null | undefined) {
    if (!view) return;
    setCard(view.card ?? undefined); setDescription(view.description); setMessages(view.messages); setEditing(view.editing);
  }

  function currentView(): ConversationView {
    return { card: card ?? null, description, messages, editing };
  }

  async function load() {
    setBusy("loading"); setError("");
    try {
      const [value, index] = await Promise.all([selectionApi.get(), selectionApi.conversations().catch((cause) => { setHistoryError(selectionError(cause)); return null; })]);
      applySession(value);
      if (value.description) setMessages([value.description]);
      restoreView(index?.view);
      if (Array.isArray(index?.conversations)) setConversations(index.conversations);
    }
    catch (cause) { setError(selectionError(cause)); }
    finally { setBusy(""); }
  }

  useEffect(() => { void load(); }, []);
  useEffect(() => {
    if (!session || busy || working) return;
    let disposed = false;
    const timer = setTimeout(() => {
      void selectionApi.saveConversation(session.id, session.revision, { card: card ?? null, description, messages, editing })
        .then((items) => { if (!disposed) { if (Array.isArray(items)) setConversations(items); setHistoryError(""); setSession((current) => current?.storageError ? { ...current, storageError: "" } : current); } })
        .catch((cause) => { if (!disposed) setHistoryError(selectionError(cause)); });
    }, 500);
    return () => { disposed = true; clearTimeout(timer); };
  }, [session, card, description, messages, editing, busy, working]);
  useEffect(() => { if (landing && connected && !busy) composerRef.current?.focus(); }, [landing, connected, busy]);
  useEffect(() => {
    const resize = () => transcriptRef.current?.querySelectorAll<HTMLTextAreaElement>(".selection-derivation-inputs textarea").forEach(resizeContextField);
    window.addEventListener("resize", resize);
    return () => window.removeEventListener("resize", resize);
  }, []);
  useEffect(() => {
    if (analysisRef.current) analysisRef.current.scrollTop = analysisRef.current.scrollHeight;
  }, [session?.progress?.analysisPreview]);
  useEffect(() => {
    const target = working ? ".selection-pending" : confirmed ? ".selection-confirmed" : !session?.confirmed && session?.intentSelections?.length ? ".selection-intent-history:last-child" : ".selection-card";
    if (!hidden) transcriptRef.current?.querySelector(target)?.scrollIntoView?.({ block: "start" });
  }, [card?.tasks[0]?.id, hidden, working, confirmed, session?.intentSelections?.length]);
  useEffect(() => {
    if (!working) return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [working]);
  useEffect(() => {
    if (!working) return;
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const current = await selectionApi.get();
        if (disposed) return;
        if (busy !== "extracting" && busy !== "researching" && !current.running) { applySession(current); return; }
        setSession(current);
      } catch (cause) { if (!disposed) setError(selectionError(cause)); }
      if (!disposed) timer = setTimeout(() => void poll(), 500);
    };
    timer = setTimeout(() => void poll(), 300);
    return () => { disposed = true; clearTimeout(timer); };
  }, [working, busy]);

  async function extract() {
    if (!session || busyRef.current || !description.trim() || !connected) return;
    const message = description.trim();
    const reviewed = card?.tasks.map((task) => `${task.name}\n${requirementFields.map(([key, label]) => `${label}：${key === "priorities" ? priorityLabel(task.fields[key].value) : task.fields[key]?.value || "未提供"}`).join("\n")}\n当前额外业务标签（以此列表为准，删除项不要恢复）：${task.userConcerns?.map((item) => item.trim()).filter(Boolean).join("、") || "无"}\n未选中的业务标签（保留未选状态，不参与推荐）：${task.excludedUserConcerns?.join("、") || "无"}`).join("\n");
    const combined = [card?.description ?? (messages.length ? session.description : ""), card ? selectionContextFields.map(([key, label]) => `当前${label}：${(key === "usageScenario" ? userScenarioValue(card) : card[key]?.value) || "未提供"}`).join("\n") + `\n不看海外模型：${card.excludeOverseas ? "是" : "否"}` : "", reviewed ? `本次只为以下一个 AI 环节选型，历史中的其他环节仅作背景，不恢复为选型任务：\n${reviewed}` : "", card || messages.length ? `最新补充：${message}` : message].filter(Boolean).join("\n\n");
    if ([...combined].length > 20000) { setError("本轮需求及补充合计不能超过 20,000 字，请缩短后再发送。"); return; }
    busyRef.current = true; setBusy("extracting"); setError(""); setNotice("");
    setWaitingStartedAt(Date.now()); setNow(Date.now());
    setSession({ ...session, progress: null });
    setMessages((current) => [...current, message]); setDescription("");
    try {
      const value = await selectionApi.extract({ sessionId: session.id, revision: session.revision, requestId: crypto.randomUUID(), description: combined });
      applySession(value);
    } catch (cause) {
      setError(selectionError(cause)); setDescription(message);
      setMessages((current) => current.slice(0, -1));
      // 获取已消耗的请求版本，不重发可能已计费的模型调用。
      try { setSession(await selectionApi.get()); } catch { /* 原话和草稿继续保留。 */ }
    } finally { busyRef.current = false; setBusy(""); }
  }

  function manual() {
    if (busy || card) return;
    setMessages(description.trim() ? [description.trim()] : []);
    setCard(prefillSuggestions({ description, projectContext: { value: "", source: "missing", evidence: "" }, tasks: [emptyTask()], suggestions: [] })); setDescription(""); setEditing(true); setNotice("");
  }

  function changeField(taskIndex: number, key: FieldKey, value: string) {
    setCard((current) => current ? {
      ...current,
      tasks: current.tasks.map((task, index) => index === taskIndex ? {
        ...task, fields: { ...task.fields, [key]: { ...task.fields[key], value, source: value.trim() ? "user" : "missing" } },
      } : task),
      suggestions: current.suggestions.map((suggestion) => suggestion.taskIndex === taskIndex && suggestion.field === key
        ? { ...suggestion, accepted: suggestion.accepted && suggestion.value === value } : suggestion),
    } : current);
  }

  function changeConcerns(taskIndex: number, update: (values: EditableConcern[]) => EditableConcern[]) {
    setCard((current) => current ? { ...current, tasks: current.tasks.map((task, index) => {
      if (index !== taskIndex) return task;
      const values = update((task.userConcerns ?? []).map((label) => ({ label, selected: !(task.excludedUserConcerns ?? []).includes(label) })));
      return { ...task, userConcerns: values.map((item) => item.label), excludedUserConcerns: values.filter((item) => !item.selected).map((item) => item.label) };
    }) } : current);
  }

  function addConcern(taskIndex: number) {
    const label = newConcern?.value.trim();
    if (label) changeConcerns(taskIndex, (values) => values.length < 8 && !values.some((item) => item.label === label) ? [...values, { label, selected: true }] : values);
    setNewConcern(null);
  }

  async function confirm() {
    if (!session || !card || busyRef.current) return;
    busyRef.current = true; setBusy("confirming"); setError(""); setNotice("");
    const reviewedCard = { ...card, tasks: card.tasks.map((task) => {
      const userConcerns = Array.from(new Set((task.userConcerns ?? []).map((value) => value.trim()).filter((value) => value && !priorityOptions.includes(value))));
      return { ...task, userConcerns, excludedUserConcerns: Array.from(new Set((task.excludedUserConcerns ?? []).map((value) => value.trim()).filter((value) => userConcerns.includes(value)))) };
    }) };
    const signature = JSON.stringify(reviewedCard);
    if (confirmationRef.current?.signature !== signature) confirmationRef.current = { signature, requestId: crypto.randomUUID() };
    try {
      const confirmed = await selectionApi.confirm({ sessionId: session.id, revision: session.revision, requestId: confirmationRef.current.requestId, card: reviewedCard });
      applySession(confirmed);
      const taskId = confirmed.confirmed!.card.tasks[0].id;
      await runResearch(confirmed, taskId);
    } catch (cause) {
      setError(selectionError(cause));
      try { setSession(await selectionApi.get()); } catch { /* 保留当前编辑，允许再次确认。 */ }
    }
    finally { busyRef.current = false; setBusy(""); }
  }

  async function runResearch(current: SelectionSession, taskId: string, retry = false) {
    setBusy("researching"); setError(""); setWaitingStartedAt(Date.now()); setNow(Date.now());
    setSession({ ...current, progress: { message: "正在核对当前官方模型目录…", startedAtMs: Date.now() } });
    applySession(await selectionApi.research(current.id, current.revision, taskId, retry));
  }

  async function retryResearch() {
    if (!session || !activeTask || busyRef.current) return;
    busyRef.current = true;
    try { await runResearch(session, activeTask.id, true); }
    catch (cause) { setError(selectionError(cause)); }
    finally { busyRef.current = false; setBusy(""); }
  }

  async function selectIntent(taskId: string) {
    if (!session || busyRef.current) return;
    busyRef.current = true; setBusy("confirming"); setError("");
    try { applySession(await selectionApi.selectTask(session.id, session.revision, taskId)); }
    catch (cause) { setError(selectionError(cause)); }
    finally { busyRef.current = false; setBusy(""); }
  }

  async function openConversation(targetId: string | null = null) {
    if (!session || busyRef.current || working || targetId === session.id) return;
    busyRef.current = true; setBusy("switching"); setError("");
    try {
      const workspace = await selectionApi.openConversation(session.id, session.revision, currentView(), targetId);
      applySession(workspace.session);
      setDescription(""); setMessages(workspace.session.description ? [workspace.session.description] : []);
      restoreView(workspace.view);
      setConversations(workspace.conversations);
      confirmationRef.current = undefined; setNotice(""); setHistoryError("");
    } catch (cause) { setError(selectionError(cause)); }
    finally { busyRef.current = false; setBusy(""); }
  }

  return <div className="selection-workspace" hidden={hidden}>
    <aside className="selection-sidebar" aria-label="选型对话列表">
      <button className="secondary-button selection-new-conversation" disabled={Boolean(busy) || working || !session} onClick={() => void openConversation()}>＋ 新建选型</button>
      <p className="selection-sidebar-title">选型对话</p>
      <nav aria-label="历史选型">{conversations.map((item) => <button key={item.id} title={item.title} className={session?.id === item.id ? "is-active" : ""} aria-current={session?.id === item.id ? "page" : undefined} disabled={Boolean(busy) || working} onClick={() => void openConversation(item.id)}>{item.id === session?.id ? activeTask?.name || item.title : item.title}</button>)}</nav>
      {historyError && <p className="selection-history-error" role="alert">{historyError}</p>}
    </aside>
    <section className={`selection-page${landing ? " is-landing" : ""}`} aria-label="模型选型">
    {landing && <header className="selection-brand"><img src={modelBattleLogo} alt="" /><h1>Model Battle</h1></header>}
    <div className="selection-transcript" ref={transcriptRef} role="log" aria-label="选型对话" aria-live="polite" hidden={landing}>
    <div className="selection-transcript-content">
      {messages.map((message, index) => <article className="selection-user-message" key={index}><p>{message}</p></article>)}
      <div>{session?.intentSelections?.map((name, index) => <div className="selection-intent-history" key={index}>
        <div className="selection-assistant-message"><p>这次先为哪个 AI 环节选模型？</p></div>
        <article className="selection-user-message"><p>本次选择：{name}</p></article>
      </div>)}</div>
      {busy === "loading" && <p className="selection-hint" role="status">正在读取需求…</p>}
      {intents.length > 1 && <section className="selection-assistant-message selection-intent-choice" aria-label="选择选型环节">
        <h2>这次先为哪个 AI 环节选模型？</h2>
        <div className="selection-intent-options" role="radiogroup" aria-label="选择一个 AI 环节">{intents.map((task) => <label key={task.id}>
          <input type="radio" name="selection-intent" value={task.id} checked={intentId === task.id} disabled={Boolean(busy)} onChange={() => setIntentId(task.id)} onClick={() => void selectIntent(task.id)} />
          <span>{task.name}</span>
        </label>)}</div>
      </section>}
      {card && <div className={`selection-requirements${confirmed ? " is-confirmed" : ""}`} role="group" aria-label="需求确认卡片">
      <section role="region" aria-label={activeTask?.name}>
      {card.tasks.map((task, taskIndex) => {
        const selected = selectedPriorities(task.fields.priorities.value);
        return <article className="selection-card requirement-task" key={task.id} aria-label={`任务 ${taskIndex + 1} 需求确认卡片`}>
        <div className="selection-task-heading"><label><span>选型场景</span><input aria-label="选型场景" value={task.name} maxLength={200} disabled={Boolean(busy) || confirmed} onChange={(event) => setCard({ ...card, tasks: card.tasks.map((item, index) => index === taskIndex ? { ...item, name: event.target.value } : item) })} /></label>
        </div>
        <div className="selection-priority-derivation" role="group" aria-label="业务背景与用户及使用场景共同推导业务优先级">
          <div className="selection-derivation-inputs">{selectionContextFields.map(([key, label, hint]) => <div className="selection-project-context" key={key}>
            <label htmlFor={`selection-context-${key}`}>{label}</label><textarea id={`selection-context-${key}`} ref={(field) => resizeContextField(field)} rows={1} value={key === "usageScenario" ? userScenarioValue(card) : card[key]?.value ?? ""} maxLength={4000} disabled={Boolean(busy) || confirmed} placeholder={hint} onChange={(event) => setCard({ ...card, ...(key === "usageScenario" ? { targetUser: null } : {}), [key]: { value: event.target.value, source: event.target.value.trim() ? "user" : "missing", evidence: "" } })} />
          </div>)}</div>
          <div className="selection-priorities" role="group" aria-label={`任务 ${taskIndex + 1} 优先级`}>
            <strong>业务优先级</strong>
            <div className="selection-priority-options">{priorityOptions.map((option) => <label className={selected.includes(option) ? "is-selected" : ""} key={option}>
              <input type="checkbox" aria-label={option} checked={selected.includes(option)} disabled={Boolean(busy) || confirmed} onChange={(event) => changeField(taskIndex, "priorities", priorityOptions.filter((item) => item === option ? event.target.checked : selected.includes(item)).join("、"))} />{option}
            </label>)}{(task.userConcerns ?? []).map((concern, concernIndex) => <span className={`selection-user-concern${(task.excludedUserConcerns ?? []).includes(concern) ? "" : " is-selected"}`} key={concernIndex} onClick={(event) => { if (!busy && !confirmed && event.target === event.currentTarget) changeConcerns(taskIndex, (values) => values.map((item, index) => index === concernIndex ? { ...item, selected: !item.selected } : item)); }}>
              <input id={`selection-concern-${task.id}-${concernIndex}`} type="checkbox" aria-label={`选择业务标签 ${concern}`} checked={!(task.excludedUserConcerns ?? []).includes(concern)} disabled={Boolean(busy) || confirmed} onChange={(event) => changeConcerns(taskIndex, (values) => values.map((item, index) => index === concernIndex ? { ...item, selected: event.target.checked } : item))} />
              <label className="selection-concern-label" htmlFor={`selection-concern-${task.id}-${concernIndex}`}>{concern}</label>
              <button type="button" className="selection-remove-concern" aria-label={`删除业务标签 ${concern}`} disabled={Boolean(busy) || confirmed} onMouseDown={(event) => event.preventDefault()} onClick={() => changeConcerns(taskIndex, (values) => values.filter((_, index) => index !== concernIndex))}>×</button>
            </span>)}{newConcern?.taskId === task.id && <input className="selection-new-concern" aria-label="新业务标签" value={newConcern.value} maxLength={40} placeholder="输入标签后回车" autoFocus disabled={Boolean(busy) || confirmed} onChange={(event) => setNewConcern({ taskId: task.id, value: event.target.value })} onBlur={() => addConcern(taskIndex)} onKeyDown={(event) => { if (event.key === "Enter" && !event.nativeEvent.isComposing) { event.preventDefault(); event.currentTarget.blur(); } }} />}<button type="button" className="selection-add-concern" disabled={Boolean(busy) || confirmed || Boolean(newConcern) || (task.userConcerns?.length ?? 0) >= 8} onClick={() => setNewConcern({ taskId: task.id, value: "" })}>＋ 添加标签</button></div>
          </div>
        </div>
        <div className="selection-form-grid">
          {requirementFields.map(([key, label, hint]) => {
            const field = task.fields[key] ?? { value: "", source: "missing", evidence: "" };
            if (key === "priorities") return null;
            if (key === "businessContext" && (!field.value || field.value === sharedContext)) return null;
            if (["accuracy", "speed"].includes(key)) return field.value ? <div className="selection-recognized-condition" key={key}><span>{label}</span><p>{field.value}</p>{!confirmed && <button className="text-button" disabled={Boolean(busy) || confirmed} aria-label={`清除任务 ${taskIndex + 1} 的${label}要求`} onClick={() => changeField(taskIndex, key, "")}>清除</button>}</div> : null;
            if (key === "fallback" && !field.value) return null;
            return <div className={`requirement-field ${key === "taskType" ? "is-task-type" : key === "businessGoal" ? "is-business-goal" : key === "taskDifficulty" ? "is-difficulty" : key === "taskFocus" ? "is-task-focus" : ""}`} key={key}>
              {key === "taskDifficulty" ? <span className="requirement-label">{label}</span> : <label className="requirement-label" htmlFor={`selection-${task.id}-${key}`}>{label}</label>}
                {key === "taskType" ? <span className="selection-task-types">{taskTypes.map((type) => <label key={type}><input type="checkbox" aria-label={`任务 ${taskIndex + 1}：${type}`} disabled={Boolean(busy) || confirmed} checked={field.value.split("、").map((value) => value.trim()).includes(type)} onChange={(event) => {
                  const values = field.value.split("、").map((value) => value.trim()).filter(Boolean).filter((value) => value !== type);
                  changeField(taskIndex, key, [...values, ...(event.target.checked ? [type] : [])].join("、"));
                }} /><span>{type}</span></label>)}</span> : key === "taskDifficulty" ? <div className="selection-priority-options selection-difficulty-options" role="radiogroup" aria-label={`任务 ${taskIndex + 1}：${label}`}>{["易", "中", "难"].map((level) => <label key={level} className={field.value === level ? "is-selected" : ""}><input type="radio" name={`difficulty-${task.id}`} value={level} checked={field.value === level} disabled={Boolean(busy) || confirmed} onChange={() => changeField(taskIndex, key, level)} />{level}</label>)}</div> : <input id={`selection-${task.id}-${key}`} aria-label={`任务 ${taskIndex + 1}：${label}`} value={field.value.replace(/\s*\n\s*/g, "；")} maxLength={4000} disabled={Boolean(busy) || confirmed} onChange={(event) => changeField(taskIndex, key, event.target.value)} placeholder={hint} />}
            </div>;
          })}
        </div>
        <label className="selection-domestic-only"><input type="checkbox" checked={card.excludeOverseas ?? false} disabled={Boolean(busy) || confirmed} onChange={(event) => setCard({ ...card, excludeOverseas: event.target.checked })} />不看海外模型</label>
      </article>; })}
      </section>
      {!confirmed && <div className="selection-bottom-actions">
        <button className="secondary-button" type="button" disabled={Boolean(busy) || confirmed} onClick={() => void openConversation()}>重新描述</button>
        <button className="primary-button" type="button" disabled={Boolean(busy) || confirmed || card.tasks.some((task) => !task.name.trim())} onClick={() => void confirm()}>{busy === "confirming" ? "确认中…" : "确认并开始选型"}</button>
      </div>}
    </div>}
      {confirmed && card && <div className="selection-assistant-message">
        <section role="region" aria-label={`${activeTask?.name} 推荐结果`}>
          <div className="selection-confirmed"><p>{activeTask?.name}</p><button className="secondary-button" onClick={() => { setEditing(true); setNotice(""); }} disabled={Boolean(busy)}>修改需求</button></div>
          {activeRun?.research && <SelectionResearch result={activeRun.research} card={card} busy={Boolean(busy) || working} onRetry={() => void retryResearch()} onSettings={onOpenSettings} />}
          {!activeRun?.research && !busy && <button className="primary-button" onClick={() => void retryResearch()}>开始调研</button>}
        </section>
      </div>}
    {working && <section className="selection-pending" aria-label="处理进展">
      <div className="selection-progress-heading selection-hint" role="status"><span className="selection-progress-spinner" aria-hidden="true" /><span>{session?.progress?.message || "正在发送需求…"}</span><span className="selection-elapsed" aria-hidden="true">{Math.max(0, Math.floor((now - (session?.progress?.startedAtMs || waitingStartedAt || now)) / 1000))} 秒</span></div>
      <p className="selection-analysis-preview" ref={analysisRef} aria-live="off">{session?.progress?.analysisPreview || (silentSeconds >= 10 ? "已连接，暂未收到分析或结果内容" : "正在等待分析内容…")}</p>
    </section>}
    </div>
    </div>
    {error && <div className="selection-error" role="alert"><span>{error}</span>{!session && !busy && <button className="secondary-button" onClick={() => void load()}>重新加载</button>}{!card && !intents.length && session && !busy && <button className="secondary-button" onClick={manual}>手工填写</button>}</div>}
    {session?.storageError && <p className="selection-error" role="alert">{session.storageError}</p>}
    {notice && <p className="selection-notice" role="status">{notice}</p>}

    {connected ? <section className="prompt-card selection-composer" aria-label="描述需求">
        <textarea ref={composerRef} aria-label="你希望 AI 完成什么工作？" value={description} maxLength={20000} disabled={Boolean(busy) || Boolean(session?.running)} onChange={(event) => setDescription(event.target.value)} placeholder={card ? `补充或修改「${activeTask?.name ?? "当前任务"}」的需求…` : "简要描述业务背景、目标用户、使用场景，以及一个 AI 环节的工作、输入和输出；每次只为一个环节选模型。"} rows={4} onKeyDown={(event) => { if (event.key === "Enter" && (event.metaKey || event.ctrlKey) && !event.nativeEvent.isComposing) { event.preventDefault(); void extract(); } }} />
        <div className="prompt-actions"><button className="primary-button" type="button" aria-label="发送需求" disabled={Boolean(busy) || !session || Boolean(session.running) || !description.trim()} onClick={() => void extract()}>{busy === "extracting" ? "识别中…" : "发送"}</button></div>
    </section> : <SelectionDriverSettings assistant={assistant} onSaved={onAssistantSaved} inline />}
  </section></div>;
}
