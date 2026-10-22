import { useId, useState } from "react";
import { priorityLabel, RequirementCard, ResearchResult, selectionApi } from "./selection";

function RecommendationIcon({ role, rank }: { role: string; rank: number }) {
  const gradientId = useId();
  const label = rank === 1 ? `AI 第一推荐的${role}` : rank === 2 ? `AI 第二推荐的${role}` : `未推荐为${role}`;
  return <span className="research-recommendation-cell"><svg role="img" aria-label={label} viewBox="0 0 24 24" width="20" height="20">
    {rank === 2 && <defs><linearGradient id={gradientId} x1="0%" y1="0%" x2="100%" y2="0%"><stop offset="50%" stopColor="#000" /><stop offset="50%" stopColor="#d9d9d9" /></linearGradient></defs>}
    <path fill={rank === 1 ? "#000" : rank === 2 ? `url(#${gradientId})` : "#d5d5d5"} d="m12 2.5 2.93 5.94 6.56.95-4.75 4.63 1.12 6.53L12 17.47l-5.86 3.08 1.12-6.53L2.5 9.39l6.56-.95Z" />
  </svg></span>;
}

export default function SelectionResearch({ result, card, busy, onRetry, onSettings }: {
  result: ResearchResult;
  card: RequirementCard;
  busy: boolean;
  onRetry: () => void;
  onSettings?: () => void;
}) {
  const [linkError, setLinkError] = useState("");
  async function openLink(url: string) {
    setLinkError("");
    try { await selectionApi.openApi(url); }
    catch { setLinkError("无法打开浏览器，请复制链接访问。"); }
  }

  function citations(items: Array<{ sourceId: string; quote: string; kind?: string; label?: string }>) {
    return <span className="research-citations">{Array.from(new Set(items.map((item) => item.sourceId))).map((id) => {
      const source = result.sources.find((item) => item.id === id);
      const item = items.find((item) => item.sourceId === id)!;
      const kindLabel: Record<string, string> = { official_case: "官方使用案例", official_docs: "官方能力文档", benchmark: "任务评测", reference: "参考资料" };
      const topic = item.label || (source?.title !== source?.url ? source?.title.slice(0, 32) : "");
      const label = item.kind ? `${kindLabel[item.kind] || "参考资料"}${topic ? `：${topic}` : ""}` : source?.title;
      return source?.status === "read" && /^https:\/\//i.test(source.url) ? <a key={id} href={source.url} target="_blank" rel="noreferrer" title={item.quote} onClick={(event) => { event.preventDefault(); void openLink(source.url); }}>{label || new URL(source.url).hostname}</a> : null;
    })}</span>;
  }

  function originTag(origin?: "china" | "overseas") {
    return origin && <span className="research-origin-tag">{origin === "china" ? "国内模型" : "海外模型"}</span>;
  }

  function recommendationReason(text: string, keywords: string[] = []) {
    const terms = Array.from(new Set(keywords.filter((word) => word && word !== text && text.includes(word)))).sort((a, b) => b.length - a.length).slice(0, 3);
    if (!terms.length) return <p>{text}</p>;
    const pattern = new RegExp(`(${terms.map((word) => word.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("|")})`, "g");
    return <p>{text.split(pattern).map((part, index) => terms.includes(part) ? <strong key={index}>{part}</strong> : part)}</p>;
  }

  return <section className="selection-research" aria-label="选型调研">
    {linkError && <p className="selection-error" role="alert">{linkError}</p>}
    {result.error && <p className="selection-error" role="alert">{result.error}</p>}
    {result.report && <>
      {result.report.notice && <p className="selection-hint" role="status">{result.report.notice}</p>}
      {result.report.groups.map((group) => {
        // 兼容旧结果：原折叠区并入名单，保留原排序并去重。
        const candidates = [...group.candidates, ...(group.suitableModels ?? []).map((model) => ({ ...model, role: "candidate", mainRank: 0, fallbackRank: 0 }))]
          .filter((model, index, models) => models.findIndex((item) => item.model.toLowerCase() === model.model.toLowerCase()) === index).slice(0, 10);
        const main = candidates.find((model) => model.mainRank === 1) ?? candidates.find((model) => model.role === "main") ?? candidates[0];
        const fallback = candidates.find((model) => model.fallbackRank === 1 && model !== main) ?? candidates.find((model) => model.role === "fallback" && model !== main) ?? candidates.find((model) => model !== main);
        const mainSecond = candidates.find((model) => model.mainRank === 2 && model !== main) ?? (candidates.every((model) => model.mainRank === undefined) ? candidates.find((model) => model !== main) : undefined);
        const fallbackSecond = candidates.find((model) => model.fallbackRank === 2 && model !== main && model !== fallback) ?? (candidates.every((model) => model.fallbackRank === undefined) ? candidates.find((model) => model !== main && model !== fallback) : undefined);
        return <section className="research-group" key={group.taskId} aria-label={card.tasks.find((task) => task.id === group.taskId)?.name}>
          {result.report!.groups.length > 1 && <h3>{card.tasks.find((task) => task.id === group.taskId)?.name}</h3>}
          <div className="research-priority"><p><span>业务优先级</span><strong>{priorityLabel(card.tasks.find((task) => task.id === group.taskId)?.fields.priorities.value ?? "")}</strong></p>
            {group.priorityReason && <p>{group.priorityReason}</p>}
          </div>
          <div className="research-list-heading"><h2>入围模型（{candidates.length} 个）</h2><span>主力模型<span className="research-role-description">（性价比更高）</span></span><span>兜底模型<span className="research-role-description">（质量更好）</span></span><span>API 官网</span></div>
          {group.leaderboard && <p className="research-shared-board">共同榜单：{citations([group.leaderboard])}</p>}
          {candidates.map((candidate) => <article className="research-candidate" key={candidate.model}>
            <div className="research-model-summary"><h3>{candidate.displayName || candidate.model}{originTag(candidate.origin)}</h3>{recommendationReason(candidate.reason, candidate.keywords)}{citations(candidate.evidence)}</div>
            <RecommendationIcon role="主力模型" rank={candidate === main ? 1 : candidate === mainSecond ? 2 : 0} />
            <RecommendationIcon role="兜底模型" rank={candidate === fallback ? 1 : candidate === fallbackSecond ? 2 : 0} />
            <span className="research-api-link">{candidate.apiUrl && /^https:\/\//i.test(candidate.apiUrl) ? <a href={candidate.apiUrl} target="_blank" rel="noreferrer" aria-label={`${candidate.displayName || candidate.model} API 官网`} onClick={(event) => { event.preventDefault(); void openLink(candidate.apiUrl!); }}>访问官网 ↗</a> : <span aria-label="暂未取得 API 官网链接">—</span>}</span>
          </article>)}

        </section>;
      })}
    </>}
    {(!busy || result.report) && <div className="research-footer">
      {result.report && (result.asOfDate || result.researchedAtMs) && <p className="research-date">检索日期：{result.asOfDate || new Date(result.researchedAtMs!).toLocaleDateString("zh-CN")}</p>}
      {!busy && <div className="selection-actions"><button className="secondary-button" onClick={onRetry}>重新调研</button>{result.report && onSettings && <button className="primary-button" onClick={onSettings}>前往模型配置</button>}</div>}
    </div>}
  </section>;
}
