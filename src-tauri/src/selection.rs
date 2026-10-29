use super::*;
use std::collections::HashSet;
pub(crate) mod conversations;
mod research;
mod web;

const DEFAULT_MODEL: &str = "deepseek-flash";
const DRIVER_KEY_FILE: &str = "selection-driver.json";
const EXTRACT_TIMEOUT: Duration = Duration::from_secs(60);
const FIELD_KEYS: [&str; 13] = [
    "taskType",
    "taskDifficulty",
    "taskFocus",
    "businessContext",
    "businessGoal",
    "input",
    "output",
    "accuracy",
    "speed",
    "cost",
    "constraints",
    "priorities",
    "fallback",
];

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AssistantSettings {
    #[serde(default)]
    has_credential: bool,
    #[serde(default = "default_model")]
    model_id: String,
}

fn default_model() -> String {
    DEFAULT_MODEL.into()
}

impl Default for AssistantSettings {
    fn default() -> Self {
        Self {
            has_credential: false,
            model_id: default_model(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RequirementField {
    value: String,
    source: String,
    evidence: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RequirementTask {
    #[serde(default)]
    id: String,
    name: String,
    #[serde(default)]
    user_concerns: Vec<String>,
    #[serde(default)]
    excluded_user_concerns: Vec<String>,
    fields: BTreeMap<String, RequirementField>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Suggestion {
    #[serde(default)]
    id: String,
    #[serde(default)]
    task_index: usize,
    field: String,
    value: String,
    reason: String,
    #[serde(default)]
    accepted: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RequirementCard {
    #[serde(default)]
    description: String,
    #[serde(default)]
    exclude_overseas: bool,
    #[serde(default)]
    project_context: Option<RequirementField>,
    #[serde(default)]
    usage_scenario: Option<RequirementField>,
    #[serde(default)]
    target_user: Option<RequirementField>,
    tasks: Vec<RequirementTask>,
    #[serde(default)]
    suggestions: Vec<Suggestion>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Invocation {
    model_id: String,
    connection_name: String,
    elapsed_ms: u64,
    first_token_ms: Option<u64>,
    usage: Option<Usage>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
struct SelectionMetrics {
    completed_calls: u64,
    usage_calls: u64,
    total_tokens: u64,
    cache_hit_tokens: u64,
    cache_input_tokens: u64,
    latest: Option<Invocation>,
}

impl SelectionMetrics {
    fn record(&mut self, invocation: Invocation) {
        self.completed_calls += 1;
        if let Some(usage) = &invocation.usage {
            if let Some(total) = usage
                .total_tokens
                .or_else(|| Some(usage.input_tokens? + usage.output_tokens?))
            {
                self.usage_calls += 1;
                self.total_tokens += total;
            }
            if let (Some(input), Some(cached)) = (usage.input_tokens, usage.cached_input_tokens)
                && cached <= input
            {
                self.cache_input_tokens += input;
                self.cache_hit_tokens += cached;
            }
        }
        self.latest = Some(invocation);
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConfirmedCard {
    card: RequirementCard,
    revision: u64,
    confirmed_at_ms: u64,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExtractionProgress {
    message: String,
    started_at_ms: u64,
    analysis_preview: String,
    received_chars: usize,
    model_started_at_ms: Option<u64>,
    last_output_at_ms: Option<u64>,
}

fn update_stream_progress(progress: &mut ExtractionProgress, content: &str, reasoning: &str) {
    let now = now_ms();
    if content.is_empty() && reasoning.is_empty() {
        progress.model_started_at_ms = Some(now);
        progress.last_output_at_ms = None;
        progress.analysis_preview.clear();
        progress.received_chars = 0;
        return;
    }
    progress.last_output_at_ms = Some(now);
    progress.received_chars += content.chars().count() + reasoning.chars().count();
    // 只保留最近的可见片段，不把整段分析重复传给界面。
    progress.analysis_preview.push_str(reasoning);
    let start = progress
        .analysis_preview
        .char_indices()
        .rev()
        .nth(799)
        .map(|(index, _)| index)
        .unwrap_or(0);
    if start > 0 {
        progress.analysis_preview.drain(..start);
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct IntentOption {
    id: String,
    name: String,
    work: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(untagged)]
enum IntentSelection {
    Record {
        options: Vec<IntentOption>,
        #[serde(rename = "selectedTaskId")]
        selected_task_id: String,
    },
    Legacy(String),
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SelectionSession {
    id: String,
    revision: u64,
    description: String,
    running: bool,
    intent_selections: Vec<IntentSelection>,
    draft: Option<RequirementCard>,
    confirmed: Option<ConfirmedCard>,
    invocation: Option<Invocation>,
    #[serde(default)]
    metrics: SelectionMetrics,
    progress: Option<ExtractionProgress>,
    task_runs: BTreeMap<String, TaskRun>,
    #[serde(default)]
    storage_error: String,
    #[serde(skip)]
    attempt: Option<Attempt>,
    #[serde(skip)]
    confirmation_request: Option<String>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskRun {
    research: Option<research::ResearchResult>,
    selected_model: Option<String>,
    fallback_model: Option<String>,
}

#[derive(Clone)]
struct Attempt {
    request_id: String,
    description: String,
    result: Option<AppResult<Box<SelectionSession>>>,
}

impl Default for SelectionSession {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            revision: 0,
            description: String::new(),
            running: false,
            intent_selections: Vec::new(),
            draft: None,
            confirmed: None,
            invocation: None,
            metrics: SelectionMetrics::default(),
            progress: None,
            task_runs: BTreeMap::new(),
            storage_error: String::new(),
            attempt: None,
            confirmation_request: None,
        }
    }
}

#[derive(Default)]
pub(crate) struct SelectionState(Mutex<SelectionSession>, Mutex<conversations::History>);

fn validation(message: &str) -> AppError {
    AppError::new("VALIDATION_ERROR", message, false)
}

fn version_check(session: &SelectionSession, id: &str, revision: u64) -> AppResult<()> {
    if session.id != id || session.revision != revision {
        return Err(AppError::new(
            "SELECTION_STALE",
            "需求已变化，请重新读取当前需求后再试。",
            true,
        ));
    }
    Ok(())
}

fn validate_description(description: &str) -> AppResult<()> {
    if description.chars().count() > 20_000 {
        return Err(validation("需求描述不能超过 20,000 字。"));
    }
    Ok(())
}

fn legacy_io_parts(value: &str) -> (String, String) {
    let value = value.trim();
    let input = |part: &str| {
        part.trim()
            .strip_prefix("输入")
            .unwrap_or(part.trim())
            .trim_start_matches(['：', ':', ' '])
            .to_string()
    };
    let output = |part: &str| {
        part.trim()
            .strip_prefix("输出")
            .unwrap_or(part.trim())
            .trim_start_matches(['：', ':', ' '])
            .to_string()
    };
    for (index, separator) in value.char_indices() {
        if ['；', ';', '\n'].contains(&separator) {
            let tail = value[index + separator.len_utf8()..].trim();
            if tail.starts_with("输出：") || tail.starts_with("输出:") {
                return (input(&value[..index]), output(tail));
            }
        }
    }
    if value.starts_with("输出") {
        (String::new(), output(value))
    } else {
        (input(value), String::new())
    }
}

fn migrate_legacy_io(card: &mut RequirementCard) {
    for task in &mut card.tasks {
        for key in ["taskDifficulty", "taskFocus"] {
            task.fields
                .entry(key.into())
                .or_insert_with(|| RequirementField {
                    value: String::new(),
                    source: "missing".into(),
                    evidence: String::new(),
                });
        }
        if let Some(legacy) = task.fields.remove("inputOutput") {
            let (input, output) = legacy_io_parts(&legacy.value);
            for (key, value) in [("input", input), ("output", output)] {
                let field = if value.is_empty() {
                    RequirementField {
                        value,
                        source: "missing".into(),
                        evidence: String::new(),
                    }
                } else {
                    RequirementField {
                        value,
                        ..legacy.clone()
                    }
                };
                task.fields.entry(key.into()).or_insert(field);
            }
        }
    }
    card.suggestions = std::mem::take(&mut card.suggestions)
        .into_iter()
        .flat_map(|suggestion| {
            if suggestion.field != "inputOutput" {
                return vec![suggestion];
            }
            let (input, output) = legacy_io_parts(&suggestion.value);
            [("input", input), ("output", output)]
                .into_iter()
                .filter(|(_, value)| !value.is_empty())
                .map(|(key, value)| Suggestion {
                    id: format!("{}-{key}", suggestion.id),
                    field: key.into(),
                    value,
                    ..suggestion.clone()
                })
                .collect()
        })
        .collect();
}

fn priority_values(value: &str) -> Option<Vec<&str>> {
    if value.trim().is_empty() {
        return Some(vec![]);
    }
    let values: Vec<_> = value.split(['、', '>']).map(str::trim).collect();
    (values.len() <= 3
        && values
            .iter()
            .all(|item| ["准确性", "速度", "成本"].contains(item))
        && values.iter().collect::<HashSet<_>>().len() == values.len())
    .then_some(values)
}

fn validate_card(card: &RequirementCard, extracted: bool) -> AppResult<()> {
    validate_description(&card.description)?;
    if card.tasks.is_empty() || card.tasks.len() > 32 || card.suggestions.len() > 100 {
        return Err(validation(
            "需求卡需包含 1～32 项任务，建议不能超过 100 条。",
        ));
    }
    let mut ids = HashSet::new();
    for field in [
        &card.project_context,
        &card.usage_scenario,
        &card.target_user,
    ]
    .into_iter()
    .flatten()
    {
        validate_field(field, &card.description, extracted)?;
    }
    for task in &card.tasks {
        if !ids.insert(&task.id)
            || task.id.is_empty()
            || task.name.trim().is_empty()
            || task.name.chars().count() > 200
        {
            return Err(validation("任务标识或名称无效，请检查需求表。"));
        }
        if task.fields.len() != FIELD_KEYS.len()
            || FIELD_KEYS.iter().any(|key| !task.fields.contains_key(*key))
        {
            return Err(validation("需求字段不完整，请重新整理或手工填写。"));
        }
        if !["", "易", "中", "难"].contains(&task.fields["taskDifficulty"].value.as_str()) {
            return Err(validation("任务难度只能选择易、中、难。"));
        }
        let priority = task.fields["priorities"].value.trim();
        if task.user_concerns.len() > 8
            || task.user_concerns.iter().any(|label| {
                label.trim().is_empty()
                    || label.chars().count() > 40
                    || label.contains(['\n', '\r'])
            })
        {
            return Err(validation("用户关注点标签需为简短单行文字，最多 8 项。"));
        }
        if task.excluded_user_concerns.len() > 8
            || task
                .excluded_user_concerns
                .iter()
                .any(|label| !task.user_concerns.contains(label))
        {
            return Err(validation("未选中的标签必须属于当前业务标签。"));
        }
        if priority_values(priority).is_none() {
            return Err(validation(
                "业务优先级只能选择准确性、速度、成本，不能重复。",
            ));
        }
        for field in task.fields.values() {
            validate_field(field, &card.description, extracted)?;
        }
    }
    let mut suggestion_ids = HashSet::new();
    for suggestion in &card.suggestions {
        if suggestion.id.is_empty()
            || !suggestion_ids.insert(&suggestion.id)
            || suggestion.task_index >= card.tasks.len()
            || !FIELD_KEYS.contains(&suggestion.field.as_str())
            || suggestion.value.trim().is_empty()
            || suggestion.value.chars().count() > 4_000
            || suggestion.reason.chars().count() > 4_000
            || (extracted && suggestion.accepted)
        {
            return Err(validation("待确认建议格式无效。"));
        }
        if suggestion.field == "taskDifficulty"
            && !["易", "中", "难"].contains(&suggestion.value.as_str())
        {
            return Err(validation("任务难度建议只能为易、中、难。"));
        }
        if suggestion.accepted
            && card.tasks[suggestion.task_index].fields[&suggestion.field].value != suggestion.value
        {
            return Err(validation("采纳建议与当前字段不一致，请重新核对。"));
        }
    }
    Ok(())
}

fn validate_field(field: &RequirementField, description: &str, extracted: bool) -> AppResult<()> {
    if field.value.chars().count() > 4_000 || field.evidence.chars().count() > 20_000 {
        return Err(validation("单项要求不能超过 4,000 字。"));
    }
    if !matches!(field.source.as_str(), "provided" | "missing" | "user")
        || (extracted && field.source == "user")
    {
        return Err(validation("需求字段的来源无效。"));
    }
    if field.value.trim().is_empty() != (field.source == "missing") {
        return Err(validation("未提供的字段必须留空，不能填入默认条件。"));
    }
    if (field.source == "provided" && field.evidence.trim().is_empty())
        || (!field.evidence.is_empty() && !description.contains(&field.evidence))
    {
        return Err(validation(
            "部分提取结果没有有效原话依据，请手工核对或重试。",
        ));
    }
    Ok(())
}

fn normalize_task_type(value: &str) -> String {
    let mut types = Vec::new();
    for part in value.split(['、', '，', ',']) {
        let key: String = part
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_ascii_lowercase();
        let mapped = match key.as_str() {
            "文生文" | "文本生成" | "文本理解" | "意图识别" | "填槽" | "课时编排" => {
                "文生文"
            }
            "文生图" | "图片生成" => "文生图",
            "语音转写" => "语音转写",
            "语音合成" => "语音合成",
            "图片理解" => "图片理解",
            "视频理解" => "视频理解",
            "视频生成" => "视频生成",
            "代码生成" | "互动课件生成" | "html课件拼接" | "html拼接" | "html生成" => {
                "代码生成"
            }
            _ => continue,
        };
        if !types.contains(&mapped) {
            types.push(mapped);
        }
    }
    types.join("、")
}

fn normalize_context(card: &mut RequirementCard) {
    for (index, task) in card.tasks.iter_mut().enumerate() {
        if let Some(field) = task.fields.get_mut("taskType") {
            field.value = normalize_task_type(&field.value);
            if field.value.is_empty() {
                field.source = "missing".into();
                field.evidence.clear();
            }
        }
        if let Some(field) = task.fields.get_mut("constraints") {
            let parts: Vec<_> = field.value.split(['；', ';', '。', '\n']).collect();
            let name: String = task
                .name
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>()
                .to_ascii_lowercase();
            let kept: Vec<_> = parts
                .iter()
                .copied()
                .filter(|part| {
                    let value: String = part
                        .chars()
                        .filter(|c| !c.is_whitespace())
                        .collect::<String>()
                        .to_ascii_lowercase();
                    ![
                        "使用ai",
                        "需要使用ai",
                        "需使用ai",
                        "必须使用ai",
                        "需要ai",
                        "需用ai",
                        "必须用ai",
                    ]
                    .contains(&value.strip_prefix(&name).unwrap_or(&value))
                })
                .collect();
            if kept.len() != parts.len() {
                field.value = kept
                    .into_iter()
                    .map(str::trim)
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join("；");
                if field.value.is_empty() {
                    field.source = "missing".into();
                    field.evidence.clear();
                }
                card.suggestions.retain(|suggestion| {
                    suggestion.task_index != index || suggestion.field != "constraints"
                });
            }
        }
    }
    for suggestion in &mut card.suggestions {
        if suggestion.field == "taskType" {
            suggestion.value = normalize_task_type(&suggestion.value);
        }
    }
    card.suggestions
        .retain(|suggestion| !suggestion.value.is_empty());
    if card.project_context.is_none()
        && let Some(first) = card
            .tasks
            .first()
            .and_then(|task| task.fields.get("businessContext"))
        && !first.value.is_empty()
        && card.tasks.iter().all(|task| {
            task.fields
                .get("businessContext")
                .is_some_and(|field| field.value == first.value)
        })
    {
        card.project_context = Some(first.clone());
    }
    if let Some(context) = &card.project_context {
        for (index, task) in card.tasks.iter_mut().enumerate() {
            if let Some(field) = task.fields.get_mut("businessContext")
                && field.value == context.value
            {
                *field = RequirementField {
                    value: String::new(),
                    source: "missing".into(),
                    evidence: String::new(),
                };
                for suggestion in &mut card.suggestions {
                    if suggestion.task_index == index && suggestion.field == "businessContext" {
                        suggestion.accepted = false;
                    }
                }
            }
        }
    }
}

fn parse_card(text: &str, description: &str) -> AppResult<RequirementCard> {
    let raw = text.trim();
    let json = if let Some(fenced) = raw
        .strip_prefix("```json")
        .or_else(|| raw.strip_prefix("```"))
    {
        fenced.trim().strip_suffix("```").unwrap_or(fenced).trim()
    } else {
        raw
    };
    let invalid_response = |_| {
        AppError::new(
            "RESPONSE_INVALID",
            "助手没有返回有效的需求表，请重试或手工填写。",
            true,
        )
    };
    let mut payload: serde_json::Value = serde_json::from_str(json).map_err(&invalid_response)?;
    let mut task_suggestions = Vec::new();
    if let Some(tasks) = payload
        .get_mut("tasks")
        .and_then(serde_json::Value::as_array_mut)
    {
        for (index, task) in tasks.iter_mut().enumerate() {
            if let Some(value) = task
                .as_object_mut()
                .and_then(|task| task.remove("suggestions"))
            {
                let suggestions: Vec<Suggestion> =
                    serde_json::from_value(value).map_err(&invalid_response)?;
                task_suggestions.extend(suggestions.into_iter().map(|mut suggestion| {
                    suggestion.task_index = index;
                    suggestion
                }));
            }
        }
    }
    let mut card: RequirementCard = serde_json::from_value(payload).map_err(invalid_response)?;
    card.suggestions.extend(task_suggestions);
    card.description = description.to_string();
    migrate_legacy_io(&mut card);
    for task in &mut card.tasks {
        task.id = Uuid::new_v4().to_string();
        // 空字段由程序补齐，避免模型反复生成相同的 missing 对象。
        for key in FIELD_KEYS {
            task.fields
                .entry(key.into())
                .or_insert_with(|| RequirementField {
                    value: String::new(),
                    source: "missing".into(),
                    evidence: String::new(),
                });
        }
        for (key, field) in &mut task.fields {
            if key == "priorities" {
                if let Some(values) = priority_values(&field.value) {
                    field.value = values.join("、");
                }
            }
            // 未知标记不是业务要求；保留原始描述，不把“预算未定”当作预算值。
            let label = match key.as_str() {
                "cost" => "预算",
                "speed" => "速度",
                "accuracy" => "准确率",
                _ => "",
            };
            let unknown = |text: &str| {
                let value = text.trim().trim_end_matches(['。', '；', ';']);
                let value = if label.is_empty() {
                    value
                } else {
                    value
                        .strip_prefix(label)
                        .unwrap_or(value)
                        .trim_start_matches(['：', ':', ' '])
                };
                matches!(
                    value,
                    "未定"
                        | "未知"
                        | "未提供"
                        | "未明确"
                        | "未说明"
                        | "不清楚"
                        | "还没定"
                        | "暂未确定"
                        | "尚未确定"
                        | "暂未明确"
                )
            };
            if unknown(&field.value) || (!label.is_empty() && unknown(&field.evidence)) {
                field.value.clear();
                field.source = "missing".into();
                field.evidence.clear();
            }
        }
    }
    for suggestion in &mut card.suggestions {
        suggestion.id = Uuid::new_v4().to_string();
    }
    validate_card(&card, true)?;
    normalize_context(&mut card);
    Ok(card)
}

fn official_connection() -> Connection {
    Connection {
        id: "selection-driver-official".into(),
        display_name: "DeepSeek 官网".into(),
        provider_kind: "deepseek_official".into(),
        base_url: "https://api.deepseek.com/v1".into(),
        has_credential: true,
        credential_length: None,
        models: vec![],
    }
}

fn fixed_model<'a>(connection: &Connection, requested: &'a str) -> AppResult<&'a str> {
    if connection.provider_kind != "deepseek_official" || requested != DEFAULT_MODEL {
        return Err(validation(
            "模型选型驱动 AI 仅支持 DeepSeek 官网的 V4.1 Flash 1017，请单独配置官网 API Key。",
        ));
    }
    Ok(requested)
}

fn driver_key(config_path: &Path) -> AppResult<String> {
    read_credentials(&config_path.with_file_name(DRIVER_KEY_FILE))?
        .remove("deepseek_official")
        .filter(|key| !key.is_empty())
        .ok_or_else(|| validation("请在模型配置中填写 DeepSeek 官网 API Key。"))
}

pub(crate) fn assistant_status(config_path: &Path) -> AssistantSettings {
    AssistantSettings {
        has_credential: driver_key(config_path).is_ok(),
        ..Default::default()
    }
}

fn save_driver_key(config_path: &Path, key: &str) -> AppResult<AssistantSettings> {
    let key = key.trim();
    if key.is_empty() || key.len() > 4096 || !key.chars().all(|value| value.is_ascii_graphic()) {
        return Err(validation("请输入有效的 DeepSeek 官网 API Key。"));
    }
    write_credentials(
        &config_path.with_file_name(DRIVER_KEY_FILE),
        &HashMap::from([("deepseek_official".to_string(), key.to_string())]),
    )?;
    Ok(AssistantSettings {
        has_credential: true,
        ..Default::default()
    })
}

#[tauri::command]
pub(crate) fn selection_get(state: State<'_, SelectionState>) -> AppResult<SelectionSession> {
    Ok(state.0.lock().map_err(|_| lock_error())?.clone())
}

#[tauri::command]
pub(crate) fn selection_reset(
    session_id: String,
    state: State<'_, SelectionState>,
) -> AppResult<SelectionSession> {
    let mut session = state.0.lock().map_err(|_| lock_error())?;
    if session.id != session_id || session.running {
        return Err(validation("当前需求正在处理或已变化，请稍后重试。"));
    }
    *session = SelectionSession::default();
    Ok(session.clone())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DriverKeyInput {
    api_key: String,
}

fn select_task(
    session: &mut SelectionSession,
    session_id: &str,
    revision: u64,
    task_id: &str,
    work_descriptions: &BTreeMap<String, String>,
) -> AppResult<SelectionSession> {
    version_check(session, session_id, revision)?;
    if session.running || session.confirmed.is_some() {
        return Err(validation("请等待当前处理完成后再选择环节。"));
    }
    let mut card = session
        .draft
        .clone()
        .filter(|card| card.tasks.len() > 1)
        .ok_or_else(|| validation("当前没有待选择的 AI 环节。"))?;
    let index = card
        .tasks
        .iter()
        .position(|task| task.id == task_id)
        .ok_or_else(|| validation("该环节不存在，请重新选择。"))?;
    for (id, work) in work_descriptions {
        let task_index = card
            .tasks
            .iter()
            .position(|task| &task.id == id)
            .ok_or_else(|| validation("工作说明对应的环节已变化，请重新选择。"))?;
        if work.chars().count() > 4000 || (id == task_id && work.trim().is_empty()) {
            return Err(validation("请填写所选环节的工作说明，最多4,000字。"));
        }
        card.tasks[task_index].fields.insert(
            "businessGoal".into(),
            RequirementField {
                value: work.clone(),
                source: if work.trim().is_empty() {
                    "missing"
                } else {
                    "user"
                }
                .into(),
                evidence: String::new(),
            },
        );
        card.suggestions
            .retain(|item| !(item.task_index == task_index && item.field == "businessGoal"));
    }
    let record = IntentSelection::Record {
        selected_task_id: task_id.to_string(),
        options: card
            .tasks
            .iter()
            .enumerate()
            .map(|(task_index, task)| {
                let goal = &task.fields["businessGoal"];
                let work = if !goal.value.trim().is_empty() || goal.source == "user" {
                    goal.value.clone()
                } else {
                    card.suggestions
                        .iter()
                        .find(|item| item.task_index == task_index && item.field == "businessGoal")
                        .map(|item| item.value.clone())
                        .unwrap_or_default()
                };
                IntentOption {
                    id: task.id.clone(),
                    name: task.name.clone(),
                    work,
                }
            })
            .collect(),
    };
    card.tasks = vec![card.tasks.remove(index)];
    card.suggestions = card
        .suggestions
        .into_iter()
        .filter(|item| item.task_index == index)
        .map(|mut item| {
            item.task_index = 0;
            item
        })
        .collect();
    validate_card(&card, false)?;
    session.revision += 1;
    session.intent_selections.push(record);
    session.draft = Some(card);
    session.progress = None;
    session.task_runs.clear();
    session.confirmation_request = None;
    session.attempt = None;
    Ok(session.clone())
}

#[tauri::command]
pub(crate) fn selection_select_task(
    session_id: String,
    revision: u64,
    task_id: String,
    work_descriptions: Option<BTreeMap<String, String>>,
    state: State<'_, SelectionState>,
) -> AppResult<SelectionSession> {
    let mut session = state.0.lock().map_err(|_| lock_error())?;
    select_task(
        &mut session,
        &session_id,
        revision,
        &task_id,
        &work_descriptions.unwrap_or_default(),
    )?;
    state.checkpoint(&mut session, true);
    Ok(session.clone())
}

#[tauri::command]
pub(crate) fn selection_assistant_set(
    input: DriverKeyInput,
    state: State<'_, AppState>,
) -> AppResult<AssistantSettings> {
    save_driver_key(&state.config_path, &input.api_key)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExtractInput {
    session_id: String,
    revision: u64,
    request_id: String,
    description: String,
}

fn validate_prefill(card: &RequirementCard) -> AppResult<()> {
    // 用户后续主动清除的字段或标签不能被默认值填回。
    if card.description.contains("本次只为以下一个 AI 环节选型") {
        return Ok(());
    }
    for (index, task) in card.tasks.iter().enumerate() {
        if task.fields["taskType"].value.is_empty() && normalize_task_type(&task.name).is_empty() {
            continue;
        }
        for key in [
            "businessGoal",
            "input",
            "output",
            "priorities",
            "taskDifficulty",
            "taskFocus",
        ] {
            if task.fields[key].value.trim().is_empty()
                && !card.suggestions.iter().any(|item| {
                    item.task_index == index && item.field == key && !item.value.trim().is_empty()
                })
            {
                return Err(validation(&format!(
                    "「{}」缺少{}的预填内容。",
                    task.name, key
                )));
            }
        }
        if task.user_concerns.is_empty() {
            return Err(validation(&format!(
                "「{}」尚未生成额外业务标签。",
                task.name
            )));
        }
    }
    Ok(())
}

async fn extract_request(
    settings: &AssistantSettings,
    connection: &Connection,
    key: String,
    description: &str,
    progress: impl Fn(&str),
    stream_progress: impl Fn(&str, &str),
    record_invocation: impl Fn(Invocation),
) -> AppResult<(RequirementCard, Invocation)> {
    let started = Instant::now();
    let mut prompt = include_str!("prompts/selection_extract.txt").to_string();
    let mut last_error = validation("需求卡预填未完成，请重试。");
    for attempt in 0..2 {
        let (output, mut invocation) = assistant_request(
            settings,
            connection,
            key.clone(),
            description,
            &prompt,
            "需求确认卡",
            &progress,
            &stream_progress,
            &record_invocation,
        )
        .await?;
        let checked = parse_card(&output.text, description).and_then(|card| {
            validate_prefill(&card)?;
            Ok(card)
        });
        match checked {
            Ok(card) => {
                invocation.elapsed_ms = started.elapsed().as_millis() as u64;
                if attempt > 0 {
                    invocation.usage = None;
                }
                return Ok((card, invocation));
            }
            Err(error) => {
                last_error = error;
                progress("正在补齐各环节的工作、输入输出、推荐优先级与业务标签…");
                prompt = format!(
                    "{}\n上次结果未通过完整性检查：{}。每个任务自己的suggestions必须有缺失的businessGoal、input、output、priorities、taskDifficulty、taskFocus，userConcerns必须包含该环节特有的业务标签。只补足缺项，不编造用户原话或性能指标。",
                    include_str!("prompts/selection_extract.txt"),
                    last_error.message
                );
            }
        }
    }
    Err(last_error)
}

async fn assistant_request(
    settings: &AssistantSettings,
    connection: &Connection,
    key: String,
    description: &str,
    system_prompt: &str,
    output_name: &str,
    progress: impl Fn(&str),
    stream_progress: impl Fn(&str, &str),
    record_invocation: impl Fn(Invocation),
) -> AppResult<(GeneratedText, Invocation)> {
    let is_card = output_name == "需求确认卡";
    let request_failure = |error: reqwest::Error| {
        if error.is_timeout() {
            AppError::new(
                "MODEL_TIMEOUT",
                format!("{output_name}连接长时间未响应，请重试。"),
                true,
            )
        } else {
            connection_error(&error, "模型选型驱动 AI")
        }
    };
    let model_id = fixed_model(connection, &settings.model_id)?;
    let (_, base) = normalize_base(&connection.base_url)?;
    let started = Instant::now();
    let mut body = chat_request_body(
        model_id,
        serde_json::Value::String(description.to_string()),
        Some(8192),
        Some(system_prompt),
    );
    stream_request_body(&mut body);
    body["response_format"] = serde_json::json!({"type":"json_object"});
    body["thinking"] = serde_json::json!({"type":"disabled"});
    progress("正在连接模型服务…");
    stream_progress("", "");
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .read_timeout(Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| validation("无法初始化 DeepSeek 官网连接。"))?;
    let request = client
        .post(chat_endpoint(base))
        .bearer_auth(key)
        .json(&body);
    // read_timeout 只限制相邻数据块，完整响应也要有总时限。
    let request = request.timeout(if is_card {
        EXTRACT_TIMEOUT
    } else {
        Duration::from_secs(120)
    });
    let mut response = request.send().await.map_err(&request_failure)?;
    let status = response.status();
    let is_stream = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("text/event-stream"));
    if status.is_success() {
        progress("服务已响应，等待模型输出…");
    }
    let mut bytes = Vec::new();
    let mut stream = ChatStreamState::default();
    let mut line_start = 0;
    let mut done = false;
    while let Some(chunk) = response.chunk().await.map_err(&request_failure)? {
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return Err(validation("模型返回内容超过处理上限，请重试。"));
        }
        bytes.extend_from_slice(&chunk);
        if status.is_success() {
            while let Some(length) = bytes[line_start..].iter().position(|byte| *byte == b'\n') {
                let line = std::str::from_utf8(&bytes[line_start..line_start + length])
                    .map_err(|_| validation("模型返回了无效的文本编码。"))?;
                line_start += length + 1;
                if let Some((content, reasoning)) = apply_stream_line(&mut stream, line, started) {
                    if stream.content.len() + stream.reasoning.len() > 1024 * 1024 {
                        return Err(validation("模型返回内容超过处理上限，请重试。"));
                    }
                    stream_progress(&content, &reasoning);
                    if !content.is_empty() {
                        progress(&if is_card {
                            card_progress_message(&stream.content)
                        } else {
                            format!("正在生成{output_name}…")
                        });
                    } else if !reasoning.is_empty() {
                        progress(if is_card {
                            "正在分析你的需求与任务划分…"
                        } else {
                            "正在对照已读取的资料筛选候选模型…"
                        });
                    }
                }
                if line.trim().strip_prefix("data:").map(str::trim) == Some("[DONE]") {
                    done = true;
                    break;
                }
            }
            if done {
                break;
            }
            if (is_stream || stream.saw_event) && line_start > 0 {
                bytes.drain(..line_start);
                line_start = 0;
            }
        }
    }
    if !status.is_success() {
        let mut error = provider_error(status, &String::from_utf8_lossy(&bytes));
        // 平台原始错误可能包含请求内容，助手仅展示已归类的安全提示。
        error.message = error
            .message
            .split(" 平台提示：")
            .next()
            .unwrap_or("助手请求失败。")
            .to_string();
        return Err(error);
    }
    if line_start < bytes.len() && !done {
        let line = std::str::from_utf8(&bytes[line_start..])
            .map_err(|_| validation("模型返回了无效的文本编码。"))?;
        if let Some((content, reasoning)) = apply_stream_line(&mut stream, line, started) {
            stream_progress(&content, &reasoning);
        }
    }
    let output = if stream.saw_event {
        if stream.content.trim().is_empty() {
            return Err(validation(&format!(
                "模型未返回{output_name}内容，请重试。"
            )));
        }
        GeneratedText {
            text: stream.content,
            reasoning: None,
            usage: stream.usage,
            first_token_ms: stream.first_token_ms,
        }
    } else {
        let response: OpenAiResponse = serde_json::from_slice(&bytes)
            .map_err(|_| AppError::new("RESPONSE_INVALID", "助手返回了无法识别的数据。", true))?;
        parse_openai_response(response)?
    };
    let usage = output.usage.clone();
    let invocation = Invocation {
        model_id: model_id.to_string(),
        connection_name: connection.display_name.clone(),
        elapsed_ms: started.elapsed().as_millis() as u64,
        first_token_ms: output.first_token_ms,
        usage,
    };
    record_invocation(invocation.clone());
    Ok((output, invocation))
}

fn card_progress_message(content: &str) -> String {
    let task_name = content.rfind("\"name\"").and_then(|index| {
        let value = content[index + 6..]
            .trim_start()
            .strip_prefix(':')?
            .trim_start();
        serde_json::Deserializer::from_str(value)
            .into_iter::<String>()
            .next()?
            .ok()
    });
    match task_name.filter(|name| !name.trim().is_empty() && name.chars().count() <= 200) {
        Some(name) => {
            let last_task = content.rfind("\"name\"").unwrap_or(0);
            let field = [
                ("taskType", "任务类型"),
                ("businessContext", "场景要求"),
                ("businessGoal", "希望 AI 完成"),
                ("input", "输入"),
                ("output", "输出"),
                ("accuracy", "准确性要求"),
                ("speed", "速度要求"),
                ("cost", "预算与用量"),
                ("constraints", "模型要求"),
                ("priorities", "业务优先级"),
                ("fallback", "兜底要求"),
            ]
            .iter()
            .filter_map(|(key, label)| {
                content[last_task..]
                    .rfind(&format!("\"{key}\""))
                    .map(|position| (position, *label))
            })
            .max_by_key(|(position, _)| *position);
            match field {
                Some((_, label)) => format!("正在填写「{name}」的{label}…"),
                None => format!("正在填写「{name}」的需求卡…"),
            }
        }
        None => "模型已开始返回，正在生成需求确认卡…".into(),
    }
}

fn begin_extraction(
    session: &mut SelectionSession,
    input: &ExtractInput,
) -> AppResult<Option<SelectionSession>> {
    if session.id == input.session_id
        && let Some(attempt) = &session.attempt
        && attempt.request_id == input.request_id
    {
        if attempt.description != input.description {
            return Err(validation("同一请求不能提交不同的需求。"));
        }
        return attempt
            .result
            .clone()
            .map(|result| result.map(|view| Some(*view)))
            .unwrap_or_else(|| Err(validation("需求正在整理，请勿重复提交。")));
    }
    version_check(&session, &input.session_id, input.revision)?;
    if session.running {
        return Err(validation("需求正在整理，请勿重复提交。"));
    }
    session.revision += 1;
    session.running = true;
    session.progress = Some(ExtractionProgress {
        message: "正在连接模型服务…".into(),
        started_at_ms: now_ms(),
        ..Default::default()
    });
    session.description = input.description.clone();
    session.attempt = Some(Attempt {
        request_id: input.request_id.clone(),
        description: input.description.clone(),
        result: None,
    });
    Ok(None)
}

#[tauri::command]
pub(crate) async fn selection_extract(
    input: ExtractInput,
    app_state: State<'_, AppState>,
    state: State<'_, SelectionState>,
) -> AppResult<SelectionSession> {
    validate_description(&input.description)?;
    if input.description.trim().is_empty() {
        return Err(validation("请先描述需求，或选择手工填写。"));
    }
    Uuid::parse_str(&input.request_id).map_err(|_| validation("请求标识无效。"))?;
    let settings = AssistantSettings::default();
    let connection = official_connection();
    let key = driver_key(&app_state.config_path)?;
    {
        let mut session = state.0.lock().map_err(|_| lock_error())?;
        if let Some(previous) = begin_extraction(&mut session, &input)? {
            return Ok(previous);
        }
        state.checkpoint(&mut session, true);
    }
    let result = tokio::time::timeout(
        EXTRACT_TIMEOUT,
        extract_request(
            &settings,
            &connection,
            key,
            &input.description,
            |message| {
                if let Ok(mut session) = state.0.lock()
                    && session.id == input.session_id
                    && session.running
                    && session.attempt.as_ref().map(|attempt| &attempt.request_id)
                        == Some(&input.request_id)
                    && let Some(progress) = &mut session.progress
                {
                    progress.message = message.to_string();
                }
            },
            |content, reasoning| {
                if let Ok(mut session) = state.0.lock()
                    && session.id == input.session_id
                    && session.running
                    && session.attempt.as_ref().map(|attempt| &attempt.request_id)
                        == Some(&input.request_id)
                    && let Some(progress) = &mut session.progress
                {
                    update_stream_progress(progress, content, reasoning);
                }
            },
            |invocation| {
                if let Ok(mut session) = state.0.lock()
                    && session.id == input.session_id
                    && session.running
                    && session.attempt.as_ref().map(|attempt| &attempt.request_id)
                        == Some(&input.request_id)
                {
                    session.metrics.record(invocation);
                }
            },
        ),
    )
    .await
    .unwrap_or_else(|_| {
        Err(validation(
            "需求识别超过 60 秒，已停止等待，原始描述已保留，请重试。",
        ))
    });
    let mut session = state.0.lock().map_err(|_| lock_error())?;
    if session.id != input.session_id {
        return Err(validation("需求会话已变化，本次结果未应用。"));
    }
    let finished = finish_extraction(&mut session, result);
    state.checkpoint(&mut session, finished.is_ok());
    finished.map(|_| session.clone())
}

fn finish_extraction(
    session: &mut SelectionSession,
    result: AppResult<(RequirementCard, Invocation)>,
) -> AppResult<SelectionSession> {
    session.running = false;
    let finished = match result {
        Ok((card, invocation)) => {
            session.draft = Some(card);
            session.task_runs.clear();
            session.confirmed = None;
            session.confirmation_request = None;
            session.invocation = Some(invocation);
            let mut view = session.clone();
            view.attempt = None;
            Ok(view)
        }
        Err(error) => Err(error),
    };
    if let Some(attempt) = &mut session.attempt {
        attempt.result = Some(finished.clone().map(Box::new));
    }
    finished
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConfirmInput {
    session_id: String,
    revision: u64,
    request_id: String,
    card: RequirementCard,
}

fn confirm(mut input: ConfirmInput, session: &mut SelectionSession) -> AppResult<SelectionSession> {
    Uuid::parse_str(&input.request_id).map_err(|_| validation("请求标识无效。"))?;
    if input.card.tasks.len() != 1 {
        return Err(validation("请先选择一个 AI 环节，再确认需求卡。"));
    }
    migrate_legacy_io(&mut input.card);
    validate_card(&input.card, false)?;
    normalize_context(&mut input.card);
    if session.id == input.session_id
        && session.confirmation_request.as_ref() == Some(&input.request_id)
    {
        if session.confirmed.as_ref().map(|confirmed| &confirmed.card) != Some(&input.card) {
            return Err(validation("同一确认请求不能包含不同的需求。"));
        }
        return Ok(session.clone());
    }
    version_check(session, &input.session_id, input.revision)?;
    if session.running {
        return Err(validation("请等待需求整理完成后再确认。"));
    }
    session.revision += 1;
    session.description = input.card.description.clone();
    session.draft = Some(input.card.clone());
    session.confirmed = Some(ConfirmedCard {
        card: input.card,
        revision: session.revision,
        confirmed_at_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    });
    session.task_runs.clear();
    session.confirmation_request = Some(input.request_id);
    Ok(session.clone())
}

#[tauri::command]
pub(crate) fn selection_confirm(
    input: ConfirmInput,
    state: State<'_, SelectionState>,
) -> AppResult<SelectionSession> {
    let mut session = state.0.lock().map_err(|_| lock_error())?;
    confirm(input, &mut session)?;
    state.checkpoint(&mut session, true);
    Ok(session.clone())
}

fn task_card(session: &SelectionSession, task_id: &str) -> AppResult<RequirementCard> {
    let card = &session
        .confirmed
        .as_ref()
        .filter(|confirmed| confirmed.revision == session.revision)
        .ok_or_else(|| validation("请先确认当前需求卡。"))?
        .card;
    if card.tasks.len() != 1 || card.tasks[0].id != task_id {
        return Err(validation("只能调研本次已选择的一个 AI 环节。"));
    }
    let mut current = card.clone();
    current.suggestions.clear();
    let excluded = std::mem::take(&mut current.tasks[0].excluded_user_concerns);
    current.tasks[0]
        .user_concerns
        .retain(|label| !excluded.contains(label));
    let priority = current.tasks[0].fields.get_mut("priorities").unwrap();
    if priority_values(&priority.value).is_some_and(|values| values.len() == 3) {
        *priority = RequirementField {
            value: String::new(),
            source: "missing".into(),
            evidence: String::new(),
        };
    }
    Ok(current)
}

fn choose_model(
    session: &mut SelectionSession,
    task_id: &str,
    model: &str,
    role: &str,
) -> AppResult<()> {
    if !["main", "fallback"].contains(&role) {
        return Err(validation("请选择主力模型或兜底模型。"));
    }
    if session.running {
        return Err(validation("请等待当前调研完成。"));
    }
    task_card(session, task_id)?;
    let run = session
        .task_runs
        .get_mut(task_id)
        .ok_or_else(|| validation("请先完成该环节的调研。"))?;
    if !run.research.as_ref().is_some_and(|result| {
        result.revision == session.revision
            && ["complete", "partial"].contains(&result.status.as_str())
            && result
                .report
                .as_ref()
                .is_some_and(|report| report.has_candidate(task_id, model))
    }) {
        return Err(validation("请选择当前环节调研结果中的候选模型。"));
    }
    let other = if role == "main" {
        &run.fallback_model
    } else {
        &run.selected_model
    };
    if other.as_deref() == Some(model) {
        return Err(validation("主力模型和兜底模型不能相同。"));
    }
    if role == "main" {
        run.selected_model = Some(model.to_string());
    } else {
        run.fallback_model = Some(model.to_string());
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn selection_choose(
    session_id: String,
    revision: u64,
    task_id: String,
    model: String,
    role: Option<String>,
    state: State<'_, SelectionState>,
) -> AppResult<SelectionSession> {
    let mut session = state.0.lock().map_err(|_| lock_error())?;
    version_check(&session, &session_id, revision)?;
    choose_model(
        &mut session,
        &task_id,
        &model,
        role.as_deref().unwrap_or("main"),
    )?;
    Ok(session.clone())
}

#[tauri::command]
pub(crate) async fn research_command(
    session_id: String,
    revision: u64,
    task_id: String,
    retry: bool,
    as_of_date: String,
    more: Option<bool>,
    app_state: State<'_, AppState>,
    state: State<'_, SelectionState>,
) -> AppResult<SelectionSession> {
    if as_of_date.len() != 10
        || as_of_date.bytes().enumerate().any(|(index, byte)| {
            if index == 4 || index == 7 {
                byte != b'-'
            } else {
                !byte.is_ascii_digit()
            }
        })
    {
        return Err(validation("检索日期格式无效，请重试。"));
    }
    let settings = AssistantSettings::default();
    let connection = official_connection();
    let key = driver_key(&app_state.config_path)?;
    let adding = more.unwrap_or(false);
    let (card, excluded_models) = {
        let mut session = state.0.lock().map_err(|_| lock_error())?;
        version_check(&session, &session_id, revision)?;
        if adding
            && session
                .task_runs
                .get(&task_id)
                .and_then(|run| run.research.as_ref())
                .is_some_and(|result| result.more_requested)
        {
            return Ok(session.clone());
        }
        if session.running {
            return Err(validation("当前任务正在执行，请勿重复提交。"));
        }
        let card = task_card(&session, &task_id)?;
        if !adding
            && !retry
            && session
                .task_runs
                .get(&task_id)
                .and_then(|run| run.research.as_ref())
                .is_some_and(|result| {
                    result.revision == revision
                        && result.resource_policy_version == research::RESOURCE_POLICY_VERSION
                        && result.as_of_date == as_of_date
                        && ["complete", "partial"].contains(&result.status.as_str())
                })
        {
            return Ok(session.clone());
        }
        let excluded_models = if adding {
            request_more_candidates(&mut session, &task_id)?
        } else {
            Vec::new()
        };
        session.running = true;
        session.progress = Some(ExtractionProgress {
            message: if adding {
                "正在查找另外 5 个适合的模型…"
            } else {
                "正在核对当前官方模型目录…"
            }
            .into(),
            started_at_ms: now_ms(),
            ..Default::default()
        });
        if !adding {
            let run = session.task_runs.entry(task_id.clone()).or_default();
            let previous = run.research.take();
            run.research = Some(research::ResearchResult {
                status: "running".into(),
                revision,
                researched_at_ms: now_ms(),
                as_of_date: as_of_date.clone(),
                sources: previous.map(|result| result.sources).unwrap_or_default(),
                report: None,
                error: String::new(),
                more_requested: false,
                more_added_count: 0,
                more_error: String::new(),
                needs_ratings: false,
                rating_attempted: false,
                rating_error: String::new(),
                rating_version: 1,
                resource_policy_version: research::RESOURCE_POLICY_VERSION,
            });
        }
        state.checkpoint(&mut session, false);
        if adding && !session.storage_error.is_empty() {
            session.running = false;
            return Err(validation(&session.storage_error));
        }
        (card, excluded_models)
    };
    let update_progress = |message: &str| {
        if let Ok(mut session) = state.0.lock()
            && session.id == session_id
            && session.revision == revision
            && session.running
            && let Some(progress) = &mut session.progress
        {
            progress.message = message.to_string();
        }
    };
    let save_sources = |sources| {
        if !adding
            && let Ok(mut session) = state.0.lock()
            && session.id == session_id
            && session.revision == revision
            && session.running
            && let Some(result) = session
                .task_runs
                .get_mut(&task_id)
                .and_then(|run| run.research.as_mut())
        {
            result.sources = sources;
        }
    };
    let stream_progress = |content: &str, reasoning: &str| {
        if let Ok(mut session) = state.0.lock()
            && session.id == session_id
            && session.revision == revision
            && session.running
            && let Some(progress) = &mut session.progress
        {
            update_stream_progress(progress, content, reasoning);
        }
    };
    let result = research::run(
        &card,
        &as_of_date,
        &settings,
        &connection,
        key,
        &excluded_models,
        update_progress,
        save_sources,
        stream_progress,
        |invocation| {
            if let Ok(mut session) = state.0.lock()
                && session.id == session_id
                && session.revision == revision
                && session.running
            {
                session.metrics.record(invocation);
            }
        },
    )
    .await;
    let mut session = state.0.lock().map_err(|_| lock_error())?;
    version_check(&session, &session_id, revision)?;
    session.running = false;
    if let Some(run) = session.task_runs.get_mut(&task_id)
        && let Some(research) = &mut run.research
    {
        match result {
            Ok((report, sources)) if adding => {
                if let Some(existing) = &mut research.report {
                    research.more_added_count = existing.append_candidates(report);
                    research.sources.extend(sources);
                    research.needs_ratings = existing.needs_ratings();
                    research.rating_attempted = false;
                    if research.more_added_count == 0 {
                        research.more_error = "本次未找到其他不同型号，原有推荐已保留。".into();
                    }
                }
            }
            Ok((mut report, sources)) => {
                report.assign_recommendations();
                (run.selected_model, run.fallback_model) = report.recommended_pair(&task_id);
                research.status = research::status(&report, &sources);
                research.needs_ratings = report.needs_ratings();
                research.report = Some(report);
                research.sources = sources;
            }
            Err(error) if adding => {
                research.more_error = error.message;
            }
            Err(error) => {
                research.status = "failed".into();
                research.error = error.message;
            }
        }
    }
    state.checkpoint(&mut session, false);
    Ok(session.clone())
}

#[tauri::command]
pub(crate) async fn selection_rate(
    session_id: String,
    revision: u64,
    task_id: String,
    retry: bool,
    app_state: State<'_, AppState>,
    state: State<'_, SelectionState>,
) -> AppResult<SelectionSession> {
    let settings = AssistantSettings::default();
    let connection = official_connection();
    let key = driver_key(&app_state.config_path)?;
    let material = {
        let mut session = state.0.lock().map_err(|_| lock_error())?;
        version_check(&session, &session_id, revision)?;
        let card = task_card(&session, &task_id)?;
        let result = session
            .task_runs
            .get(&task_id)
            .and_then(|run| run.research.as_ref())
            .ok_or_else(|| validation("请先生成推荐名单。"))?;
        if !result.needs_ratings || (result.rating_attempted && !retry) {
            return Ok(session.clone());
        }
        let models = result
            .report
            .as_ref()
            .ok_or_else(|| validation("请先生成推荐名单。"))?
            .rating_material();
        let material =
            serde_json::json!({"requirements":card,"groups":models,"sources":result.sources})
                .to_string();
        if session.running {
            return Err(validation("请等待当前请求完成。"));
        }
        let result = session
            .task_runs
            .get_mut(&task_id)
            .unwrap()
            .research
            .as_mut()
            .unwrap();
        result.rating_attempted = true;
        result.rating_version = 1;
        result.rating_error.clear();
        session.running = true;
        session.progress = Some(ExtractionProgress {
            message: "正在按需求卡重新评定推荐星级…".into(),
            started_at_ms: now_ms(),
            ..Default::default()
        });
        state.checkpoint(&mut session, false);
        if !session.storage_error.is_empty() {
            session.running = false;
            return Err(validation(&session.storage_error));
        }
        material
    };
    let prompt = include_str!("prompts/selection_rating.txt");
    let generated = assistant_request(
        &settings,
        &connection,
        key,
        &material,
        prompt,
        "推荐星级",
        |message| {
            if let Ok(mut session) = state.0.lock()
                && session.id == session_id
                && session.revision == revision
                && let Some(progress) = &mut session.progress
            {
                progress.message = message.into();
            }
        },
        |content, reasoning| {
            if let Ok(mut session) = state.0.lock()
                && session.id == session_id
                && session.revision == revision
                && let Some(progress) = &mut session.progress
            {
                update_stream_progress(progress, content, reasoning);
            }
        },
        |invocation| {
            if let Ok(mut session) = state.0.lock()
                && session.id == session_id
                && session.revision == revision
            {
                session.metrics.record(invocation);
            }
        },
    )
    .await;
    let mut session = state.0.lock().map_err(|_| lock_error())?;
    version_check(&session, &session_id, revision)?;
    session.running = false;
    if let Some(result) = session
        .task_runs
        .get_mut(&task_id)
        .and_then(|run| run.research.as_mut())
    {
        let graded = generated.and_then(|(output, _)| {
            result
                .report
                .as_mut()
                .ok_or_else(|| validation("原推荐名单不存在。"))?
                .apply_ratings(&output.text)
        });
        match graded {
            Ok(()) => {
                result.needs_ratings = false;
                result.rating_error.clear();
            }
            Err(error) => {
                result.rating_error = error.message;
            }
        }
    }
    state.checkpoint(&mut session, false);
    Ok(session.clone())
}

fn request_more_candidates(
    session: &mut SelectionSession,
    task_id: &str,
) -> AppResult<Vec<String>> {
    let result = session
        .task_runs
        .get_mut(task_id)
        .and_then(|run| run.research.as_mut())
        .ok_or_else(|| validation("请先生成推荐名单。"))?;
    if result.more_requested {
        return Err(validation("本轮推荐只能补充一次。"));
    }
    if result.revision != session.revision
        || !["complete", "partial"].contains(&result.status.as_str())
    {
        return Err(validation("请先完成本轮推荐。"));
    }
    let models = result
        .report
        .as_ref()
        .map(|report| report.model_names())
        .filter(|models| !models.is_empty())
        .ok_or_else(|| validation("请先生成推荐名单。"))?;
    result.more_requested = true;
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn card() -> RequirementCard {
        RequirementCard {
            description: "会议录音转文字，预算未定".into(),
            exclude_overseas: false,
            project_context: None,
            usage_scenario: None,
            target_user: None,
            tasks: vec![RequirementTask {
                id: "task".into(),
                name: "语音转写".into(),
                user_concerns: vec![],
                excluded_user_concerns: vec![],
                fields: FIELD_KEYS
                    .iter()
                    .map(|key| {
                        (
                            key.to_string(),
                            RequirementField {
                                value: if *key == "taskType" {
                                    "语音转写".into()
                                } else {
                                    String::new()
                                },
                                source: if *key == "taskType" {
                                    "provided".into()
                                } else {
                                    "missing".into()
                                },
                                evidence: if *key == "taskType" {
                                    "录音转文字".into()
                                } else {
                                    String::new()
                                },
                            },
                        )
                    })
                    .collect(),
            }],
            suggestions: vec![],
        }
    }

    #[test]
    fn keeps_inferred_user_concerns_and_accepts_legacy_cards_without_them() {
        let mut current = card();
        current.tasks[0].user_concerns = vec!["少反复沟通".into(), "补齐遗漏信息".into()];
        assert!(validate_card(&current, false).is_ok());
        let mut value = serde_json::to_value(&current).unwrap();
        assert_eq!(value["tasks"][0]["userConcerns"][0], "少反复沟通");
        value["tasks"][0]
            .as_object_mut()
            .unwrap()
            .remove("userConcerns");
        let legacy: RequirementCard = serde_json::from_value(value).unwrap();
        assert!(legacy.tasks[0].user_concerns.is_empty());
        current.tasks[0].user_concerns.push("不允许\n换行".into());
        assert!(validate_card(&current, false).is_err());
    }

    #[test]
    fn researches_only_selected_business_tags_and_preserves_every_choice_for_editing() {
        let mut current = card();
        current.tasks[0].user_concerns = vec!["缺项主动追问".into(), "结构化字段输出".into()];
        current.tasks[0].excluded_user_concerns = vec!["缺项主动追问".into()];
        let mut session = SelectionSession::default();
        confirm(
            ConfirmInput {
                session_id: session.id.clone(),
                revision: 0,
                request_id: Uuid::new_v4().to_string(),
                card: current.clone(),
            },
            &mut session,
        )
        .unwrap();
        let effective = task_card(&session, "task").unwrap();
        assert_eq!(effective.tasks[0].user_concerns, vec!["结构化字段输出"]);
        assert!(effective.tasks[0].excluded_user_concerns.is_empty());
        assert_eq!(
            session.confirmed.as_ref().unwrap().card.tasks[0]
                .user_concerns
                .len(),
            2
        );
        assert_eq!(
            session.confirmed.as_ref().unwrap().card.tasks[0].excluded_user_concerns,
            vec!["缺项主动追问"]
        );
        session.confirmed.as_mut().unwrap().card.tasks[0].excluded_user_concerns =
            current.tasks[0].user_concerns.clone();
        assert!(
            task_card(&session, "task").unwrap().tasks[0]
                .user_concerns
                .is_empty()
        );
        current.tasks[0]
            .excluded_user_concerns
            .push("不存在的标签".into());
        assert!(validate_card(&current, false).is_err());
    }

    #[test]
    fn narrows_multiple_intents_before_confirmation_and_research() {
        let mut pending = card();
        let mut second = pending.tasks[0].clone();
        second.id = "second".into();
        second.name = "整理摘要".into();
        pending.tasks.push(second);
        pending.project_context = Some(RequirementField {
            value: "会议录音".into(),
            source: "provided".into(),
            evidence: "会议录音".into(),
        });
        for index in 0..2 {
            pending.suggestions.push(Suggestion {
                id: format!("suggestion-{index}"),
                task_index: index,
                field: "output".into(),
                value: format!("输出 {index}"),
                reason: "建议".into(),
                accepted: false,
            });
        }
        for task_index in 0..2 {
            pending.suggestions.push(Suggestion {
                id: format!("work-{task_index}"),
                task_index,
                field: "businessGoal".into(),
                value: format!("工作说明 {task_index}"),
                reason: "建议".into(),
                accepted: false,
            });
        }
        validate_card(&pending, true).unwrap();
        let mut session = SelectionSession {
            draft: Some(pending.clone()),
            ..Default::default()
        };
        let id = session.id.clone();
        assert!(
            confirm(
                ConfirmInput {
                    session_id: id.clone(),
                    revision: 0,
                    request_id: Uuid::new_v4().to_string(),
                    card: pending,
                },
                &mut session
            )
            .is_err()
        );
        assert_eq!(session.revision, 0);
        assert!(session.confirmed.is_none());
        for edits in [
            BTreeMap::from([("second".into(), String::new())]),
            BTreeMap::from([("missing".into(), "说明".into())]),
            BTreeMap::from([("second".into(), "字".repeat(4001))]),
        ] {
            assert!(select_task(&mut session, &id, 0, "second", &edits).is_err());
            assert_eq!(session.revision, 0);
            assert_eq!(session.draft.as_ref().unwrap().tasks.len(), 2);
        }
        let edited_work = "仅提取会议结论，不生成摘要";
        let chosen = select_task(
            &mut session,
            &id,
            0,
            "second",
            &BTreeMap::from([("second".into(), edited_work.into())]),
        )
        .unwrap();
        let selected = chosen.draft.unwrap();
        assert_eq!(selected.tasks.len(), 1);
        assert_eq!(selected.tasks[0].id, "second");
        assert_eq!(selected.project_context.unwrap().value, "会议录音");
        assert_eq!(selected.suggestions.len(), 1);
        assert_eq!(selected.tasks[0].fields["businessGoal"].value, edited_work);
        assert_eq!(selected.tasks[0].fields["businessGoal"].source, "user");
        assert_eq!(selected.suggestions[0].id, "suggestion-1");
        assert_eq!(selected.suggestions[0].task_index, 0);
        assert_eq!(session.revision, 1);
        let saved_choices = serde_json::to_value(&session.intent_selections).unwrap();
        assert_eq!(saved_choices[0]["selectedTaskId"], "second");
        assert_eq!(saved_choices[0]["options"].as_array().unwrap().len(), 2);
        assert_eq!(saved_choices[0]["options"][1]["name"], "整理摘要");
        assert_eq!(saved_choices[0]["options"][0]["work"], "工作说明 0");
        assert_eq!(saved_choices[0]["options"][1]["work"], edited_work);
        assert!(session.task_runs.is_empty());
        assert!(session.confirmed.is_none());
        confirm(
            ConfirmInput {
                session_id: id,
                revision: 1,
                request_id: Uuid::new_v4().to_string(),
                card: session.draft.clone().unwrap(),
            },
            &mut session,
        )
        .unwrap();
        assert!(task_card(&session, "task").is_err());
        assert_eq!(task_card(&session, "second").unwrap().tasks.len(), 1);
        assert_eq!(
            task_card(&session, "second").unwrap().tasks[0].fields["businessGoal"].value,
            edited_work
        );
        assert_eq!(
            serde_json::to_value(&session.intent_selections).unwrap(),
            saved_choices
        );
        let restored: SelectionSession =
            serde_json::from_value(serde_json::to_value(&session).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(restored.intent_selections).unwrap(),
            saved_choices
        );
        assert!(matches!(
            serde_json::from_value::<IntentSelection>(serde_json::json!("整理摘要")).unwrap(),
            IntentSelection::Legacy(_)
        ));
    }

    #[test]
    fn rejects_stale_missing_or_busy_intent_selection_without_losing_choices() {
        let mut pending = card();
        let mut second = pending.tasks[0].clone();
        second.id = "second".into();
        pending.tasks.push(second);
        let mut session = SelectionSession {
            draft: Some(pending),
            revision: 2,
            ..Default::default()
        };
        let id = session.id.clone();
        for (session_id, revision, task_id) in [
            (&id[..], 1, "task"),
            ("other", 2, "task"),
            (&id[..], 2, "unknown"),
        ] {
            let before = serde_json::to_value(&session).unwrap();
            assert!(
                select_task(
                    &mut session,
                    session_id,
                    revision,
                    task_id,
                    &BTreeMap::new()
                )
                .is_err()
            );
            assert_eq!(serde_json::to_value(&session).unwrap(), before);
        }
        session.running = true;
        assert!(select_task(&mut session, &id, 2, "task", &BTreeMap::new()).is_err());
        assert_eq!(session.draft.as_ref().unwrap().tasks.len(), 2);
        session.running = false;
        select_task(&mut session, &id, 2, "task", &BTreeMap::new()).unwrap();
        assert!(select_task(&mut session, &id, 3, "second", &BTreeMap::new()).is_err());
        assert_eq!(session.draft.as_ref().unwrap().tasks[0].id, "task");
    }

    #[test]
    fn migrates_legacy_io_without_losing_values_or_suggestions() {
        for (value, input, output) in [
            ("输入：教师描述；输出：HTML 课件", "教师描述", "HTML 课件"),
            ("输入：录音; 输出: 文字摘要", "录音", "文字摘要"),
            ("输出分条摘要", "", "分条摘要"),
            ("录音文件", "录音文件", ""),
        ] {
            let mut legacy = card();
            legacy.tasks[0].fields.remove("input");
            legacy.tasks[0].fields.remove("output");
            legacy.tasks[0].fields.insert(
                "inputOutput".into(),
                RequirementField {
                    value: value.into(),
                    source: "user".into(),
                    evidence: String::new(),
                },
            );
            legacy.suggestions.push(Suggestion {
                id: "legacy".into(),
                task_index: 0,
                field: "inputOutput".into(),
                value: value.into(),
                reason: "旧建议".into(),
                accepted: true,
            });
            migrate_legacy_io(&mut legacy);
            assert_eq!(legacy.tasks[0].fields["input"].value, input);
            assert_eq!(legacy.tasks[0].fields["output"].value, output);
            assert!(!legacy.tasks[0].fields.contains_key("inputOutput"));
            validate_card(&legacy, false).unwrap();
        }
    }

    #[test]
    fn uses_only_fixed_task_types_and_migrates_business_stage_names() {
        assert_eq!(normalize_task_type("意图识别、填槽、文本生成"), "文生文");
        assert_eq!(
            normalize_task_type("互动课件生成、HTML 课件拼接、代码生成"),
            "代码生成"
        );
        assert_eq!(
            normalize_task_type("图片生成、图片理解"),
            "文生图、图片理解"
        );
        assert_eq!(normalize_task_type("新造的类型"), "");
        let mut input = card();
        input.tasks[0].fields.get_mut("taskType").unwrap().value = "新造的类型".into();
        normalize_context(&mut input);
        assert_eq!(input.tasks[0].fields["taskType"].source, "missing");
        assert_eq!(input.tasks[0].fields["taskType"].evidence, "");
    }

    #[test]
    fn removes_generic_ai_statements_but_keeps_real_constraints() {
        let mut input = card();
        for (value, expected) in [
            ("需要使用 AI", ""),
            ("语音转写需使用 AI", ""),
            ("需要使用 AI；学生数据不得出境", "学生数据不得出境"),
            (
                "必须使用国产 AI；支持私有化部署",
                "必须使用国产 AI；支持私有化部署",
            ),
        ] {
            input.tasks[0].fields.insert(
                "constraints".into(),
                RequirementField {
                    value: value.into(),
                    source: "user".into(),
                    evidence: String::new(),
                },
            );
            normalize_context(&mut input);
            assert_eq!(input.tasks[0].fields["constraints"].value, expected);
            assert_eq!(
                input.tasks[0].fields["constraints"].source,
                if expected.is_empty() {
                    "missing"
                } else {
                    "user"
                }
            );
        }
    }

    #[test]
    fn tracks_only_received_analysis_and_bounds_the_live_preview() {
        let mut progress = ExtractionProgress::default();
        update_stream_progress(&mut progress, "", "");
        assert!(progress.model_started_at_ms.is_some());
        assert!(progress.last_output_at_ms.is_none());
        update_stream_progress(&mut progress, "", "先识别使用场景。");
        update_stream_progress(&mut progress, "", "再拆分 AI 环节。");
        assert_eq!(
            progress.analysis_preview,
            "先识别使用场景。再拆分 AI 环节。"
        );
        let analysis_chars = progress.received_chars;
        update_stream_progress(&mut progress, "{\"tasks\":[", "");
        assert_eq!(progress.received_chars, analysis_chars + 10);
        assert!(!progress.analysis_preview.contains("tasks"));
        assert!(progress.last_output_at_ms.is_some());
        update_stream_progress(&mut progress, "", &"中".repeat(1000));
        assert_eq!(progress.analysis_preview.chars().count(), 800);
        assert!(progress.analysis_preview.chars().all(|value| value == '中'));
        assert!(
            card_progress_message("{\"tasks\":[{\"name\":\"课时编排\",\"fields\":{\"output\":")
                .contains("课时编排」的输出")
        );
        update_stream_progress(&mut progress, "", "");
        assert!(progress.analysis_preview.is_empty());
        assert_eq!(progress.received_chars, 0);
    }

    #[test]
    fn saves_shared_context_once_and_keeps_task_specific_context() {
        let mut input = card();
        let context = RequirementField {
            value: "会议录音".into(),
            source: "provided".into(),
            evidence: "会议录音".into(),
        };
        input.tasks[0]
            .fields
            .insert("businessContext".into(), context.clone());
        let parsed =
            parse_card(&serde_json::to_string(&input).unwrap(), &input.description).unwrap();
        assert_eq!(parsed.project_context, Some(context));
        assert!(
            parsed
                .tasks
                .iter()
                .all(|task| task.fields["businessContext"].value.is_empty())
        );
        validate_card(&parsed, true).unwrap();
        let mut invalid = parsed;
        invalid.project_context.as_mut().unwrap().evidence = "不存在的背景".into();
        assert!(validate_card(&invalid, true).is_err());
    }

    #[test]
    fn keeps_background_scenario_and_end_user_separate_and_checks_their_evidence() {
        let mut input = card();
        input.description.push_str("；K12教育业务，教师线下备课");
        let provided = |value: &str| RequirementField {
            value: value.into(),
            source: "provided".into(),
            evidence: value.into(),
        };
        input.project_context = Some(provided("K12教育业务"));
        input.usage_scenario = Some(provided("线下备课"));
        input.target_user = Some(provided("教师"));
        let mut parsed =
            parse_card(&serde_json::to_string(&input).unwrap(), &input.description).unwrap();
        validate_card(&parsed, true).unwrap();
        assert_eq!(
            parsed.project_context.as_ref().unwrap().value,
            "K12教育业务"
        );
        assert_eq!(parsed.usage_scenario.as_ref().unwrap().value, "线下备课");
        assert_eq!(parsed.target_user.as_ref().unwrap().value, "教师");
        parsed.usage_scenario.as_mut().unwrap().evidence = "未提及的场景".into();
        assert!(validate_card(&parsed, true).is_err());
        parsed.usage_scenario = input.usage_scenario;
        parsed.target_user.as_mut().unwrap().evidence = "未提及的用户".into();
        assert!(validate_card(&parsed, true).is_err());
        let mut legacy = serde_json::to_value(card()).unwrap();
        legacy.as_object_mut().unwrap().remove("usageScenario");
        legacy.as_object_mut().unwrap().remove("targetUser");
        let legacy: RequirementCard = serde_json::from_value(legacy).unwrap();
        assert!(legacy.usage_scenario.is_none());
        assert!(legacy.target_user.is_none());
    }

    #[test]
    fn accepts_only_the_single_selected_tasks_verified_candidates() {
        let mut session = SelectionSession::default();
        let mut requirements = card();
        requirements.project_context = Some(RequirementField {
            value: "会议录音".into(),
            source: "provided".into(),
            evidence: "会议录音".into(),
        });
        confirm(
            ConfirmInput {
                session_id: session.id.clone(),
                revision: 0,
                request_id: Uuid::new_v4().to_string(),
                card: requirements,
            },
            &mut session,
        )
        .unwrap();
        assert_eq!(task_card(&session, "task").unwrap().tasks.len(), 1);
        assert_eq!(
            task_card(&session, "task")
                .unwrap()
                .project_context
                .as_ref()
                .unwrap()
                .value,
            "会议录音"
        );
        assert!(task_card(&session, "second").is_err());
        assert!(task_card(&session, "unknown").is_err());
        assert!(choose_model(&mut session, "task", "Model X", "main").is_err());
        let report = serde_json::from_value(serde_json::json!({"findings":[],"groups":[{"taskId":"task","candidates":[{"model":"Model X","role":"main","reason":"test","evidence":[],"test":"test","gaps":[]}],"gaps":[]}],"questions":[]})).unwrap();
        session.task_runs.insert(
            "task".into(),
            TaskRun {
                selected_model: None,
                fallback_model: None,
                research: Some(research::ResearchResult {
                    status: "partial".into(),
                    revision: session.revision,
                    researched_at_ms: now_ms(),
                    as_of_date: "2026-10-22".into(),
                    sources: vec![],
                    report: Some(report),
                    error: String::new(),
                    more_requested: false,
                    more_added_count: 0,
                    more_error: String::new(),
                    needs_ratings: false,
                    rating_attempted: false,
                    rating_error: String::new(),
                    rating_version: 1,
                    resource_policy_version: research::RESOURCE_POLICY_VERSION,
                }),
            },
        );
        assert!(choose_model(&mut session, "task", "invented", "main").is_err());
        session.running = true;
        assert!(choose_model(&mut session, "task", "Model X", "main").is_err());
        session.running = false;
        choose_model(&mut session, "task", "Model X", "main").unwrap();
        choose_model(&mut session, "task", "Model X", "main").unwrap();
        assert!(choose_model(&mut session, "task", "Model X", "fallback").is_err());
        assert!(choose_model(&mut session, "task", "Model X", "invalid").is_err());
        assert!(task_card(&session, "second").is_err());
        assert_eq!(
            session.task_runs["task"].selected_model.as_deref(),
            Some("Model X")
        );
        let confirmed = session.confirmed.as_ref().unwrap().card.clone();
        confirm(
            ConfirmInput {
                session_id: session.id.clone(),
                revision: session.revision,
                request_id: Uuid::new_v4().to_string(),
                card: confirmed,
            },
            &mut session,
        )
        .unwrap();
        assert!(session.task_runs.is_empty());
        assert!(task_card(&session, "second").is_err());
    }

    #[test]
    fn assigns_compact_suggestions_to_each_task_without_model_generated_indexes() {
        let compact = serde_json::json!({
            "tasks": (["意图识别", "课时编排"].iter().map(|name| serde_json::json!({
                "name":name,
                "fields":{"taskType":{"value":"文生文","source":"provided","evidence":name}},
                "suggestions":[
                    {"field":"businessGoal","value":format!("完成{name}"),"reason":""},
                    {"field":"input","value":format!("{name}所需描述"),"reason":""},
                    {"field":"output","value":format!("{name}的结构化结果"),"reason":""},
                    {"field":"priorities","value":"准确性","reason":"教师需要正确的备课结果。"}
                ]
            })).collect::<Vec<_>>())
        });
        let parsed = parse_card(&compact.to_string(), "教师备课需要意图识别、课时编排").unwrap();
        assert_eq!(parsed.suggestions.len(), 8);
        for (index, task) in parsed.tasks.iter().enumerate() {
            let suggestions: Vec<_> = parsed
                .suggestions
                .iter()
                .filter(|item| item.task_index == index)
                .collect();
            assert_eq!(suggestions.len(), 4);
            assert!(
                suggestions
                    .iter()
                    .any(|item| item.field == "businessGoal" && item.value.contains(&task.name))
            );
            assert!(
                suggestions
                    .iter()
                    .any(|item| item.field == "input" && item.value.contains(&task.name))
            );
            assert!(
                suggestions
                    .iter()
                    .any(|item| item.field == "output" && item.value.contains(&task.name))
            );
            assert!(suggestions.iter().any(|item| item.field == "priorities"));
        }
    }

    #[test]
    fn expands_compact_extraction_without_weakening_field_validation() {
        let compact = serde_json::json!({
            "excludeOverseas": true,
            "tasks": [{"name":"会议转写","fields":{"taskType":{"value":"语音转写","source":"provided","evidence":"录音转文字"}}}],
            "suggestions":[{"taskIndex":0,"field":"output","value":"转写文本","reason":""},{"taskIndex":0,"field":"priorities","value":"速度","reason":"会议结束后需要及时整理，优先响应速度。"}]
        });
        let parsed = parse_card(&compact.to_string(), "会议录音转文字，预算未定").unwrap();
        assert_eq!(parsed.tasks[0].fields.len(), FIELD_KEYS.len());
        assert_eq!(parsed.tasks[0].fields["cost"].source, "missing");
        assert!(parsed.tasks[0].fields["cost"].value.is_empty());
        assert_eq!(parsed.suggestions.len(), 2);
        assert!(parsed.exclude_overseas);
        let mut incomplete_confirmation = parsed.clone();
        incomplete_confirmation.tasks[0].fields.remove("output");
        assert!(validate_card(&incomplete_confirmation, false).is_err());
        let mut invalid = compact.clone();
        invalid["tasks"][0]["fields"]["taskType"]["evidence"] =
            serde_json::json!("不存在的用户原话");
        assert!(parse_card(&invalid.to_string(), "会议录音转文字").is_err());
        let mut invalid = compact;
        invalid["tasks"][0]["fields"]["invented"] =
            serde_json::json!({"value":"","source":"missing","evidence":""});
        assert!(parse_card(&invalid.to_string(), "会议录音转文字").is_err());
    }

    #[test]
    fn parses_fenced_json_and_rejects_invented_evidence() {
        let text = serde_json::to_string(&card()).unwrap();
        let parsed =
            parse_card(&format!("```json\n{text}\n```"), "会议录音转文字，预算未定").unwrap();
        assert_eq!(parsed.tasks[0].fields["cost"].source, "missing");
        assert_ne!(parsed.tasks[0].id, "task");
        assert!(parse_card(&text, "另一个需求").is_err());
        let mut wrong = card();
        wrong.tasks[0].fields.get_mut("cost").unwrap().value = "100元".into();
        assert!(validate_card(&wrong, true).is_err());
        let budget = wrong.tasks[0].fields.get_mut("cost").unwrap();
        budget.value = "预算未定".into();
        budget.source = "provided".into();
        budget.evidence = "预算未定".into();
        let parsed =
            parse_card(&serde_json::to_string(&wrong).unwrap(), &wrong.description).unwrap();
        assert_eq!(parsed.tasks[0].fields["cost"].value, "");
        assert_eq!(parsed.tasks[0].fields["cost"].source, "missing");
    }

    #[test]
    fn confirms_edits_and_empty_fields_without_adopting_suggestions() {
        let mut edited = card();
        let field = edited.tasks[0].fields.get_mut("taskType").unwrap();
        field.value.clear();
        field.source = "missing".into();
        edited.suggestions.push(Suggestion {
            id: "s".into(),
            task_index: 0,
            field: "cost".into(),
            value: "100元".into(),
            reason: "建议".into(),
            accepted: false,
        });
        let mut session = SelectionSession::default();
        let request_id = Uuid::new_v4().to_string();
        let input = || ConfirmInput {
            session_id: session.id.clone(),
            revision: 0,
            request_id: request_id.clone(),
            card: edited.clone(),
        };
        let first = input();
        let second = input();
        confirm(first, &mut session).unwrap();
        confirm(second, &mut session).unwrap();
        assert_eq!(session.revision, 1);
        let confirmed = &session.confirmed.unwrap().card;
        assert_eq!(confirmed.tasks[0].fields["taskType"].value, "");
        assert!(!confirmed.suggestions[0].accepted);
        assert_eq!(confirmed.tasks[0].fields["cost"].value, "");
    }

    #[test]
    fn validates_versions_and_configuration_compatibility() {
        let config =
            parse_config(br#"{"schemaVersion":1,"activeArenaType":"text","connections":[]}"#)
                .unwrap();
        assert_eq!(config.selection_assistant.model_id, DEFAULT_MODEL);
        assert!(!config.selection_assistant.has_credential);
        let mut session = SelectionSession::default();
        assert!(
            confirm(
                ConfirmInput {
                    session_id: session.id.clone(),
                    revision: 2,
                    request_id: Uuid::new_v4().to_string(),
                    card: card()
                },
                &mut session
            )
            .is_err()
        );
        let mut invalid = card();
        invalid.tasks[0].fields.remove("accuracy");
        assert!(validate_card(&invalid, false).is_err());
        invalid = card();
        invalid.tasks.push(invalid.tasks[0].clone());
        assert!(validate_card(&invalid, false).is_err());
    }

    #[test]
    fn accepts_priority_subsets_and_treats_all_three_as_no_preference() {
        for value in ["", "准确性", "速度、成本", "准确性、速度、成本"] {
            let mut input = card();
            input.tasks[0].fields.insert(
                "priorities".into(),
                RequirementField {
                    value: value.into(),
                    source: if value.is_empty() { "missing" } else { "user" }.into(),
                    evidence: String::new(),
                },
            );
            validate_card(&input, false).unwrap();
            let mut session = SelectionSession::default();
            confirm(
                ConfirmInput {
                    session_id: session.id.clone(),
                    revision: 0,
                    request_id: Uuid::new_v4().to_string(),
                    card: input,
                },
                &mut session,
            )
            .unwrap();
            let research = task_card(&session, "task").unwrap();
            assert_eq!(
                research.tasks[0].fields["priorities"].value,
                if value == "准确性、速度、成本" {
                    ""
                } else {
                    value
                }
            );
            assert_eq!(
                session.confirmed.unwrap().card.tasks[0].fields["priorities"].value,
                value
            );
        }
        for value in [
            "准确性、准确性",
            "质量",
            "准确性、、速度",
            "准确性、速度、成本、成本",
        ] {
            assert!(priority_values(value).is_none());
        }
        let mut legacy = card();
        legacy.description.push_str("；准确性 > 速度 > 成本");
        legacy.tasks[0].fields.insert(
            "priorities".into(),
            RequirementField {
                value: "准确性 > 速度 > 成本".into(),
                source: "provided".into(),
                evidence: "准确性 > 速度 > 成本".into(),
            },
        );
        let parsed = parse_card(
            &serde_json::to_string(&legacy).unwrap(),
            &legacy.description,
        )
        .unwrap();
        assert_eq!(
            parsed.tasks[0].fields["priorities"].value,
            "准确性、速度、成本"
        );
    }

    #[test]
    fn fixes_the_driver_model_and_rejects_other_models() {
        let mut connection = official_connection();
        assert_eq!(connection.base_url, "https://api.deepseek.com/v1");
        assert_eq!(
            fixed_model(&connection, DEFAULT_MODEL).unwrap(),
            "deepseek-flash"
        );
        assert!(fixed_model(&connection, "deepseek-chat").is_err());
        assert!(fixed_model(&connection, "gpt-4o").is_err());
        connection.provider_kind = "siliconflow".into();
        assert!(fixed_model(&connection, DEFAULT_MODEL).is_err());
    }

    #[test]
    fn keeps_the_driver_key_separate_and_returns_metadata_only() {
        let directory =
            std::env::temp_dir().join(format!("model-battle-driver-{}", Uuid::new_v4()));
        let path = directory.join("config.json");
        let battle_keys = HashMap::from([("sf".to_string(), "sk-test-only-battle".to_string())]);
        write_credentials(&directory.join(CREDENTIALS_FILE_NAME), &battle_keys).unwrap();
        assert!(!assistant_status(&path).has_credential);
        assert!(save_driver_key(&path, "bad key").is_err());
        let settings = save_driver_key(&path, "sk-test-only-official-secret").unwrap();
        let public = serde_json::to_string(&settings).unwrap();
        assert!(!public.contains("sk-test"));
        assert!(settings.has_credential);
        assert_eq!(settings.model_id, DEFAULT_MODEL);
        assert_eq!(driver_key(&path).unwrap(), "sk-test-only-official-secret");
        assert_eq!(
            read_credentials(&directory.join(CREDENTIALS_FILE_NAME)).unwrap(),
            battle_keys
        );
        let public_config = serde_json::to_string(&load_config(&path).unwrap()).unwrap();
        assert!(!public_config.contains("sk-test"));
        assert!(
            load_config(&path)
                .unwrap()
                .selection_assistant
                .has_credential
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(directory.join(DRIVER_KEY_FILE))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn updates_existing_cards_without_losing_the_previous_card_on_failure() {
        let mut session = SelectionSession::default();
        let previous = card();
        confirm(
            ConfirmInput {
                session_id: session.id.clone(),
                revision: 0,
                request_id: Uuid::new_v4().to_string(),
                card: previous.clone(),
            },
            &mut session,
        )
        .unwrap();
        let input = ExtractInput {
            session_id: session.id.clone(),
            revision: session.revision,
            request_id: Uuid::new_v4().to_string(),
            description: "新的补充".into(),
        };
        assert!(begin_extraction(&mut session, &input).unwrap().is_none());
        assert!(session.running);
        assert!(begin_extraction(&mut session, &input).is_err());
        assert!(finish_extraction(&mut session, Err(validation("测试失败"))).is_err());
        assert!(!session.running);
        assert_eq!(session.confirmed.as_ref().unwrap().card, previous);
        let mut next = input;
        next.request_id = Uuid::new_v4().to_string();
        assert!(begin_extraction(&mut session, &next).is_err());
        next.revision = session.revision;
        begin_extraction(&mut session, &next).unwrap();
        let invocation = Invocation {
            model_id: DEFAULT_MODEL.into(),
            connection_name: "test".into(),
            elapsed_ms: 1,
            first_token_ms: None,
            usage: None,
        };
        finish_extraction(&mut session, Ok((previous.clone(), invocation))).unwrap();
        assert!(session.confirmed.is_none());
        assert!(session.confirmation_request.is_none());
        assert_eq!(session.draft.as_ref().unwrap(), &previous);
        let revision = session.revision;
        assert!(begin_extraction(&mut session, &next).unwrap().is_some());
        assert_eq!(session.revision, revision);
    }

    fn prefilled_card() -> RequirementCard {
        let mut draft = card();
        draft.tasks[0].user_concerns = vec!["术语保留".into()];
        draft.suggestions = [
            ("taskDifficulty", "易"),
            ("taskFocus", "语音转写"),
            ("businessGoal", "将会议录音转为文字"),
            ("input", "会议录音"),
            ("output", "转写文本"),
            ("priorities", "速度"),
        ]
        .into_iter()
        .map(|(field, value)| Suggestion {
            id: String::new(),
            task_index: 0,
            field: field.into(),
            value: value.into(),
            reason: "会议后需要及时整理".into(),
            accepted: false,
        })
        .collect();
        draft
    }

    #[test]
    fn validates_task_profile_prefill_and_migrates_old_cards_without_guessing_difficulty() {
        let mut draft = prefilled_card();
        let parsed =
            parse_card(&serde_json::to_string(&draft).unwrap(), &draft.description).unwrap();
        validate_prefill(&parsed).unwrap();
        assert!(
            parsed
                .suggestions
                .iter()
                .any(|item| item.field == "taskDifficulty" && item.value == "易")
        );
        draft
            .suggestions
            .retain(|item| item.field != "taskDifficulty");
        assert!(validate_prefill(&draft).is_err());
        draft.suggestions.clear();
        draft.tasks[0]
            .fields
            .get_mut("taskDifficulty")
            .unwrap()
            .value = "非常难".into();
        draft.tasks[0]
            .fields
            .get_mut("taskDifficulty")
            .unwrap()
            .source = "user".into();
        assert!(validate_card(&draft, false).is_err());
        draft.tasks[0].fields.remove("taskDifficulty");
        draft.tasks[0].fields.remove("taskFocus");
        migrate_legacy_io(&mut draft);
        validate_card(&draft, false).unwrap();
        assert_eq!(draft.tasks[0].fields["taskDifficulty"].value, "");
        assert_eq!(draft.tasks[0].fields["taskFocus"].value, "");
    }

    #[test]
    fn checks_initial_prefill_but_respects_explicitly_cleared_followups() {
        let mut draft = prefilled_card();
        validate_prefill(&draft).unwrap();
        draft.suggestions.retain(|item| item.field != "output");
        assert!(validate_prefill(&draft).is_err());
        draft = prefilled_card();
        draft.tasks[0].user_concerns.clear();
        assert!(validate_prefill(&draft).is_err());
        draft.description = "本次只为以下一个 AI 环节选型，用户主动清除建议".into();
        validate_prefill(&draft).unwrap();
    }

    fn mock_connection(
        status: &str,
        body: serde_json::Value,
    ) -> (Connection, std::thread::JoinHandle<()>) {
        use std::io::Read;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
        let status = status.to_string();
        let thread = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let length = stream.read(&mut buffer).unwrap();
                if length == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..length]);
                let text = String::from_utf8_lossy(&request);
                if let Some((headers, payload)) = text.split_once("\r\n\r\n") {
                    let size: usize = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(str::trim)
                                .map(str::to_string)
                        })
                        .unwrap()
                        .parse()
                        .unwrap();
                    if payload.len() >= size {
                        let parsed: serde_json::Value = serde_json::from_str(payload).unwrap();
                        assert_eq!(parsed["model"], DEFAULT_MODEL);
                        assert_eq!(parsed["messages"][0]["role"], "system");
                        assert_eq!(parsed["stream"], true);
                        assert_eq!(parsed["thinking"]["type"], "disabled");
                        assert!(parsed.get("reasoning_effort").is_none());
                        assert_eq!(parsed["response_format"]["type"], "json_object");
                        break;
                    }
                }
            }
            let content_type = if body.is_string() {
                "text/event-stream"
            } else {
                "application/json"
            };
            let body = body
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| body.to_string());
            write!(stream, "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
            for chunk in body
                .as_bytes()
                .chunks(if body.len() > 65536 { 4093 } else { 7 })
            {
                stream.write_all(chunk).unwrap();
            }
        });
        (
            Connection {
                id: "mock".into(),
                display_name: "test".into(),
                provider_kind: "deepseek_official".into(),
                base_url,
                has_credential: true,
                credential_length: None,
                models: vec![],
            },
            thread,
        )
    }

    #[tokio::test]
    async fn normalizes_a_real_http_recommendation_response_before_display() {
        let card = prefilled_card();
        let reply = serde_json::json!({"groups":[{"taskId":null,"candidates":[
            {"modelId":"model-a","name":"Model A","reason":"适合会议录音转写。","origin":"domestic","evidence":null,"strengths":["术语保留"],"contextWindow":null},
            {"model":"model-b","reason":"适合多语言转写。","origin":"overseas","evidence":[]}
        ],"leaderboard":null,"priorityReason":null}],"summary":"额外说明"});
        let body = serde_json::json!({"choices":[{"message":{"content":reply.to_string()}}]});
        let (connection, thread) = mock_connection("200 OK", body);
        let (output, _) = assistant_request(
            &AssistantSettings::default(),
            &connection,
            "test-only-key".into(),
            "确认后的会议转写需求",
            include_str!("prompts/selection_research.txt"),
            "调研结论",
            |_| {},
            |_, _| {},
            |_| {},
        )
        .await
        .unwrap();
        thread.join().unwrap();
        let report = research::parse_report(&output.text, &card).unwrap();
        assert!(report.has_candidate(&card.tasks[0].id, "model-a"));
        assert!(report.has_candidate(&card.tasks[0].id, "model-b"));
        let json = serde_json::to_value(report).unwrap();
        assert_eq!(json["groups"][0]["candidates"][0]["displayName"], "Model A");
        assert_eq!(
            json["groups"][0]["candidates"][0]["reason"],
            "适合会议录音转写。"
        );
        assert_eq!(
            json["groups"][0]["candidates"][0]["evidence"],
            serde_json::json!([])
        );
    }

    #[tokio::test]
    async fn uses_the_real_http_contract_and_validates_before_returning_a_card() {
        let body = serde_json::json!({"choices":[{"message":{"content":serde_json::to_string(&prefilled_card()).unwrap()}}],"usage":{"prompt_tokens":20,"completion_tokens":30,"total_tokens":50,"prompt_cache_hit_tokens":16}});
        let (connection, thread) = mock_connection("200 OK", body);
        let metrics = Mutex::new(SelectionMetrics::default());
        let (draft, invocation) = extract_request(
            &AssistantSettings::default(),
            &connection,
            "test-only-key".into(),
            "会议录音转文字，预算未定",
            |_| {},
            |_, _| {},
            |invocation| metrics.lock().unwrap().record(invocation),
        )
        .await
        .unwrap();
        thread.join().unwrap();
        assert_eq!(draft.tasks[0].fields["cost"].value, "");
        assert_eq!(invocation.usage.unwrap().total_tokens, Some(50));
        let metrics = metrics.lock().unwrap();
        assert_eq!(metrics.completed_calls, 1);
        assert_eq!(metrics.total_tokens, 50);
        assert_eq!(metrics.cache_hit_tokens, 16);
        assert_eq!(metrics.cache_input_tokens, 20);
        let (connection, thread) = mock_connection(
            "401 Unauthorized",
            serde_json::json!({"error":{"message":"Invalid key: sk-test-secret"}}),
        );
        let error = extract_request(
            &AssistantSettings::default(),
            &connection,
            "test-only-key".into(),
            "会议录音转文字",
            |_| {},
            |_, _| {},
            |_| {},
        )
        .await
        .err()
        .unwrap();
        thread.join().unwrap();
        assert_eq!(error.code, "AUTH_FAILED");
        assert!(!error.message.contains("sk-test-secret"));
    }

    #[test]
    fn more_candidates_are_consumed_once_and_remain_consumed_after_reload() {
        let mut session = SelectionSession::default();
        let report = research::parse_report(
            r#"{"candidates":[{"model":"existing","reason":"适合当前任务。"}]}"#,
            &card(),
        )
        .unwrap();
        let result: research::ResearchResult = serde_json::from_value(serde_json::json!({"status":"complete","revision":0,"researchedAtMs":1,"asOfDate":"2026-10-22","sources":[],"report":report,"error":""})).unwrap();
        session.task_runs.insert(
            "task".into(),
            TaskRun {
                research: Some(result),
                ..Default::default()
            },
        );
        assert_eq!(
            request_more_candidates(&mut session, "task").unwrap(),
            vec!["existing"]
        );
        assert!(request_more_candidates(&mut session, "task").is_err());
        let mut restored: SelectionSession =
            serde_json::from_value(serde_json::to_value(&session).unwrap()).unwrap();
        assert!(request_more_candidates(&mut restored, "task").is_err());
        assert!(
            restored.task_runs["task"]
                .research
                .as_ref()
                .unwrap()
                .report
                .as_ref()
                .unwrap()
                .has_candidate("task", "existing")
        );
    }

    #[test]
    fn preserves_cumulative_metrics_and_excludes_missing_or_invalid_cache_usage() {
        let mut metrics = SelectionMetrics::default();
        let invocation = |usage| Invocation {
            model_id: DEFAULT_MODEL.into(),
            connection_name: "test".into(),
            elapsed_ms: 3000,
            first_token_ms: Some(1000),
            usage,
        };
        for value in [
            serde_json::json!({"prompt_tokens":100,"completion_tokens":20,"total_tokens":120,"prompt_cache_hit_tokens":96}),
            serde_json::json!({"prompt_tokens":200,"completion_tokens":20,"prompt_cache_hit_tokens":0}),
            serde_json::json!({"prompt_tokens":10,"completion_tokens":20,"total_tokens":30,"prompt_cache_hit_tokens":20}),
        ] {
            metrics.record(invocation(usage_from_value(&value)));
        }
        metrics.record(invocation(None));
        let restored: SelectionMetrics =
            serde_json::from_value(serde_json::to_value(&metrics).unwrap()).unwrap();
        assert_eq!(restored.completed_calls, 4);
        assert_eq!(restored.usage_calls, 3);
        assert_eq!(restored.total_tokens, 370);
        assert_eq!(restored.cache_hit_tokens, 96);
        assert_eq!(restored.cache_input_tokens, 300);
        assert!(restored.latest.unwrap().usage.is_none());
        let mut old = serde_json::to_value(SelectionSession::default()).unwrap();
        old.as_object_mut().unwrap().remove("metrics");
        assert_eq!(
            serde_json::from_value::<SelectionSession>(old)
                .unwrap()
                .metrics
                .completed_calls,
            0
        );
    }

    #[tokio::test]
    async fn reports_stream_phases_and_analysis_separately_without_json_in_the_preview() {
        let content = serde_json::to_string(&prefilled_card()).unwrap();
        let reasoning = serde_json::json!({"choices":[{"delta":{"reasoning_content":"先识别目标用户和输入输出。"}}]});
        let event = serde_json::json!({"choices":[{"delta":{"content":content}}]});
        let body = format!("data: {reasoning}\n\ndata: {event}\n\ndata: [DONE]\n\n");
        let (connection, thread) = mock_connection("200 OK", serde_json::Value::String(body));
        let messages = Mutex::new(Vec::new());
        let updates = Mutex::new(ExtractionProgress::default());
        let (draft, _) = extract_request(
            &AssistantSettings::default(),
            &connection,
            "test-only-key".into(),
            "会议录音转文字，预算未定",
            |message| messages.lock().unwrap().push(message.to_string()),
            |content, reasoning| {
                update_stream_progress(&mut updates.lock().unwrap(), content, reasoning)
            },
            |_| {},
        )
        .await
        .unwrap();
        thread.join().unwrap();
        assert_eq!(draft.tasks[0].name, "语音转写");
        let updates = updates.lock().unwrap();
        assert_eq!(updates.analysis_preview, "先识别目标用户和输入输出。");
        assert!(updates.received_chars > content.chars().count());
        let messages = messages.lock().unwrap();
        assert!(
            messages
                .iter()
                .any(|message| message.contains("服务已响应"))
        );
        assert!(messages.iter().any(|message| message.contains("正在分析")));
        assert!(messages.iter().any(|message| message.contains("语音转写")));
        assert!(
            messages
                .iter()
                .all(|message| !message.contains("先识别目标用户和输入输出。"))
        );
    }

    #[tokio::test]
    async fn reports_research_work_instead_of_card_extraction() {
        let reasoning =
            serde_json::json!({"choices":[{"delta":{"reasoning_content":"private-reasoning"}}]});
        let event = serde_json::json!({"choices":[{"delta":{"content":"{}"}}]});
        let body = format!("data: {reasoning}\n\ndata: {event}\n\ndata: [DONE]\n\n");
        let (connection, thread) = mock_connection("200 OK", serde_json::Value::String(body));
        let messages = Mutex::new(Vec::new());
        assistant_request(
            &AssistantSettings::default(),
            &connection,
            "test-only-key".into(),
            "测试资料",
            "测试提示",
            "调研结论",
            |message| messages.lock().unwrap().push(message.to_string()),
            |_, _| {},
            |_| {},
        )
        .await
        .unwrap();
        thread.join().unwrap();
        let messages = messages.lock().unwrap();
        assert!(
            messages
                .iter()
                .any(|message| message.contains("筛选候选模型"))
        );
        assert!(
            messages
                .iter()
                .any(|message| message.contains("生成调研结论"))
        );
        assert!(
            messages
                .iter()
                .all(|message| !message.contains("任务划分")
                    && !message.contains("private-reasoning"))
        );
    }

    #[tokio::test]
    async fn does_not_count_sse_transport_overhead_as_generated_content() {
        let event = serde_json::json!({"choices":[{"delta":{"content":serde_json::to_string(&prefilled_card()).unwrap()}}]});
        let body = format!(
            "{}data: {event}\n\ndata: [DONE]\n\n",
            ": heartbeat\n".repeat(100000)
        );
        assert!(body.len() > 1024 * 1024);
        let (connection, thread) = mock_connection("200 OK", serde_json::Value::String(body));
        let (draft, _) = extract_request(
            &AssistantSettings::default(),
            &connection,
            "test-only-key".into(),
            "会议录音转文字，预算未定",
            |_| {},
            |_, _| {},
            |_| {},
        )
        .await
        .unwrap();
        thread.join().unwrap();
        assert_eq!(draft.tasks[0].name, "语音转写");
    }

    #[tokio::test]
    #[ignore = "需要显式指定本机配置、服务商与模型；会产生一次模型调用费用"]
    async fn real_assistant_smoke() {
        let config_path =
            PathBuf::from(std::env::var("MODEL_BATTLE_SMOKE_CONFIG").expect("需要配置路径"));
        let connection = official_connection();
        let key = driver_key(&config_path).expect("需要在 App 内配置官网 API Key");
        let settings = AssistantSettings::default();
        let (draft, invocation) = extract_request(
            &settings,
            &connection,
            key,
            "本次只为会议录音转成文字这一个 AI 环节选模型，现在人工转写，预算未定。",
            |_| {},
            |_, _| {},
            |_| {},
        )
        .await
        .expect("真实需求整理失败");
        assert_eq!(draft.tasks.len(), 1, "本次只选择录音转写");
        assert!(
            draft
                .tasks
                .iter()
                .all(|task| task.fields["cost"].value.is_empty()),
            "未知预算必须留空"
        );
        println!(
            "SMOKE model={} elapsed_ms={} tasks={} total_tokens={:?}",
            invocation.model_id,
            invocation.elapsed_ms,
            draft.tasks.len(),
            invocation.usage.and_then(|usage| usage.total_tokens)
        );
    }
}

#[tauri::command]
pub(crate) fn selection_open_api(url: String) -> AppResult<()> {
    if !research::safe_url(&url) {
        return Err(validation("API 官网地址无效。"));
    }
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("/usr/bin/open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("rundll32");
        command.arg("url.dll,FileProtocolHandler");
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = std::process::Command::new("xdg-open");
    command
        .arg(url)
        .status()
        .ok()
        .filter(|status| status.success())
        .map(|_| ())
        .ok_or_else(|| validation("无法打开浏览器，请复制官网链接访问。"))
}
