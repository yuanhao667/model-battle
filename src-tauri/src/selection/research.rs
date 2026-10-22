use super::*;
use futures_util::{StreamExt, stream};

const REQUIRED: [(&str, &str); 2] = [
    ("Arena 文本榜", "https://arena.ai/leaderboard/chat/text"),
    ("SuperCLUE", "https://www.superclueai.com/homepage"),
];
const AREAS: [&str; 4] = ["ranking", "capabilities", "cases", "cost"];

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Source {
    id: String,
    area: String,
    title: String,
    url: String,
    pub(super) status: String,
    checked_at_ms: u64,
    error: String,
    #[serde(skip)]
    text: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Evidence {
    source_id: String,
    quote: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    label: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Finding {
    area: String,
    summary: String,
    evidence: Vec<Evidence>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Candidate {
    model: String,
    #[serde(default)]
    origin: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    ranking_name: String,
    #[serde(default)]
    ranking_evidence: Option<Evidence>,
    #[serde(default = "candidate_role")]
    role: String,
    #[serde(default)]
    main_rank: u8,
    #[serde(default)]
    fallback_rank: u8,
    #[serde(default = "source_basis")]
    basis: String,
    reason: String,
    #[serde(default)]
    api_url: String,
    #[serde(default)]
    keywords: Vec<String>,
    #[serde(default)]
    evidence: Vec<Evidence>,
    #[serde(default)]
    test: String,
    #[serde(default)]
    gaps: Vec<String>,
}
fn candidate_role() -> String {
    "candidate".into()
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SuitableModel {
    model: String,
    #[serde(default)]
    origin: String,
    #[serde(default)]
    display_name: String,
    reason: String,
    #[serde(default)]
    api_url: String,
    #[serde(default)]
    keywords: Vec<String>,
    #[serde(default)]
    evidence: Vec<Evidence>,
}
fn source_basis() -> String {
    "source".into()
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Group {
    task_id: String,
    #[serde(default)]
    priority_reason: String,
    #[serde(default)]
    leaderboard: Option<Evidence>,
    #[serde(default)]
    suitable_models: Vec<SuitableModel>,
    candidates: Vec<Candidate>,
    #[serde(default)]
    gaps: Vec<String>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Report {
    #[serde(default)]
    notice: String,
    #[serde(default)]
    findings: Vec<Finding>,
    groups: Vec<Group>,
    #[serde(default)]
    questions: Vec<String>,
}
impl Report {
    pub(super) fn assign_recommendations(&mut self) {
        for group in &mut self.groups {
            let main = group
                .candidates
                .iter()
                .position(|model| model.main_rank == 1)
                .or_else(|| {
                    group
                        .candidates
                        .iter()
                        .position(|model| model.role == "main")
                })
                .unwrap_or(0);
            let fallback = group
                .candidates
                .iter()
                .enumerate()
                .find(|(index, model)| *index != main && model.fallback_rank == 1)
                .or_else(|| {
                    group
                        .candidates
                        .iter()
                        .enumerate()
                        .find(|(index, model)| *index != main && model.role == "fallback")
                })
                .map(|(index, _)| index)
                .or_else(|| (0..group.candidates.len()).find(|index| *index != main));
            let main_second = group
                .candidates
                .iter()
                .enumerate()
                .find(|(index, model)| *index != main && model.main_rank == 2)
                .map(|(index, _)| index)
                .or_else(|| (0..group.candidates.len()).find(|index| *index != main));
            let fallback_second = group
                .candidates
                .iter()
                .enumerate()
                .find(|(index, model)| {
                    *index != main && Some(*index) != fallback && model.fallback_rank == 2
                })
                .map(|(index, _)| index)
                .or_else(|| {
                    (0..group.candidates.len())
                        .find(|index| *index != main && Some(*index) != fallback)
                });
            for (index, model) in group.candidates.iter_mut().enumerate() {
                model.main_rank = if index == main {
                    1
                } else if Some(index) == main_second {
                    2
                } else {
                    0
                };
                model.fallback_rank = if Some(index) == fallback {
                    1
                } else if Some(index) == fallback_second {
                    2
                } else {
                    0
                };
                model.role = if index == main {
                    "main"
                } else if Some(index) == fallback {
                    "fallback"
                } else {
                    "candidate"
                }
                .into();
            }
        }
    }

    pub(super) fn recommended_pair(&self, task_id: &str) -> (Option<String>, Option<String>) {
        let model = |role: &str| {
            self.groups
                .iter()
                .find(|group| group.task_id == task_id)
                .and_then(|group| group.candidates.iter().find(|model| model.role == role))
                .map(|model| model.model.clone())
        };
        (model("main"), model("fallback"))
    }

    pub(super) fn has_candidate(&self, task_id: &str, model: &str) -> bool {
        self.groups.iter().any(|group| {
            group.task_id == task_id
                && group
                    .candidates
                    .iter()
                    .any(|candidate| candidate.model == model)
        })
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ResearchResult {
    pub(super) status: String,
    pub(super) revision: u64,
    pub(super) researched_at_ms: u64,
    pub(super) as_of_date: String,
    pub(super) sources: Vec<Source>,
    pub(super) report: Option<Report>,
    pub(super) error: String,
}

pub(super) fn safe_url(value: &str) -> bool {
    Url::parse(value).ok().is_some_and(|url| {
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none()
            && url.host_str().is_some_and(|host| {
                host.contains('.')
                    && host != "localhost"
                    && host.parse::<std::net::IpAddr>().is_err()
                    && !host.ends_with(".local")
            })
    })
}

fn page_text(text: String, jina: bool) -> AppResult<String> {
    if text.contains("Just a moment...")
        || text.contains("Enable JavaScript and cookies to continue")
    {
        return Err(validation(
            "来源网站要求访问验证（HTTP 403），自动读取被阻止。",
        ));
    }
    if let Some(status) = text.lines().find_map(|line| {
        line.strip_prefix("Warning: Target URL returned error ")
            .and_then(|value| value.split_whitespace().next())
            .and_then(|value| value.trim_end_matches(':').parse::<u16>().ok())
    }) {
        return Err(web::status_error("来源网站", status));
    }
    if text.trim().is_empty() || (jina && !text.contains("Markdown Content:")) {
        return Err(validation("未取得可核对的页面正文。"));
    }
    // 官网目录的导航可能比正文还长，先定位正文标题再截取，避免漏掉当前型号。
    let content = text
        .split_once("Markdown Content:")
        .map_or(text.as_str(), |(_, content)| content);
    let start = if content.starts_with("# ") {
        0
    } else {
        content
            .find("\n# ")
            .or_else(|| content.find("\n## "))
            .unwrap_or(0)
    };
    // 目录后半段也可能有小参数型号；推荐请求仍单独限制摘要长度。
    Ok(content[start..].trim().chars().take(60000).collect())
}

async fn read_source(
    web: &web::DeepSeekWeb<'_>,
    id: String,
    area: &str,
    title: &str,
    url: &str,
) -> Source {
    let mut source = Source {
        id,
        area: area.into(),
        title: title.into(),
        url: url.into(),
        status: "failed".into(),
        checked_at_ms: now_ms(),
        error: String::new(),
        text: String::new(),
    };
    if !safe_url(url) {
        source.error = "资料链接无效，本页未读取。".into();
        return source;
    }
    let primary = async { web.fetch(url).await.and_then(|text| page_text(text, false)) };
    let fallback = async {
        let response = http_client()?
            .get(format!("https://r.jina.ai/{url}"))
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(|_| validation("公开网页读取未完成。"))?;
        page_text(web::bounded_text(response, "公开网页读取服务").await?, true)
    };
    tokio::pin!(primary, fallback);
    let result = tokio::select! {
        result = &mut primary => match result {
            Ok(text) => Ok(text),
            Err(first) => fallback.await.map_err(|second| validation(&format!("{} {}", first.message, second.message))),
        },
        result = &mut fallback => match result {
            Ok(text) => Ok(text),
            Err(first) => primary.await.map_err(|second| validation(&format!("{} {}", first.message, second.message))),
        },
    };
    match result {
        Ok(text) => {
            source.status = "read".into();
            source.text = text;
        }
        Err(error) => source.error = error.message,
    }
    source
}

fn verified_evidence(evidence: &[Evidence], sources: &[Source]) -> bool {
    evidence.len() <= 6
        && evidence.iter().all(|item| {
            !item.quote.trim().is_empty()
                && item.quote.chars().count() <= 600
                && sources.iter().any(|source| {
                    source.id == item.source_id
                        && source.status == "read"
                        && (source.text.contains(&item.quote)
                            || source
                                .text
                                .split_whitespace()
                                .collect::<Vec<_>>()
                                .join(" ")
                                .contains(
                                    &item.quote.split_whitespace().collect::<Vec<_>>().join(" "),
                                ))
                })
        })
}

fn official_evidence_domain(source_url: &str, api_url: &str) -> bool {
    let domain = |value: &str| -> Option<String> {
        let url = Url::parse(value).ok()?;
        let host = url.host_str()?;
        // 公共托管域名不能仅凭同域就认定为模型厂商。
        if ["github.com", "huggingface.co", "github.io"]
            .iter()
            .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
        {
            return None;
        }
        let parts: Vec<_> = host.split('.').collect();
        let count = if parts.len() >= 3
            && parts.last()?.len() == 2
            && ["com", "co", "org", "net", "edu", "gov"].contains(&parts[parts.len() - 2])
        {
            3
        } else {
            2
        };
        Some(parts[parts.len().saturating_sub(count)..].join("."))
    };
    let source = domain(source_url);
    source.is_some() && source == domain(api_url)
}

fn discard_unverified(report: &mut Report, sources: &[Source]) {
    for finding in &mut report.findings {
        if !verified_evidence(&finding.evidence, sources) {
            finding.evidence.clear();
            finding.summary = "该项资料暂未核验，未用于推荐。".into();
        }
    }
    for group in &mut report.groups {
        group.suitable_models.retain(|model| {
            !model.evidence.is_empty()
                && verified_evidence(&model.evidence, sources)
                && verified_model_id(&model.model, &model.display_name, sources)
        });
        for candidate in &mut group.candidates {
            // 官网入口必须来自实际取得的网页或网页内链接，不能使用模型猜出的地址。
            if !safe_url(&candidate.api_url)
                || !sources.iter().any(|source| {
                    source.status == "read"
                        && (source.url == candidate.api_url
                            || source.text.contains(&candidate.api_url))
                })
            {
                candidate.api_url.clear();
            }
            candidate
                .evidence
                .retain(|item| verified_evidence(std::slice::from_ref(item), sources));
            for item in &mut candidate.evidence {
                let source = sources
                    .iter()
                    .find(|source| source.id == item.source_id)
                    .unwrap();
                if !["official_case", "official_docs", "benchmark", "reference"]
                    .contains(&item.kind.as_str())
                {
                    item.kind = "reference".into();
                }
                if item.kind.starts_with("official_")
                    && !official_evidence_domain(&source.url, &candidate.api_url)
                {
                    item.kind = "reference".into();
                }
                if item.kind == "official_case"
                    && !verified_model_id(
                        &candidate.model,
                        &candidate.display_name,
                        std::slice::from_ref(source),
                    )
                {
                    item.kind = "official_docs".into();
                }
            }
            if candidate.evidence.is_empty() {
                candidate.basis = "inference".into();
            }
            if candidate
                .ranking_evidence
                .as_ref()
                .is_some_and(|item| !verified_evidence(std::slice::from_ref(item), sources))
            {
                candidate.ranking_evidence = None;
            }
        }
        if group
            .leaderboard
            .as_ref()
            .is_some_and(|item| !verified_evidence(std::slice::from_ref(item), sources))
        {
            group.leaderboard = None;
        }
    }
}

fn verified_model_id(model: &str, display_name: &str, sources: &[Source]) -> bool {
    let normalized = |value: &str| {
        value
            .chars()
            .filter(|letter| letter.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    sources.iter().any(|source| {
        source.status == "read"
            && (source.text.contains(model)
                || source.url.ends_with(&format!("/{model}"))
                || !display_name.trim().is_empty()
                    && normalized(model) == normalized(display_name)
                    && source.text.contains(display_name))
    })
}

fn validate_report(report: &Report, card: &RequirementCard, sources: &[Source]) -> AppResult<()> {
    let invalid = || validation("推荐结果未完成核验，请重试。");
    let check_evidence = |evidence: &[Evidence]| -> AppResult<()> {
        if !verified_evidence(evidence, sources) {
            return Err(invalid());
        }
        Ok(())
    };
    if (!report.findings.is_empty()
        && (report.findings.len() != 4
            || AREAS.iter().any(|area| {
                report
                    .findings
                    .iter()
                    .filter(|item| item.area == *area)
                    .count()
                    != 1
            })))
        || report.groups.len() != card.tasks.len()
        || report.questions.len() > 2
    {
        return Err(invalid());
    }
    for finding in &report.findings {
        if finding.summary.is_empty() || finding.summary.chars().count() > 1200 {
            return Err(invalid());
        }
        check_evidence(&finding.evidence)?;
    }
    for task in &card.tasks {
        let group = report
            .groups
            .iter()
            .find(|group| group.task_id == task.id)
            .ok_or_else(invalid)?;
        if report
            .groups
            .iter()
            .filter(|group| group.task_id == task.id)
            .count()
            != 1
            || group.candidates.is_empty()
            || group.candidates.len() > 10
            || group
                .candidates
                .iter()
                .map(|candidate| &candidate.model)
                .collect::<HashSet<_>>()
                .len()
                != group.candidates.len()
            || group.gaps.len() > 8
            || group.priority_reason.chars().count() > 600
            || group.suitable_models.len() > 24
        {
            return Err(invalid());
        }
        for candidate in &group.candidates {
            if candidate.model.trim().is_empty()
                || candidate.model.len() > 200
                || candidate.display_name.chars().count() > 200
                || !["main", "fallback", "candidate"].contains(&candidate.role.as_str())
                || !["source", "inference"].contains(&candidate.basis.as_str())
                || candidate.reason.trim().is_empty()
                || candidate.reason.chars().count() > 600
                || candidate.test.chars().count() > 600
                || (candidate.basis == "source" && candidate.evidence.is_empty())
                || candidate.gaps.len() > 8
            {
                return Err(invalid());
            }
            check_evidence(&candidate.evidence)?;
        }
        let mut model_ids: HashSet<_> = group
            .candidates
            .iter()
            .map(|candidate| candidate.model.as_str())
            .collect();
        for model in &group.suitable_models {
            if model.model.trim().is_empty()
                || model.model.len() > 200
                || !model_ids.insert(&model.model)
                || model.display_name.chars().count() > 200
                || model.reason.trim().is_empty()
                || model.reason.chars().count() > 600
                || model.evidence.is_empty()
            {
                return Err(invalid());
            }
            check_evidence(&model.evidence)?;
        }
    }
    if report
        .questions
        .iter()
        .any(|question| question.chars().count() > 300)
    {
        return Err(invalid());
    }
    Ok(())
}

type Page = (String, String, String);

fn search_pages(
    sources: &mut Vec<Source>,
    searches: Vec<(&str, AppResult<Vec<web::SearchPage>>)>,
    save_sources: &impl Fn(Vec<Source>),
) -> Vec<Page> {
    let mut pages = Vec::new();
    for (area, result) in searches {
        let result = result.and_then(|items| {
            let mut found = Vec::new();
            for item in items {
                if safe_url(&item.url) && !found.iter().any(|page: &Page| page.2 == item.url) {
                    if !sources
                        .iter()
                        .any(|source| source.url == item.url && source.status == "read")
                        && !item.content.trim().is_empty()
                        && let Ok(text) = page_text(item.content, false)
                    {
                        sources.push(Source {
                            id: format!("s{}", sources.len()),
                            area: area.into(),
                            title: item.title.clone(),
                            url: item.url.clone(),
                            status: "read".into(),
                            checked_at_ms: now_ms(),
                            error: String::new(),
                            text,
                        });
                    }
                    found.push((area.to_string(), item.title, item.url));
                }
                if found.len() == 2 {
                    break;
                }
            }
            if found.is_empty() {
                Err(validation("搜索未返回可读取的公开原始页面。"))
            } else {
                Ok(found)
            }
        });
        match result {
            Ok(found) => pages.extend(
                found
                    .into_iter()
                    .filter(|page| {
                        !sources
                            .iter()
                            .any(|source| source.url == page.2 && source.status == "read")
                            && !pages.iter().any(|existing: &Page| existing.2 == page.2)
                    })
                    .collect::<Vec<_>>(),
            ),
            Err(error) => sources.push(Source {
                id: format!("s{}", sources.len()),
                area: area.into(),
                title: "DeepSeek 官网搜索".into(),
                url: String::new(),
                status: "failed".into(),
                checked_at_ms: now_ms(),
                error: error.message,
                text: String::new(),
            }),
        }
    }
    save_sources(sources.clone());
    pages
}

fn official_pages(sources: &[Source], domestic_only: bool) -> Vec<Page> {
    [
        (
            "capabilities",
            "DeepSeek 模型与价格",
            "https://api-docs.deepseek.com/quick_start/pricing",
        ),
        (
            "capabilities",
            "Claude 模型能力",
            "https://platform.claude.com/docs/en/about-claude/models/overview",
        ),
        (
            "capabilities",
            "Gemini 模型能力",
            "https://ai.google.dev/gemini-api/docs/models",
        ),
        (
            "capabilities",
            "千问当前模型目录",
            "https://help.aliyun.com/zh/model-studio/models",
        ),
        (
            "capabilities",
            "OpenAI 当前模型目录",
            "https://developers.openai.com/api/docs/models",
        ),
        (
            "capabilities",
            "智谱当前模型目录",
            "https://docs.bigmodel.cn/cn/guide/start/model-overview",
        ),
        (
            "cost",
            "Claude 官方价格",
            "https://platform.claude.com/docs/en/about-claude/pricing",
        ),
        (
            "cost",
            "Gemini 官方价格",
            "https://ai.google.dev/gemini-api/docs/pricing",
        ),
    ]
    .into_iter()
    .filter(|(_, _, url)| {
        (!domestic_only
            || url.contains("deepseek.com/")
            || url.contains("aliyun.com/")
            || url.contains("bigmodel.cn/"))
            && !sources
                .iter()
                .any(|source| source.url == *url && source.status == "read")
    })
    .map(|(area, title, url)| (area.into(), title.into(), url.into()))
    .collect()
}

async fn read_pages(
    web: &web::DeepSeekWeb<'_>,
    sources: &mut Vec<Source>,
    pages: Vec<Page>,
    save_sources: &impl Fn(Vec<Source>),
) {
    let offset = sources.len();
    let mut reads = stream::iter(pages.into_iter().enumerate().map(
        |(index, (area, title, url))| async move {
            read_source(web, format!("s{}", offset + index), &area, &title, &url).await
        },
    ))
    .buffer_unordered(8);
    while let Some(source) = reads.next().await {
        sources.push(source);
        save_sources(sources.clone());
    }
}

// 按百炼选型方法先确定模态；具体品牌和型号由本轮资料决定。
fn task_modalities(task: &RequirementTask) -> Vec<serde_json::Value> {
    let types: HashSet<_> = task.fields["taskType"].value.split('、').collect();
    [
        ("文生文", "Text", "Text", "TG"),
        ("文生图", "Text", "Image", "IG"),
        ("语音转写", "Audio", "Text", "ASR"),
        ("语音合成", "Text", "Audio", "TTS"),
        ("图片理解", "Image", "Text", "VU"),
        ("视频理解", "Video", "Text", "VU"),
        ("视频生成", "Text", "Video", "VG"),
        ("代码生成", "Text", "Text", "TG"),
    ]
    .into_iter()
    .filter(|(kind, _, _, _)| types.contains(kind))
    .map(|(kind, input, output, capability)| serde_json::json!({
        "taskType":kind,"inputModality":[input],"outputModality":[output],"requiredCapabilities":[capability]
    }))
    .collect()
}

fn research_queries(card: &RequirementCard) -> Vec<(&'static str, String)> {
    let field = |key: &str, limit: usize| {
        card.tasks
            .iter()
            .map(|task| task.fields[key].value.as_str())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join("；")
            .chars()
            .take(limit)
            .collect::<String>()
    };
    let kind = field("taskType", 35);
    let difficulty = field("taskDifficulty", 5);
    let focus = field("taskFocus", 30);
    let compare_small = kind.contains("文生文") && difficulty != "难";
    let english_kind = card
        .tasks
        .iter()
        .flat_map(|task| task.fields["taskType"].value.split('、'))
        .filter_map(|kind| match kind {
            "文生文" => Some("text generation"),
            "代码生成" => Some("code generation"),
            "文生图" => Some("text-to-image"),
            "视频生成" => Some("text-to-video"),
            "语音转写" => Some("speech-to-text"),
            "语音合成" => Some("text-to-speech"),
            "图片理解" => Some("image understanding"),
            "视频理解" => Some("video understanding"),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ");
    let work = field("businessGoal", 40);
    let io = format!("{} → {}", field("input", 20), field("output", 20));
    let features = card
        .tasks
        .iter()
        .flat_map(|task| {
            task.user_concerns
                .iter()
                .filter(|concern| !task.excluded_user_concerns.contains(concern))
        })
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("、")
        .chars()
        .take(35)
        .collect::<String>();
    let scope = if card.exclude_overseas {
        "国内"
    } else {
        "国内及海外"
    };
    vec![
        (
            "capabilities",
            format!("{scope} 最新 {kind} 模型 官方文档 输入输出 {io}"),
        ),
        (
            "cases",
            format!("{scope} {kind} 模型 官方使用案例 示例 教程 {focus} 难度{difficulty} {work}"),
        ),
        (
            "capabilities",
            if compare_small {
                format!("国内 {focus} 小参数模型 7B 8B 14B 30B A3B 非思考 官方 API")
            } else {
                format!("国内 最新 {kind} 模型 官方 API 文档 型号 {focus}")
            },
        ),
        (
            "capabilities",
            if card.exclude_overseas {
                format!("国内开源 最新 {kind} 模型 官方 API 文档")
            } else {
                format!(
                    "international {english_kind} {} models official API documentation",
                    if compare_small {
                        "small mini nano"
                    } else {
                        "latest"
                    }
                )
            },
        ),
        (
            "capabilities",
            format!(
                "{scope} {kind} 模型 功能特性 {features} {}",
                field("constraints", 35)
            ),
        ),
        (
            "cost",
            if card.exclude_overseas {
                format!("国内 {kind} 模型 官方 价格 计费 {}", field("cost", 35))
            } else {
                format!(
                    "latest international {english_kind} model official pricing {}",
                    field("cost", 35)
                )
            },
        ),
        (
            "ranking",
            format!("{scope} 最新 {kind} 模型 专项评测 对比 {work}"),
        ),
        (
            "cases",
            format!("{scope} {focus} {work} official use case cookbook {english_kind}"),
        ),
    ]
}

async fn collect(
    card: &RequirementCard,
    as_of_date: &str,
    web: &web::DeepSeekWeb<'_>,
    progress: &impl Fn(&str),
    save_sources: impl Fn(Vec<Source>),
) -> Vec<Source> {
    let mut sources = Vec::new();
    progress("正在核对当前官方模型目录、价格和榜单…");
    let mut initial = Vec::new();
    let task_type = card
        .tasks
        .iter()
        .filter_map(|task| task.fields.get("taskType"))
        .map(|field| field.value.as_str())
        .collect::<Vec<_>>()
        .join("、");
    if ["文生文", "代码生成", "图片理解", "视频理解"]
        .iter()
        .any(|kind| task_type.contains(kind))
    {
        initial = official_pages(&sources, card.exclude_overseas);
    }
    if task_type.contains("代码生成") {
        initial.push((
            "ranking".into(),
            "Arena 代码榜".into(),
            "https://arena.ai/leaderboard/chat/text/coding".into(),
        ));
    } else if task_type.contains("文生文") {
        initial.extend(
            REQUIRED
                .iter()
                .map(|(title, url)| ("ranking".into(), (*title).into(), (*url).into())),
        );
    }
    let queries = research_queries(card);
    progress("正在检索模型能力、官网案例和价格资料…");
    let searches = futures_util::future::join_all(queries.iter().map(|(area, query)| async move {
        (
            *area,
            tokio::time::timeout(
                Duration::from_secs(10),
                web.search(&format!(
                    "{as_of_date} {} {query}",
                    if card.exclude_overseas {
                        "国内模型"
                    } else {
                        ""
                    }
                )),
            )
            .await
            .unwrap_or_else(|_| Err(validation("本轮搜索较慢，使用已取得的资料继续推荐。"))),
        )
    }))
    .await;
    let pages = search_pages(&mut sources, searches, &save_sources);
    let has_board = sources
        .iter()
        .any(|source| source.area == "ranking" && source.status == "read");
    initial.retain(|page| {
        !(page.0 == "ranking" && has_board)
            && !sources
                .iter()
                .any(|source| source.url == page.2 && source.status == "read")
    });
    // 官方目录先开始读取，避免被慢搜索页面占满并发槽直到超时。
    for page in pages {
        if !initial.iter().any(|existing| existing.2 == page.2) {
            initial.push(page);
        }
    }
    let pages = initial;
    progress("正在并行核对当前型号、任务能力和价格资料…");
    // 慢站点不阻塞推荐；已完成的页面仍保留在 sources 中。
    let _ = tokio::time::timeout(
        Duration::from_secs(12),
        read_pages(web, &mut sources, pages, &save_sources),
    )
    .await;
    sources
}

fn response_string(value: &serde_json::Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| {
            value
                .get(*key)
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
        })
        .unwrap_or_default()
        .to_string()
}

fn response_evidence_item(item: &serde_json::Value) -> Option<Evidence> {
    let source_id = response_string(item, &["sourceId", "source_id"]);
    let quote = response_string(item, &["quote", "excerpt"]);
    (!source_id.is_empty() && !quote.is_empty()).then_some(Evidence {
        source_id,
        quote,
        kind: response_string(item, &["kind"]),
        label: response_string(item, &["label"])
            .chars()
            .filter(|ch| !ch.is_control())
            .take(40)
            .collect(),
    })
}

fn response_evidence(value: Option<&serde_json::Value>) -> Vec<Evidence> {
    value
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(response_evidence_item)
        .collect()
}

fn response_rank(item: &serde_json::Value, key: &str) -> u8 {
    item.get(key)
        .and_then(|rank| {
            rank.as_u64()
                .or_else(|| rank.as_str().and_then(|rank| rank.parse().ok()))
        })
        .filter(|rank| [1, 2].contains(rank))
        .unwrap_or(0) as u8
}

fn response_candidates(value: &serde_json::Value, keys: &[&str]) -> Vec<Candidate> {
    let rows = if value.is_array() {
        Some(value)
    } else {
        keys.iter()
            .find_map(|key| value.get(*key).filter(|rows| rows.is_array()))
    };
    let mut seen = HashSet::new();
    rows.and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let model = response_string(
                item,
                &[
                    "model",
                    "modelId",
                    "model_id",
                    "modelName",
                    "name",
                    "displayName",
                ],
            );
            let reason = response_string(
                item,
                &[
                    "reason",
                    "recommendationReason",
                    "recommendation_reason",
                    "fitReason",
                    "description",
                ],
            );
            // 只丢弃无法展示的条目；不让一个坏字段清空其他模型。
            if model.is_empty()
                || model.len() > 200
                || reason.is_empty()
                || reason.chars().count() > 600
                || !seen.insert(model.to_lowercase())
            {
                return None;
            }
            let keywords = item
                .get("keywords")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|keyword| {
                    !keyword.is_empty() && keyword.chars().count() <= 20 && reason.contains(keyword)
                })
                .take(3)
                .map(str::to_string)
                .collect();
            let evidence = response_evidence(item.get("evidence"));
            let origin = match response_string(item, &["origin", "region"])
                .to_lowercase()
                .as_str()
            {
                "china" | "domestic" | "国内" | "中国" | "国内模型" => "china",
                "overseas" | "international" | "海外" | "国外" | "海外模型" => "overseas",
                _ => "",
            }
            .to_string();
            let mut display_name =
                response_string(item, &["displayName", "display_name", "modelName", "name"]);
            if display_name.chars().count() > 200 {
                display_name.clear();
            }
            Some(Candidate {
                model,
                display_name,
                origin,
                reason,
                api_url: response_string(item, &["apiUrl", "api_url", "officialUrl"]),
                keywords,
                basis: if evidence.is_empty()
                    || item.get("basis").and_then(serde_json::Value::as_str) == Some("inference")
                {
                    "inference"
                } else {
                    "source"
                }
                .into(),
                evidence,
                ranking_name: response_string(item, &["rankingName"]),
                ranking_evidence: item.get("rankingEvidence").and_then(response_evidence_item),
                main_rank: response_rank(item, "mainRank"),
                fallback_rank: response_rank(item, "fallbackRank"),
                role: match (
                    response_rank(item, "mainRank"),
                    response_rank(item, "fallbackRank"),
                    response_string(item, &["role"]).as_str(),
                ) {
                    (1, _, _) => "main",
                    (_, 1, _) => "fallback",
                    (2, _, _) | (_, 2, _) => "candidate",
                    (_, _, "main" | "primary") => "main",
                    (_, _, "fallback" | "backup") => "fallback",
                    _ => "candidate",
                }
                .into(),
                test: String::new(),
                gaps: vec![],
            })
        })
        .collect()
}

pub(super) fn parse_report(text: &str, card: &RequirementCard) -> AppResult<Report> {
    let raw = text.trim().trim_start_matches('\u{feff}');
    // 兼容代码围栏和前后的说明文字，不改写模型实际给出的名称或理由。
    let value = serde_json::from_str::<serde_json::Value>(raw)
        .or_else(|original_error| {
            raw.char_indices()
                .filter(|(_, letter)| matches!(letter, '{' | '['))
                .take(256)
                .find_map(|(index, _)| {
                    serde_json::Deserializer::from_str(&raw[index..])
                        .into_iter::<serde_json::Value>()
                        .next()
                        .and_then(Result::ok)
                        .filter(|value| value.is_object() || value.is_array())
                })
                .ok_or(original_error)
        })
        .map_err(|error| {
            validation(&format!(
                "推荐内容的JSON尚未完整（第{}行，第{}列），需求已保留。",
                error.line(),
                error.column()
            ))
        })?;
    let root = value
        .get("report")
        .filter(|report| report.is_object())
        .unwrap_or(&value);
    let groups = root.get("groups").and_then(serde_json::Value::as_array);
    let mut report = Report {
        notice: String::new(),
        findings: vec![],
        groups: vec![],
        questions: vec![],
    };
    for task in &card.tasks {
        let group = groups
            .and_then(|groups| {
                groups
                    .iter()
                    .find(|group| response_string(group, &["taskId", "task_id"]) == task.id)
                    .or_else(|| (card.tasks.len() == 1 && groups.len() == 1).then(|| &groups[0]))
            })
            .or_else(|| (card.tasks.len() == 1 && groups.is_none()).then_some(root))
            .ok_or_else(|| validation("推荐内容未对应当前选型环节，需求已保留。"))?;
        let mut candidates = response_candidates(
            group,
            &["candidates", "recommendations", "models", "shortlist"],
        );
        for model in response_candidates(group, &["suitableModels", "suitable_models"]) {
            if !candidates
                .iter()
                .any(|item| item.model.eq_ignore_ascii_case(&model.model))
            {
                candidates.push(model);
            }
        }
        candidates.truncate(10);
        if candidates.is_empty() {
            return Err(validation(
                "返回内容尚未包含可显示的模型名称和推荐理由，需求已保留。",
            ));
        }
        let suitable_models = vec![];
        let leaderboard = group.get("leaderboard").and_then(response_evidence_item);
        report.groups.push(Group {
            task_id: task.id.clone(),
            priority_reason: response_string(group, &["priorityReason", "priority_reason"])
                .chars()
                .take(600)
                .collect(),
            leaderboard,
            candidates,
            suitable_models,
            gaps: vec![],
        });
    }
    Ok(report)
}

fn recommendation_sources(sources: &[Source], domestic_only: bool) -> Vec<&Source> {
    let catalogues = official_pages(&[], domestic_only);
    let mut read = sources
        .iter()
        .filter(|source| source.status == "read")
        .collect::<Vec<_>>();
    // 不让先返回的搜索资料挤掉后返回的各家官方目录。
    read.sort_by_key(|source| !catalogues.iter().any(|page| page.2 == source.url));
    read.truncate(16);
    read
}

fn recommendation_content(source: &Source, card: &RequirementCard) -> String {
    let limit = if source.area == "ranking" { 6000 } else { 4500 };
    let mut content: String = source.text.chars().take(1200).collect();
    let lower = source.text.to_ascii_lowercase();
    let mut terms = Vec::new();
    if card.tasks.iter().any(|task| {
        task.fields["taskType"].value.contains("文生文")
            && task.fields["taskDifficulty"].value != "难"
    }) {
        terms.extend(
            [
                "14b",
                "8b",
                "7b",
                "30b-a3b",
                "a3b",
                "small",
                "mini",
                "nano",
                "轻量",
                "小模型",
            ]
            .map(str::to_string),
        );
    }
    terms.extend(
        card.tasks
            .iter()
            .flat_map(|task| {
                task.fields["taskFocus"]
                    .value
                    .split(['、', '，', ',', '/', '；'])
            })
            .map(str::trim)
            .filter(|term| !term.is_empty())
            .map(|term| term.to_ascii_lowercase()),
    );
    let mut covered = vec![(0, content.len())];
    for term in terms {
        if let Some((index, _)) = lower.match_indices(&term).find(|(index, _)| {
            !covered
                .iter()
                .any(|(start, end)| index >= start && index < end)
        }) {
            let mut start = index.saturating_sub(180);
            while !source.text.is_char_boundary(start) {
                start += 1;
            }
            let excerpt: String = source.text[start..].chars().take(600).collect();
            covered.push((start, start + excerpt.len()));
            content.push_str("\n[…]\n");
            content.push_str(&excerpt);
            if content.chars().count() >= limit {
                break;
            }
        }
    }
    if content.chars().count() < limit {
        // 没命中的目录仍保留原来的顺序正文。
        content.push_str("\n[…]\n");
        content.extend(
            source
                .text
                .chars()
                .skip(1200)
                .take(limit.saturating_sub(content.chars().count())),
        );
    }
    content.chars().take(limit).collect()
}

fn missing_region(report: &Report, domestic_only: bool) -> Option<&'static str> {
    if domestic_only {
        return None;
    }
    for group in &report.groups {
        if !group
            .candidates
            .iter()
            .any(|model| model.origin == "overseas")
        {
            return Some("overseas");
        }
        if !group.candidates.iter().any(|model| model.origin == "china") {
            return Some("china");
        }
    }
    None
}

fn merge_supplement(mut existing: Report, supplement: Report) -> Report {
    for group in &mut existing.groups {
        if let Some(extra) = supplement
            .groups
            .iter()
            .find(|extra| extra.task_id == group.task_id)
        {
            let mut position = 1;
            for model in &extra.candidates {
                if !group
                    .candidates
                    .iter()
                    .any(|item| item.model.eq_ignore_ascii_case(&model.model))
                {
                    for previous in &mut group.candidates {
                        if model.main_rank > 0 && model.main_rank == previous.main_rank {
                            previous.main_rank = 0;
                        }
                        if model.fallback_rank > 0 && model.fallback_rank == previous.fallback_rank
                        {
                            previous.fallback_rank = 0;
                        }
                    }
                    if ["main", "fallback"].contains(&model.role.as_str()) {
                        for previous in &mut group.candidates {
                            if previous.role == model.role {
                                if model.role == "main" {
                                    previous.main_rank = 0;
                                }
                                if model.role == "fallback" {
                                    previous.fallback_rank = 0;
                                }
                                previous.role = "candidate".into();
                            }
                        }
                    }
                    group
                        .candidates
                        .insert(position.min(group.candidates.len()), model.clone());
                    position += 2;
                }
            }
            group.candidates.truncate(10);
        }
    }
    existing
}

pub(super) async fn run(
    card: &RequirementCard,
    as_of_date: &str,
    settings: &AssistantSettings,
    connection: &Connection,
    key: String,
    progress: impl Fn(&str),
    save_sources: impl Fn(Vec<Source>),
    stream_progress: impl Fn(&str, &str),
) -> AppResult<(Report, Vec<Source>)> {
    let web = web::DeepSeekWeb::new(&key)?;
    let sources = collect(card, as_of_date, &web, &progress, save_sources).await;
    let requirements = serde_json::json!({
        "projectContext":card.project_context.as_ref().map(|field| &field.value),
        "targetUser":card.target_user.as_ref().map(|field| &field.value),
        "usageScenario":card.usage_scenario.as_ref().map(|field| &field.value),
        "excludeOverseas":card.exclude_overseas,
        "tasks":card.tasks.iter().map(|task| serde_json::json!({"id":task.id,"name":task.name,"modalityRequirements":task_modalities(task),"userConcerns":task.user_concerns,"fields":task.fields.iter().filter(|(_, field)| !field.value.is_empty()).map(|(key, field)| (key.clone(), field.value.clone())).collect::<BTreeMap<_,_>>()})).collect::<Vec<_>>()
    });
    let mut material = serde_json::json!({"requirements":requirements,"asOfDate":as_of_date,"researchedAtMs":now_ms(),"sources":recommendation_sources(&sources, card.exclude_overseas).iter().map(|source| serde_json::json!({"id":source.id,"area":source.area,"url":source.url,"status":source.status,"content":recommendation_content(source, card)})).collect::<Vec<_>>()});
    let mut last_error = validation("入围名单未生成完整，请重试。");
    let mut retained = None;
    let mut supplement_region = None;
    for attempt in 0..3 {
        progress(if supplement_region.is_some() {
            "正在补充另一地区的适合模型…"
        } else {
            "正在按输入输出、业务需求和预算比较模型…"
        });
        let request_material = material.to_string();
        let request = assistant_request(
            settings,
            connection,
            key.clone(),
            &request_material,
            if attempt == 0 {
                include_str!("../prompts/selection_research.txt")
            } else if supplement_region.is_some() {
                "用户未限制模型地区。按missingRegion补充3个适合本任务的不同型号：china是国内，overseas是海外。只返回新增候选，不重复existingCandidates，不重做已有名单。主力侧重性价比，兜底侧重质量；apiUrl使用sources中该模型官方API文档或接入页的真实地址，不猜地址。若新增型号比已有主力或兜底更合适，role标main或fallback；否则role=candidate，不能为凑地区强改推荐。分别用mainRank、fallbackRank标注新增型号在该角色的第一推荐1、第二推荐2或未推荐0，主力和兜底分别比较。结合requirements和sources，给出模型名称、origin及35字内的一句适配理由，keywords提取理由原文中1～3个关键词，有可核验引用则附上，每项用kind标official_case、official_docs、benchmark或reference，label简述当前任务的具体用法，不能把新闻或目录冒充案例；没有则basis=inference、evidence=[]，不编造版本和事实。格式为 {\"groups\":[{\"taskId\":\"原任务ID\",\"candidates\":[{\"model\":\"实际型号或官方名称\",\"origin\":\"china或overseas\",\"reason\":\"一句适配理由\",\"basis\":\"inference\",\"evidence\":[]}]}]}。"
            } else {
                "你是JSON格式修复器。只整理previousResult中已经给出的模型与理由，不重新选型、不搜索、不分析、不编造补充型号。返回 {\"groups\":[{\"taskId\":\"给定任务ID\",\"candidates\":[{\"model\":\"原模型名称或ID\",\"origin\":\"china或overseas\",\"reason\":\"原推荐理由\",\"basis\":\"inference\",\"evidence\":[]}],\"suitableModels\":[]}]}。保留原有apiUrl、keywords、mainRank、fallbackRank、evidence的kind及label和明确的地区限制。"
            },
            "调研结论",
            &progress,
            &stream_progress,
        );
        let generated = if supplement_region.is_some() {
            tokio::time::timeout(Duration::from_secs(20), request)
                .await
                .unwrap_or_else(|_| Err(validation("补充推荐暂未完成，保留已有名单。")))
        } else {
            request.await
        };
        let (output, _) = match generated {
            Ok(result) => result,
            Err(error) => return retained.map(|report| (report, sources)).ok_or(error),
        };
        let parsed = parse_report(&output.text, card);
        let checked = parsed.and_then(|mut report| {
            if card.exclude_overseas {
                for group in &mut report.groups {
                    group.candidates.retain(|model| model.origin == "china");
                    group
                        .suitable_models
                        .retain(|model| model.origin == "china");
                }
            }
            discard_unverified(&mut report, &sources);
            validate_report(&report, card, &sources)?;
            Ok(report)
        });
        match checked {
            Ok(report) => {
                if let Some(existing) = retained.take() {
                    return Ok((merge_supplement(existing, report), sources));
                }
                if supplement_region.is_none()
                    && let Some(region) = missing_region(&report, card.exclude_overseas)
                {
                    material["existingCandidates"] =
                        serde_json::to_value(&report.groups).unwrap_or_default();
                    material["missingRegion"] = serde_json::json!(region);
                    retained = Some(report);
                    supplement_region = Some(region);
                    continue;
                }
                return Ok((report, sources));
            }
            Err(error) => {
                last_error = error;
                if attempt == 0 {
                    material = serde_json::json!({"previousResult":output.text,"taskIds":card.tasks.iter().map(|task| &task.id).collect::<Vec<_>>(),"excludeOverseas":card.exclude_overseas,"correction":last_error.message});
                    progress("正在整理模型已返回的推荐内容…");
                } else {
                    return retained.map(|report| (report, sources)).ok_or(last_error);
                }
            }
        }
    }
    retained.map(|report| (report, sources)).ok_or(last_error)
}

pub(super) fn status(report: &Report, sources: &[Source]) -> String {
    if sources.iter().any(|source| source.status != "read")
        || report.findings.iter().any(|item| item.evidence.is_empty())
        || report.groups.iter().any(|group| {
            group.candidates.is_empty()
                || !group.gaps.is_empty()
                || group
                    .candidates
                    .iter()
                    .any(|candidate| candidate.basis == "inference")
        })
    {
        "partial".into()
    } else {
        "complete".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_real_official_cases_but_never_relabels_third_party_news_as_official() {
        let card = super::super::tests::card();
        let mut report = parse_report(&serde_json::json!({"candidates":[{"model":"MiniModel-14B", "reason":"适合意图分类", "apiUrl":"https://platform.vendor.test/api", "evidence":[
            {"sourceId":"case", "quote":"MiniModel-14B supports intent classification", "kind":"official_case", "label":"意图分类示例"},
            {"sourceId":"news", "quote":"MiniModel-14B supports intent classification", "kind":"official_case", "label":"媒体介绍"},
            {"sourceId":"case", "quote":"invented official customer case", "kind":"official_case", "label":"虚构案例"}
        ]}]}).to_string(), &card).unwrap();
        let source = |id: &str, url: &str| {
            Source { id: id.into(), area: "cases".into(), title: "case".into(), url: url.into(), status: "read".into(), checked_at_ms: 1, error: String::new(), text: "MiniModel-14B supports intent classification. API: https://platform.vendor.test/api".into() }
        };
        discard_unverified(
            &mut report,
            &[
                source("case", "https://docs.vendor.test/cases/intent"),
                source("news", "https://media.other.test/news"),
            ],
        );
        let evidence = &report.groups[0].candidates[0].evidence;
        assert_eq!(evidence.len(), 2);
        assert_eq!(evidence[0].kind, "official_case");
        assert_eq!(evidence[0].label, "意图分类示例");
        assert_eq!(evidence[1].kind, "reference");
        assert!(official_evidence_domain(
            "https://docs.vendor.com.au/case",
            "https://api.vendor.com.au/api"
        ));
        assert!(!official_evidence_domain(
            "https://media.other.com.au/news",
            "https://api.vendor.com.au/api"
        ));
        assert!(!official_evidence_domain(
            "https://docs.vendor.test.evil.test/case",
            "https://api.vendor.test/api"
        ));
        assert!(!official_evidence_domain(
            "https://github.com/other/case",
            "https://github.com/vendor/model"
        ));
    }

    #[test]
    fn uses_confirmed_difficulty_and_focus_in_search_and_keeps_small_models_late_in_catalogues() {
        let mut card = super::super::tests::card();
        card.tasks[0].fields.get_mut("taskType").unwrap().value = "文生文".into();
        card.tasks[0]
            .fields
            .get_mut("taskDifficulty")
            .unwrap()
            .value = "易".into();
        card.tasks[0].fields.get_mut("taskFocus").unwrap().value = "意图分类、信息抽取".into();
        let queries = research_queries(&card)
            .iter()
            .map(|(_, query)| query.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        for term in [
            "意图分类",
            "信息抽取",
            "难度易",
            "14B",
            "A3B",
            "非思考",
            "small mini nano",
        ] {
            assert!(queries.contains(term), "missing {term}");
        }
        let text = format!(
            "# Catalogue\n{}\nQwen3-8B supports extraction\n{}\nQwen3-14B supports intent classification\n{}\nQwen3-30B-A3B supports structured output",
            "旗舰介绍\n".repeat(3000),
            "description\n".repeat(500),
            "description\n".repeat(500)
        );
        let text = page_text(text, false).unwrap();
        let source = Source {
            id: "docs".into(),
            area: "capabilities".into(),
            title: "目录".into(),
            url: "https://example.com/models".into(),
            status: "read".into(),
            checked_at_ms: 1,
            error: String::new(),
            text,
        };
        let excerpt = recommendation_content(&source, &card);
        for term in ["Qwen3-8B", "Qwen3-14B", "Qwen3-30B-A3B"] {
            assert!(excerpt.contains(term), "missing {term}");
        }
        assert!(excerpt.chars().count() <= 4500);
        card.tasks[0].fields.get_mut("taskType").unwrap().value = "代码生成".into();
        card.tasks[0]
            .fields
            .get_mut("taskDifficulty")
            .unwrap()
            .value = "难".into();
        card.tasks[0].fields.get_mut("taskFocus").unwrap().value = "算法推理".into();
        let queries = research_queries(&card)
            .iter()
            .map(|(_, query)| query.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(queries.contains("算法推理") && queries.contains("难度难"));
        assert!(!queries.contains("小参数模型") && !queries.contains("small mini nano"));
    }

    #[test]
    fn unifies_legacy_pool_caps_ten_and_keeps_only_observed_api_links() {
        let card = super::super::tests::card();
        let models = (0..8)
            .map(|index| {
                serde_json::json!({
                    "model": format!("m{index}"), "reason": "结构化输出适合当前输入。",
                    "keywords": ["结构化输出", "编造关键词"], "apiUrl": "https://example.com/api"
                })
            })
            .collect::<Vec<_>>();
        let extra = (7..14)
            .map(|index| {
                serde_json::json!({
                    "model": format!("m{index}"), "reason": "另一个匹配型号。",
                    "apiUrl": "https://invented.example/api"
                })
            })
            .collect::<Vec<_>>();
        let mut report = parse_report(
            &serde_json::json!({"candidates": models, "suitableModels": extra}).to_string(),
            &card,
        )
        .unwrap();
        assert_eq!(report.groups[0].candidates.len(), 10);
        assert_eq!(report.groups[0].candidates[8].model, "m8");
        assert!(report.groups[0].suitable_models.is_empty());
        assert_eq!(report.groups[0].candidates[0].keywords, vec!["结构化输出"]);
        let source = Source {
            id: "docs".into(),
            area: "capabilities".into(),
            title: "官方接入".into(),
            url: "https://example.com/docs".into(),
            status: "read".into(),
            checked_at_ms: 1,
            error: String::new(),
            text: "API: https://example.com/api".into(),
        };
        discard_unverified(&mut report, &[source]);
        assert_eq!(
            report.groups[0].candidates[0].api_url,
            "https://example.com/api"
        );
        assert!(report.groups[0].candidates[8].api_url.is_empty());
        validate_report(&report, &card, &[]).unwrap();
    }

    #[test]
    fn assigns_one_distinct_ai_main_and_fallback_and_preserves_explicit_choices() {
        let card = super::super::tests::card();
        let mut report = parse_report(
            r#"{"candidates":[
            {"model":"a","role":"fallback","fallbackRank":1,"reason":"适合兜底。"},
            {"model":"b","role":"candidate","fallbackRank":2,"reason":"另一种兜底取舍。"},
            {"model":"c","role":"main","mainRank":1,"reason":"最符合本次需求。"},
            {"model":"d","role":"main","mainRank":"2","reason":"次选主力。"}
        ]}"#,
            &card,
        )
        .unwrap();
        assert_eq!(report.groups[0].candidates[3].role, "candidate");
        report.assign_recommendations();
        assert_eq!(
            report.recommended_pair(&card.tasks[0].id),
            (Some("c".into()), Some("a".into()))
        );
        assert_eq!(
            report.groups[0]
                .candidates
                .iter()
                .filter(|model| model.role == "main")
                .count(),
            1
        );
        assert_eq!(
            report.groups[0]
                .candidates
                .iter()
                .filter(|model| model.role == "fallback")
                .count(),
            1
        );
        assert_eq!(report.groups[0].candidates[3].main_rank, 2);
        assert_eq!(report.groups[0].candidates[1].fallback_rank, 2);
        for rank in [1, 2] {
            assert_eq!(
                report.groups[0]
                    .candidates
                    .iter()
                    .filter(|model| model.main_rank == rank)
                    .count(),
                1
            );
            assert_eq!(
                report.groups[0]
                    .candidates
                    .iter()
                    .filter(|model| model.fallback_rank == rank)
                    .count(),
                1
            );
        }
        assert_eq!(response_rank(&serde_json::json!({"rank":99}), "rank"), 0);
        assert_eq!(response_rank(&serde_json::json!({"rank":null}), "rank"), 0);
        validate_report(&report, &card, &[]).unwrap();
        let mut legacy = parse_report(
            r#"{"candidates":[{"model":"a","reason":"首选。"},{"model":"b","reason":"备选。"}]}"#,
            &card,
        )
        .unwrap();
        legacy.assign_recommendations();
        assert_eq!(
            legacy.recommended_pair(&card.tasks[0].id),
            (Some("a".into()), Some("b".into()))
        );
        legacy.groups[0].candidates.truncate(1);
        legacy.assign_recommendations();
        assert_eq!(
            legacy.recommended_pair(&card.tasks[0].id),
            (Some("a".into()), None)
        );
    }

    #[test]
    fn supplements_a_missing_region_without_erasing_existing_recommendations() {
        let card = super::super::tests::card();
        let domestic = parse_report(r#"{"candidates":[{"model":"china-a","origin":"china","reason":"适合当前任务。"},{"model":"china-b","origin":"china","reason":"成本取舍。"}]}"#, &card).unwrap();
        assert_eq!(missing_region(&domestic, false), Some("overseas"));
        assert_eq!(missing_region(&domestic, true), None);
        let extra = parse_report(r#"{"candidates":[{"model":"overseas-a","origin":"overseas","reason":"另一种能力取舍。"},{"model":"china-a","origin":"china","reason":"重复项。"}]}"#, &card).unwrap();
        let merged = merge_supplement(domestic, extra);
        assert_eq!(missing_region(&merged, false), None);
        assert_eq!(merged.groups[0].candidates.len(), 3);
        assert_eq!(merged.groups[0].candidates[0].model, "china-a");
        assert_eq!(merged.groups[0].candidates[1].model, "overseas-a");
        assert!(merged.has_candidate(&card.tasks[0].id, "china-b"));
        validate_report(&merged, &card, &[]).unwrap();
        let overseas = parse_report(
            r#"{"models":[{"model":"overseas-a","origin":"overseas","reason":"适合当前任务。"}]}"#,
            &card,
        )
        .unwrap();
        assert_eq!(missing_region(&overseas, false), Some("china"));
    }

    #[test]
    fn keeps_official_overseas_catalogues_when_the_search_results_fill_the_window() {
        let source = |id: usize, url: String| Source {
            id: format!("s{id}"),
            area: "capabilities".into(),
            title: "docs".into(),
            url,
            status: "read".into(),
            checked_at_ms: 1,
            text: "model documentation".into(),
            error: String::new(),
        };
        let mut sources = (0..16)
            .map(|id| source(id, format!("https://example.com/{id}")))
            .collect::<Vec<_>>();
        sources.extend(
            official_pages(&[], false)
                .iter()
                .enumerate()
                .map(|(id, page)| source(16 + id, page.2.clone())),
        );
        let selected = recommendation_sources(&sources, false);
        assert_eq!(selected.len(), 16);
        for domain in [
            "claude.com",
            "google.dev",
            "openai.com",
            "deepseek.com",
            "aliyun.com",
            "bigmodel.cn",
        ] {
            assert!(
                selected.iter().any(|source| source.url.contains(domain)),
                "lost {domain}"
            );
        }
        sources.last_mut().unwrap().status = "failed".into();
        assert!(
            recommendation_sources(&sources, false)
                .iter()
                .all(|source| source.status == "read")
        );
    }

    #[test]
    fn accepts_useful_model_rows_despite_optional_fields_and_schema_differences() {
        let card = super::super::tests::card();
        let raw = serde_json::json!({
            "analysis": "无需展示的额外说明",
            "findings": null,
            "groups": [{"taskId": card.tasks[0].id, "priorityReason": null, "leaderboard": "暂无共同榜单", "suitableModels": null,
                "candidates": [
                    {"modelId":"model-a", "name":"Model A", "region":"国内模型", "reason":"适合会议转写并保留术语。", "score":95, "role":"primary", "basis":"business-fit", "evidence":null, "test":null},
                    {"model":"model-b", "origin":"international", "reason":"适合多语言转写。", "evidence":[{"url":"https://example.com/fake", "quote":"未核验资料"}]},
                    {"model":"model-a", "reason":"重复项"},
                    {"model":"bad-row", "reason":null}
                ]}]
        });
        let mut report =
            parse_report(&format!("结果如下：\n```json\n{raw}\n```\n已完成。"), &card).unwrap();
        discard_unverified(&mut report, &[]);
        validate_report(&report, &card, &[]).unwrap();
        assert_eq!(report.groups[0].candidates.len(), 2);
        assert_eq!(report.groups[0].candidates[0].model, "model-a");
        assert_eq!(report.groups[0].candidates[0].display_name, "Model A");
        assert_eq!(report.groups[0].candidates[0].origin, "china");
        assert_eq!(report.groups[0].candidates[1].origin, "overseas");
        assert!(report.groups[0].leaderboard.is_none());
        assert!(
            report.groups[0]
                .candidates
                .iter()
                .all(|model| model.evidence.is_empty())
        );
        assert!(report.groups[0].priority_reason.is_empty());
    }

    #[test]
    fn accepts_flat_recommendations_and_binds_only_an_unambiguous_task() {
        let mut card = super::super::tests::card();
        for raw in [
            r#"{"recommendations":[{"name":"Model A","recommendationReason":"适合此任务。"}]}"#,
            r#"[{"model":"Model A","reason":"适合此任务。"}]"#,
            r#"{"report":{"groups":[{"candidates":[{"model":"Model A","reason":"适合此任务。"}]}]}}"#,
        ] {
            let report = parse_report(raw, &card).unwrap();
            validate_report(&report, &card, &[]).unwrap();
            assert!(report.has_candidate(&card.tasks[0].id, "Model A"));
        }
        assert!(
            parse_report(
                r#"{"groups":[{"candidates":[{"model":"没有理由"}]}]}"#,
                &card
            )
            .is_err()
        );
        assert!(parse_report("无法推荐", &card).is_err());
        let mut other = card.tasks[0].clone();
        other.id = "another-task".into();
        card.tasks.push(other);
        assert!(
            parse_report(
                r#"{"recommendations":[{"model":"Model A","reason":"适合此任务。"}]}"#,
                &card
            )
            .is_err()
        );
    }

    #[test]
    fn derives_modality_requirements_for_all_fixed_task_types() {
        let mut card = super::super::tests::card();
        for (kind, input, output, capability) in [
            ("文生文", "Text", "Text", "TG"),
            ("文生图", "Text", "Image", "IG"),
            ("语音转写", "Audio", "Text", "ASR"),
            ("语音合成", "Text", "Audio", "TTS"),
            ("图片理解", "Image", "Text", "VU"),
            ("视频理解", "Video", "Text", "VU"),
            ("视频生成", "Text", "Video", "VG"),
            ("代码生成", "Text", "Text", "TG"),
        ] {
            card.tasks[0].fields.get_mut("taskType").unwrap().value = kind.into();
            let requirements = task_modalities(&card.tasks[0]);
            assert_eq!(requirements.len(), 1);
            assert_eq!(requirements[0]["inputModality"], serde_json::json!([input]));
            assert_eq!(
                requirements[0]["outputModality"],
                serde_json::json!([output])
            );
            assert_eq!(
                requirements[0]["requiredCapabilities"],
                serde_json::json!([capability])
            );
        }
        card.tasks[0].fields.get_mut("taskType").unwrap().value = "图片理解、代码生成".into();
        assert_eq!(task_modalities(&card.tasks[0]).len(), 2);
        card.tasks[0]
            .fields
            .get_mut("taskType")
            .unwrap()
            .value
            .clear();
        assert!(task_modalities(&card.tasks[0]).is_empty());
    }

    #[test]
    fn searches_across_providers_using_confirmed_work_and_model_requirements() {
        let mut card = super::super::tests::card();
        for (key, value) in [
            ("businessGoal", "生成互动课件"),
            ("input", "教师描述"),
            ("output", "HTML代码"),
            ("cost", "每月200元"),
            ("constraints", "独立运行、128K上下文"),
        ] {
            card.tasks[0].fields.get_mut(key).unwrap().value = value.into();
        }
        card.tasks[0].user_concerns = vec!["结构化输出".into(), "删除项不要恢复".into()];
        card.tasks[0].excluded_user_concerns = vec!["删除项不要恢复".into()];
        let queries = research_queries(&card);
        let text = queries
            .iter()
            .map(|(_, query)| query.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        for value in [
            "生成互动课件",
            "教师描述",
            "HTML代码",
            "每月200元",
            "128K上下文",
            "结构化输出",
            "international",
        ] {
            assert!(text.contains(value), "missing {value}");
        }
        assert!(!text.contains("删除项不要恢复"));
        assert!(!text.contains("site:"));
        assert!(
            queries
                .iter()
                .any(|(_, query)| query.contains("international")
                    && query.contains("speech-to-text")
                    && query.is_ascii())
        );
        card.exclude_overseas = true;
        assert!(
            research_queries(&card)
                .iter()
                .all(|(_, query)| !query.contains("international") && !query.contains("海外"))
        );
    }

    #[test]
    fn returns_recommendations_without_board_count_or_region_quotas() {
        let card = super::super::tests::card();
        let mut report: Report = serde_json::from_value(serde_json::json!({
            "groups":[{"taskId":card.tasks[0].id,"leaderboard":{"sourceId":"missing","quote":"fake board"},"candidates":[
                {"model":"Text Model A","origin":"china","reason":"适合中文录音转写。"},
                {"model":"Text Model B","origin":"china","reason":"适合保留说话人和专业术语。","evidence":[{"sourceId":"missing","quote":"fake proof"}]}
            ]}]
        })).unwrap();
        discard_unverified(&mut report, &[]);
        validate_report(&report, &card, &[]).unwrap();
        assert_eq!(report.groups[0].candidates.len(), 2);
        assert!(report.groups[0].leaderboard.is_none());
        assert!(
            report.groups[0]
                .candidates
                .iter()
                .all(|model| model.basis == "inference" && model.evidence.is_empty())
        );
        assert!(!verified_model_id("model-v5", "Model V5", &[]));
    }

    #[test]
    fn uses_search_page_content_but_never_summary_as_retrieved_evidence() {
        let page = |content: &str| {
            serde_json::from_value::<web::SearchPage>(serde_json::json!({"title":"Text board","url":"https://example.com/board","snippet":"not evidence","summary":"not page content","content":content})).unwrap()
        };
        let mut sources = vec![];
        let pages = search_pages(
            &mut sources,
            vec![("ranking", Ok(vec![page("# Text board\nModel X: 1500")]))],
            &|_| {},
        );
        assert!(pages.is_empty());
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].status, "read");
        assert!(sources[0].text.contains("Model X: 1500"));
        let mut sources = vec![];
        let pages = search_pages(&mut sources, vec![("ranking", Ok(vec![page("")]))], &|_| {});
        assert_eq!(pages.len(), 1);
        assert!(sources.is_empty());
        let navigation = format!(
            "Markdown Content:\n{}\n# Current models\nnew-model-id",
            "navigation ".repeat(2000)
        );
        assert!(
            page_text(navigation, true)
                .unwrap()
                .contains("new-model-id")
        );
    }

    #[test]
    fn recommends_known_models_by_business_fit_without_web_evidence() {
        let card = super::super::tests::card();
        let mut report: Report = serde_json::from_value(serde_json::json!({
            "findings": AREAS.iter().map(|area| serde_json::json!({"area":area,"summary":"没有公开数据","evidence":[]})).collect::<Vec<_>>(),
            "groups":[{"taskId":card.tasks[0].id,"candidates":[
                {"model":"Known Text Model","role":"main","basis":"inference","reason":"建议先用于中文意图识别和结构化填槽，验证教师需求能否被正确理解。","evidence":[{"sourceId":"missing","quote":"invented proof"}],"test":"核对意图和补齐情况","gaps":[]}
            ],"gaps":[]}],"questions":[]
        })).unwrap();
        discard_unverified(&mut report, &[]);
        assert_eq!(report.groups[0].candidates.len(), 1);
        assert!(report.groups[0].candidates[0].evidence.is_empty());
        assert!(validate_report(&report, &card, &[]).is_ok());
        let sources = vec![Source {
            id: "docs".into(),
            area: "capabilities".into(),
            title: "Docs".into(),
            url: "https://example.com/docs".into(),
            status: "read".into(),
            checked_at_ms: 1,
            error: String::new(),
            text: "Known Text Model supports text generation".into(),
        }];
        assert!(verified_model_id("Known Text Model", "", &sources));
        report.groups[0].candidates[0].basis = "source".into();
        assert!(validate_report(&report, &card, &[]).is_err());
    }

    #[test]
    fn keeps_verified_recommendations_when_other_quotes_fail() {
        let card = super::super::tests::card();
        let sources = vec![
            Source {
                id: "s0".into(),
                area: "capabilities".into(),
                title: "Official docs".into(),
                url: "https://example.com/docs".into(),
                status: "read".into(),
                checked_at_ms: 1,
                error: String::new(),
                text: "Model X supports\ntext generation".into(),
            },
            Source {
                id: "blocked".into(),
                area: "ranking".into(),
                title: "Blocked".into(),
                url: "https://example.com/blocked".into(),
                status: "failed".into(),
                checked_at_ms: 1,
                error: "403".into(),
                text: "Model Z is best".into(),
            },
        ];
        let mut report: Report = serde_json::from_value(serde_json::json!({
            "findings": AREAS.iter().map(|area| serde_json::json!({"area":area,"summary":"needs verification","evidence":[{"sourceId":"s0","quote":"invented ranking"}]})).collect::<Vec<_>>(),
            "groups":[{"taskId":card.tasks[0].id,"candidates":[
                {"model":"Model X","role":"main","reason":"supports this task","evidence":[{"sourceId":"s0","quote":"supports text generation"}],"test":"check with business samples","gaps":[]},
                {"model":"Model Y","role":"fallback","reason":"invented","evidence":[{"sourceId":"s0","quote":"Model Y is perfect"}],"test":"check","gaps":[]},
                {"model":"Model Z","role":"fallback","reason":"blocked page","evidence":[{"sourceId":"blocked","quote":"Model Z is best"}],"test":"check","gaps":[]}
            ],"gaps":[]}],"questions":[]
        })).unwrap();
        discard_unverified(&mut report, &sources);
        assert!(validate_report(&report, &card, &sources).is_ok());
        assert_eq!(report.groups[0].candidates.len(), 3);
        assert!(report.has_candidate(&card.tasks[0].id, "Model X"));
        assert!(report.has_candidate(&card.tasks[0].id, "Model Y"));
        assert!(report.groups[0].candidates[1].evidence.is_empty());
        assert!(report.has_candidate(&card.tasks[0].id, "Model Z"));
        assert!(report.groups[0].candidates[2].evidence.is_empty());
        assert!(
            report
                .findings
                .iter()
                .all(|finding| finding.evidence.is_empty())
        );
        assert_eq!(status(&report, &sources), "partial");
    }

    #[test]
    fn keeps_every_search_failure_even_when_no_pages_can_be_read() {
        let mut sources = vec![];
        let saved = Mutex::new(vec![]);
        let pages = search_pages(
            &mut sources,
            AREAS[1..]
                .iter()
                .map(|area| (*area, Err(web::status_error("DeepSeek 官网搜索", 429))))
                .collect(),
            &|items| *saved.lock().unwrap() = items,
        );
        assert!(pages.is_empty());
        assert_eq!(saved.lock().unwrap().len(), 3);
        assert!(
            saved
                .lock()
                .unwrap()
                .iter()
                .all(|source| source.error.contains("429"))
        );
        let fallback = official_pages(&sources, false);
        assert_eq!(fallback.len(), 8);
        let domestic_pages = official_pages(&sources, true);
        assert_eq!(domestic_pages.len(), 3);
        assert!(
            domestic_pages
                .iter()
                .all(|page| !page.2.contains("claude.com")
                    && !page.2.contains("google.dev")
                    && !page.2.contains("openai.com"))
        );
        assert!(fallback.iter().all(|page| safe_url(&page.2)));
        sources.push(Source {
            id: "ok".into(),
            area: "capabilities".into(),
            title: "docs".into(),
            url: "https://example.com/docs".into(),
            status: "read".into(),
            checked_at_ms: 1,
            error: String::new(),
            text: "verified".into(),
        });
        assert_eq!(official_pages(&sources, false).len(), 8);
        sources.last_mut().unwrap().url =
            "https://api-docs.deepseek.com/quick_start/pricing".into();
        assert_eq!(official_pages(&sources, false).len(), 7);
    }

    #[test]
    fn distinguishes_target_blocks_from_reader_and_protocol_failures() {
        assert!(
            page_text(
                "Title: Just a moment...\nEnable JavaScript and cookies to continue".into(),
                true
            )
            .unwrap_err()
            .message
            .contains("403")
        );
        assert!(
            page_text(
                "Warning: Target URL returned error 404: Not Found\nMarkdown Content:".into(),
                true
            )
            .unwrap_err()
            .message
            .contains("404")
        );
        assert!(page_text("not markdown".into(), true).is_err());
        assert_eq!(
            page_text("# Official model details".into(), false).unwrap(),
            "# Official model details"
        );
        assert!(!safe_url("https://127.0.0.1/admin"));
        assert!(!safe_url("https://name:password@example.com/docs"));
    }

    #[test]
    fn rejects_invented_or_failed_page_quotes_and_requires_all_task_groups() {
        let card = super::super::tests::card();
        let mut sources = vec![Source {
            id: "s0".into(),
            area: "capabilities".into(),
            title: "docs".into(),
            url: "https://example.com/docs".into(),
            status: "read".into(),
            checked_at_ms: 1,
            error: String::new(),
            text: "Model X supports text generation".into(),
        }];
        let mut report = Report {
            notice: String::new(),
            findings: AREAS
                .iter()
                .map(|area| Finding {
                    area: area.to_string(),
                    summary: "资料有缺口".into(),
                    evidence: vec![],
                })
                .collect(),
            groups: vec![Group {
                task_id: card.tasks[0].id.clone(),
                priority_reason: "用户希望少反复解释，需核对识别正确率，优先准确性。".into(),
                candidates: vec![serde_json::from_value(serde_json::json!({"model":"Model X","reason":"适合文本生成。","basis":"inference"})).unwrap()],
                leaderboard: None,
                suitable_models: vec![],
                gaps: vec!["缺少资料".into()],
            }],
            questions: vec![],
        };
        assert!(validate_report(&report, &card, &sources).is_ok());
        let priority_reason = report.groups[0].priority_reason.clone();
        report.groups[0].priority_reason = "字".repeat(601);
        assert!(validate_report(&report, &card, &sources).is_err());
        report.groups[0].priority_reason = priority_reason;
        report.findings[1].evidence.push(Evidence {
            source_id: "s0".into(),
            quote: "invented claim".into(),
            kind: String::new(),
            label: String::new(),
        });
        assert!(validate_report(&report, &card, &sources).is_err());
        report.findings[1].evidence[0].quote = "supports text generation".into();
        assert!(validate_report(&report, &card, &sources).is_ok());
        sources[0].status = "failed".into();
        assert!(validate_report(&report, &card, &sources).is_err());
        report.findings[1].evidence.clear();
        report.groups.clear();
        assert!(validate_report(&report, &card, &sources).is_err());
    }
}
