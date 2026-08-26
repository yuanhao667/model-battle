use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use futures_util::future::join_all;
use keyring::Entry;
use reqwest::{Client, StatusCode, Url};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, hash_map::DefaultHasher},
    fs::{self, OpenOptions},
    hash::{Hash, Hasher},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager, State};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const SCHEMA_VERSION: u32 = 1;
const KEYCHAIN_SERVICE: &str = "Model Battle API Key 加密存储";
const KEYCHAIN_VAULT_ACCOUNT: &str = "api-key-vault-v1";
const CONNECTION_TEST_PROMPT: &str = "请只回复：连接成功";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppError {
    code: &'static str,
    message: String,
    retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_status: Option<u16>,
}

impl AppError {
    fn new(code: &'static str, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
            provider_status: None,
        }
    }

    fn status(mut self, status: StatusCode) -> Self {
        self.provider_status = Some(status.as_u16());
        self
    }
}

type AppResult<T> = Result<T, AppError>;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppConfig {
    schema_version: u32,
    active_arena_type: String,
    connections: Vec<Connection>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            active_arena_type: "text".into(),
            connections: vec![],
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Connection {
    id: String,
    display_name: String,
    provider_kind: String,
    base_url: String,
    has_credential: bool,
    models: Vec<ModelConfig>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelConfig {
    id: String,
    model_id: String,
    display_name: String,
    output_type: String,
    supports_reference_image: bool,
    enabled: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelInput {
    model_id: String,
    display_name: String,
    output_type: String,
    supports_reference_image: bool,
    enabled: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionInput {
    connection_id: Option<String>,
    display_name: String,
    provider_kind: String,
    base_url: String,
    api_key: Option<String>,
    output_type: String,
    supports_reference_image: bool,
    models: Vec<ModelInput>,
    validation_token: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionProbeInput {
    connection_id: Option<String>,
    provider_kind: String,
    base_url: String,
    api_key: Option<String>,
    model_id: Option<String>,
    output_type: String,
    audio_input: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunInput {
    prompt: String,
    reference_image: Option<String>,
    audio_input: Option<String>,
    image_ratio: Option<String>,
    audio_voice: Option<String>,
    video_seconds: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TestResult {
    validation_token: String,
    has_usage: bool,
    elapsed_ms: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogResult {
    validation_token: String,
    models: Vec<CatalogOption>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SkippedModel {
    model_id: String,
    reason: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionSaveResult {
    connection: Connection,
    skipped_models: Vec<SkippedModel>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogOption {
    model_id: String,
    output_type: String,
    supports_reference_image: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Usage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    total_tokens: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelRunFinished {
    run_id: String,
    model_config_id: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_audio: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_video: Option<String>,
    elapsed_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<Usage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<AppError>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunFinished {
    run_id: String,
    status: &'static str,
    elapsed_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AutoDisabledModel {
    model_config_id: String,
    model_id: String,
    provider_name: String,
    reason: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RunStarted {
    run_id: String,
}

#[derive(Serialize)]
struct CancelResult {
    status: &'static str,
}

#[derive(Clone)]
struct CurrentRun {
    id: String,
    started: Instant,
    cancellation: CancellationToken,
}

#[derive(Clone)]
struct AppState {
    config_path: PathBuf,
    key_vault: Arc<Mutex<Option<HashMap<String, String>>>>,
    validated_connections: Arc<Mutex<HashMap<String, u64>>>,
    current_run: Arc<Mutex<Option<CurrentRun>>>,
}

#[derive(Clone)]
struct RunTarget {
    model_config_id: String,
    model_id: String,
    provider_kind: String,
    output_type: String,
    base_url: Url,
    api_key: String,
}

fn lock_error() -> AppError {
    AppError::new("UNKNOWN", "应用内部状态不可用，请重启后重试。", true)
}

fn validate_output_type(output_type: &str) -> AppResult<()> {
    if matches!(
        output_type,
        "text" | "image" | "audio" | "audio_to_text" | "video"
    ) {
        Ok(())
    } else {
        Err(AppError::new(
            "VALIDATION_ERROR",
            "当前只支持文本、图片、音频和视频模型。",
            false,
        ))
    }
}

fn output_category(output_type: &str) -> &str {
    if output_type == "audio_to_text" {
        "audio"
    } else {
        output_type
    }
}

fn normalize_base(raw: &str) -> AppResult<(String, Url)> {
    let base = raw.trim().trim_end_matches('/').to_string();
    let url = Url::parse(&base)
        .map_err(|_| AppError::new("VALIDATION_ERROR", "Base URL 格式不正确。", false))?;
    let local_http = url.scheme() == "http"
        && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if url.scheme() != "https" && !local_http {
        return Err(AppError::new(
            "VALIDATION_ERROR",
            "Base URL 必须使用 HTTPS；本地测试可使用 localhost。",
            false,
        ));
    }
    Ok((base, url))
}

fn chat_endpoint(mut base: Url) -> Url {
    if base
        .path()
        .trim_end_matches('/')
        .ends_with("chat/completions")
    {
        return base;
    }
    let path = format!("{}/chat/completions", base.path().trim_end_matches('/'));
    base.set_path(&path);
    base
}

fn models_endpoint(provider_kind: &str, output_type: &str, mut base: Url) -> Url {
    if provider_kind == "aihubmix" {
        base.set_path("/api/v1/models");
        base.set_query(None);
        let catalog_type = match output_type {
            "image" => "image_generation",
            "audio" => "tts",
            "audio_to_text" => "stt",
            "video" => "video",
            _ => "llm",
        };
        base.query_pairs_mut().append_pair("type", catalog_type);
        return base;
    }
    if !base.path().trim_end_matches('/').ends_with("models") {
        let path = format!("{}/models", base.path().trim_end_matches('/'));
        base.set_path(&path);
    }
    if provider_kind == "siliconflow" {
        if output_type == "audio_to_text" {
            base.query_pairs_mut()
                .append_pair("sub_type", "speech-to-text");
        } else {
            base.query_pairs_mut().append_pair("type", output_type);
        }
        if output_type == "text" {
            base.query_pairs_mut().append_pair("sub_type", "chat");
        }
    }
    base
}

fn image_endpoint(mut base: Url) -> Url {
    if !base
        .path()
        .trim_end_matches('/')
        .ends_with("images/generations")
    {
        let path = format!("{}/images/generations", base.path().trim_end_matches('/'));
        base.set_path(&path);
    }
    base
}

fn aihubmix_prediction_endpoint(mut base: Url, model_id: &str) -> Url {
    let model_path = aihubmix_image_model_path(model_id);
    let path = format!(
        "{}/models/{model_path}/predictions",
        base.path().trim_end_matches('/')
    );
    base.set_path(&path);
    base
}

fn aihubmix_gemini_endpoint(mut base: Url, model_id: &str) -> Url {
    base.set_path(&format!("/gemini/v1beta/models/{model_id}:generateContent"));
    base
}

fn aihubmix_image_model_path(model_id: &str) -> String {
    if model_id.contains('/') {
        return model_id.to_string();
    }
    let lower = model_id.to_ascii_lowercase();
    let provider = if lower.starts_with("gpt-image") || lower.starts_with("dall-e") {
        "openai"
    } else if lower.contains("qwen") {
        "qianfan"
    } else if lower.contains("flux") {
        "bfl"
    } else {
        ""
    };
    if provider.is_empty() {
        model_id.to_string()
    } else {
        format!("{provider}/{model_id}")
    }
}

fn audio_endpoint(mut base: Url) -> Url {
    let path = format!("{}/audio/speech", base.path().trim_end_matches('/'));
    base.set_path(&path);
    base
}

fn transcription_endpoint(mut base: Url) -> Url {
    let path = format!("{}/audio/transcriptions", base.path().trim_end_matches('/'));
    base.set_path(&path);
    base
}

fn video_endpoint(mut base: Url) -> Url {
    let path = format!("{}/videos", base.path().trim_end_matches('/'));
    base.set_path(&path);
    base
}

fn config_backup_path(config_path: &Path) -> PathBuf {
    config_path.with_file_name("config.backup.json")
}

fn parse_config(bytes: &[u8]) -> AppResult<AppConfig> {
    let config: AppConfig = serde_json::from_slice(bytes).map_err(|_| {
        AppError::new(
            "CONFIG_CORRUPTED",
            "本地配置无法读取，已尝试恢复备份。",
            false,
        )
    })?;
    if config.schema_version > SCHEMA_VERSION {
        return Err(AppError::new(
            "CONFIG_CORRUPTED",
            "配置来自更高版本，请升级应用后再使用。",
            false,
        ));
    }
    Ok(config)
}

fn migrate_legacy_model_types(config: &mut AppConfig) -> bool {
    let mut changed = false;
    for connection in &mut config.connections {
        if connection.provider_kind != "aihubmix" {
            continue;
        }
        for model in &mut connection.models {
            if model.output_type == "text"
                && model.model_id.eq_ignore_ascii_case("gemini-3-pro-image")
            {
                model.output_type = "image".into();
                model.supports_reference_image = true;
                changed = true;
            }
        }
    }
    if changed && config.active_arena_type == "text" {
        let has_enabled_text = config.connections.iter().any(|connection| {
            connection
                .models
                .iter()
                .any(|model| model.enabled && model.output_type == "text")
        });
        if !has_enabled_text {
            config.active_arena_type = "image".into();
        }
    }
    changed
}

fn keep_latest_platform_connections(config: &mut AppConfig) -> bool {
    let mut changed = false;
    for provider_kind in ["aihubmix", "siliconflow"] {
        let Some(latest_index) = config
            .connections
            .iter()
            .rposition(|connection| connection.provider_kind == provider_kind)
        else {
            continue;
        };
        for index in (0..latest_index).rev() {
            if config.connections[index].provider_kind != provider_kind {
                continue;
            }
            config.connections.remove(index);
            changed = true;
        }
    }
    changed
}

fn load_config(path: &Path) -> AppResult<AppConfig> {
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let primary = fs::read(path)
        .map_err(|_| AppError::new("CONFIG_CORRUPTED", "无法读取本地配置。", true))
        .and_then(|bytes| parse_config(&bytes));
    match primary {
        Ok(mut config) => {
            if migrate_legacy_model_types(&mut config)
                | keep_latest_platform_connections(&mut config)
            {
                save_config(path, &config)?;
            }
            Ok(config)
        }
        Err(primary_error) => {
            let backup = config_backup_path(path);
            if !backup.exists() {
                return Err(primary_error);
            }
            let bytes = fs::read(&backup).map_err(|_| primary_error.clone())?;
            let mut restored = parse_config(&bytes)?;
            migrate_legacy_model_types(&mut restored);
            keep_latest_platform_connections(&mut restored);
            fs::remove_file(path).map_err(|_| primary_error)?;
            save_config(path, &restored)?;
            Ok(restored)
        }
    }
}

fn save_config(path: &Path, config: &AppConfig) -> AppResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::new("UNKNOWN", "无法确定本地配置目录。", false))?;
    fs::create_dir_all(parent)
        .map_err(|_| AppError::new("UNKNOWN", "无法创建本地配置目录。", true))?;
    let bytes = serde_json::to_vec_pretty(config)
        .map_err(|_| AppError::new("UNKNOWN", "无法生成本地配置。", false))?;
    let temp = path.with_file_name("config.tmp.json");
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temp)
        .map_err(|_| AppError::new("UNKNOWN", "无法写入本地配置。", true))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| AppError::new("UNKNOWN", "无法完整保存本地配置。", true))?;
    if path.exists() {
        fs::copy(path, config_backup_path(path))
            .map_err(|_| AppError::new("UNKNOWN", "无法备份原有配置。", true))?;
    }
    fs::rename(&temp, path).map_err(|_| AppError::new("UNKNOWN", "无法替换本地配置。", true))?;
    Ok(())
}

fn keychain_entry_for(service: &str, account: &str) -> AppResult<Entry> {
    Entry::new(service, account)
        .map_err(|_| AppError::new("UNKNOWN", "无法访问 macOS 钥匙串。", true))
}

fn keychain_entry(account: &str) -> AppResult<Entry> {
    keychain_entry_for(KEYCHAIN_SERVICE, account)
}

fn read_key_vault() -> AppResult<Option<HashMap<String, String>>> {
    match keychain_entry(KEYCHAIN_VAULT_ACCOUNT)?.get_password() {
        Ok(value) => serde_json::from_str(&value)
            .map(Some)
            .map_err(|_| AppError::new("UNKNOWN", "钥匙串中的 API Key 仓库无法读取。", false)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err(AppError::new(
            "KEYCHAIN_PERMISSION_REQUIRED",
            "需要允许访问 macOS 钥匙串。",
            true,
        )),
    }
}

fn write_key_vault(keys: &HashMap<String, String>) -> AppResult<()> {
    let value = serde_json::to_string(keys)
        .map_err(|_| AppError::new("UNKNOWN", "无法保存 API Key 仓库。", false))?;
    keychain_entry(KEYCHAIN_VAULT_ACCOUNT)?
        .set_password(&value)
        .map_err(|_| AppError::new("UNKNOWN", "API Key 无法写入 macOS 钥匙串。", true))
}

fn cached_key_vault(state: &AppState) -> AppResult<HashMap<String, String>> {
    let mut cache = state.key_vault.lock().map_err(|_| lock_error())?;
    if let Some(keys) = cache.as_ref() {
        return Ok(keys.clone());
    }
    let keys = read_key_vault()?.unwrap_or_default();
    *cache = Some(keys.clone());
    Ok(keys)
}

fn save_key_vault(state: &AppState, keys: &HashMap<String, String>) -> AppResult<()> {
    write_key_vault(keys)?;
    *state.key_vault.lock().map_err(|_| lock_error())? = Some(keys.clone());
    Ok(())
}

fn get_key(state: &AppState, connection_id: &str) -> AppResult<String> {
    cached_key_vault(state)?
        .get(connection_id)
        .cloned()
        .ok_or_else(|| {
            AppError::new(
                "CREDENTIAL_MISSING",
                "没有找到这个连接的 API Key，请重新编辑连接。",
                false,
            )
        })
}

fn set_key(state: &AppState, connection_id: &str, key: &str) -> AppResult<()> {
    let mut keys = cached_key_vault(state)?;
    keys.insert(connection_id.to_string(), key.to_string());
    save_key_vault(state, &keys)
}

fn delete_key(state: &AppState, connection_id: &str) -> AppResult<()> {
    let mut keys = cached_key_vault(state)?;
    if keys.remove(connection_id).is_some() {
        save_key_vault(state, &keys)?;
    }
    Ok(())
}

fn effective_key(
    state: &AppState,
    connection_id: Option<&str>,
    api_key: Option<&str>,
) -> AppResult<String> {
    if let Some(key) = api_key.map(str::trim).filter(|key| !key.is_empty()) {
        return Ok(key.to_string());
    }
    get_key(
        state,
        connection_id
            .ok_or_else(|| AppError::new("CREDENTIAL_MISSING", "请输入 API Key。", false))?,
    )
}

fn connection_fingerprint(
    connection_id: Option<&str>,
    provider_kind: &str,
    normalized_base: &str,
    api_key: &str,
    output_type: &str,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    connection_id.hash(&mut hasher);
    provider_kind.hash(&mut hasher);
    normalized_base.hash(&mut hasher);
    api_key.hash(&mut hasher);
    output_type.hash(&mut hasher);
    hasher.finish()
}

fn remember_validation(state: &AppState, fingerprint: u64) -> AppResult<String> {
    let token = Uuid::new_v4().to_string();
    state
        .validated_connections
        .lock()
        .map_err(|_| lock_error())?
        .insert(token.clone(), fingerprint);
    Ok(token)
}

fn provider_error(status: StatusCode, body: &str) -> AppError {
    let lower = body.to_lowercase();
    let mut error = match status.as_u16() {
        _ if lower.contains("model disabled") => AppError::new(
            "MODEL_UNAVAILABLE",
            "该模型已被平台停用，请删除或更换模型。",
            false,
        ),
        _ if lower.contains("model not exist") || lower.contains("model does not exist") => {
            AppError::new(
                "MODEL_UNAVAILABLE",
                "平台当前找不到该模型，请删除或更换模型。",
                false,
            )
        }
        _ if lower.contains("not supported") || lower.contains("does not support") => {
            AppError::new(
                "MODEL_UNSUPPORTED",
                "平台不支持调用该模型，请删除或更换模型。",
                false,
            )
        }
        _ if lower.contains("cannot be routed") => AppError::new(
            "MODEL_UNAVAILABLE",
            "平台暂时无法调度该模型，请稍后重试或更换模型。",
            true,
        ),
        401 => AppError::new("AUTH_FAILED", "API Key 无效，请检查后重新配置。", false),
        403 => AppError::new(
            "MODEL_ACCESS_DENIED",
            "当前 API Key 无权调用该模型，或该模型未对账户开放。",
            false,
        ),
        429 => AppError::new("RATE_LIMITED", "接口请求过于频繁，请稍后重试。", true),
        _ if lower.contains("quota") || lower.contains("credit") || lower.contains("balance") => {
            AppError::new("QUOTA_EXCEEDED", "账户额度不足，请检查平台账户。", false)
        }
        _ if status.is_server_error() => {
            AppError::new("PROVIDER_ERROR", "模型平台暂时不可用，请稍后重试。", true)
        }
        _ => AppError::new("PROVIDER_ERROR", "模型接口拒绝了本次请求。", false),
    };
    if let Some(detail) = provider_error_detail(body) {
        error.message = format!("{} 平台提示：{detail}", error.message);
    }
    error.status(status)
}

fn chat_request_body(
    model_id: &str,
    content: serde_json::Value,
    max_tokens: Option<u32>,
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "model": model_id,
        "messages": [{"role": "user", "content": content}],
        "stream": false
    });
    if model_id.to_ascii_lowercase().contains("qwen") {
        body["enable_thinking"] = serde_json::Value::Bool(false);
    }
    if let Some(max_tokens) = max_tokens {
        body["max_tokens"] = serde_json::Value::from(max_tokens);
    }
    body
}

fn provider_error_detail(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let detail = value
        .pointer("/error/message")
        .or_else(|| value.get("message"))
        .or_else(|| value.get("detail"))?
        .as_str()?;
    let detail = detail.split_whitespace().collect::<Vec<_>>().join(" ");
    (!detail.is_empty()).then(|| detail.chars().take(240).collect())
}

fn http_client() -> AppResult<Client> {
    Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .build()
        .map_err(|_| AppError::new("UNKNOWN", "无法初始化网络连接。", true))
}

fn connection_error(error: &reqwest::Error, target: &str) -> AppError {
    connection_error_for(error.is_timeout(), target)
}

fn connection_error_for(is_timeout: bool, target: &str) -> AppError {
    if is_timeout {
        return AppError::new(
            "CONNECTION_TIMEOUT",
            format!("连接 {target} 超时。请切换网络或开启全局代理后重试。"),
            true,
        );
    }
    AppError::new(
        "CONNECTION_FAILED",
        format!("无法连接 {target}。请检查网络、DNS 或代理设置。"),
        true,
    )
}

#[derive(Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
    usage: Option<OpenAiUsage>,
}
#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}
#[derive(Deserialize)]
struct OpenAiMessage {
    content: Option<String>,
}
#[derive(Deserialize)]
struct OpenAiUsage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    total_tokens: Option<u64>,
}
#[derive(Deserialize)]
struct CatalogResponse {
    data: Vec<CatalogModel>,
}
#[derive(Deserialize)]
struct CatalogModel {
    id: Option<String>,
    model_id: Option<String>,
    model: Option<String>,
    input_modalities: Option<String>,
}

#[derive(Debug)]
struct GeneratedText {
    text: String,
    usage: Option<Usage>,
}

#[derive(Debug)]
struct GeneratedImage {
    image: String,
    usage: Option<Usage>,
}

#[derive(Debug)]
struct GeneratedMedia {
    data_url: String,
}

struct GeneratedOutput {
    text: Option<String>,
    image: Option<String>,
    audio: Option<String>,
    video: Option<String>,
    usage: Option<Usage>,
}

struct ImageGenerationRequest {
    provider_kind: String,
    base_url: Url,
    api_key: String,
    model_id: String,
    prompt: String,
    reference_image: Option<String>,
    image_ratio: String,
}

struct VideoGenerationRequest {
    provider_kind: String,
    base_url: Url,
    api_key: String,
    model_id: String,
    prompt: String,
    reference_image: Option<String>,
    ratio: String,
    seconds: String,
    output_path: PathBuf,
}

async fn generate_text(
    base_url: Url,
    api_key: String,
    model_id: String,
    prompt: String,
    reference_image: Option<String>,
    cancellation: CancellationToken,
) -> AppResult<GeneratedText> {
    let content = reference_image.map_or_else(
        || serde_json::Value::String(prompt.clone()),
        |image| {
            serde_json::json!([
                {"type": "text", "text": prompt},
                {"type": "image_url", "image_url": {"url": image}}
            ])
        },
    );
    let request = http_client()?
        .post(chat_endpoint(base_url))
        .bearer_auth(api_key)
        .json(&chat_request_body(&model_id, content, None))
        .send();
    let response = tokio::select! {
        _ = cancellation.cancelled() => return Err(AppError::new("CANCELLED", "本次运行已结束。", false)),
        response = request => response.map_err(|error| connection_error(&error, "模型接口"))?,
    };
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(provider_error(status, &body));
    }
    let body: OpenAiResponse = response
        .json()
        .await
        .map_err(|_| AppError::new("RESPONSE_INVALID", "模型返回了无法识别的数据。", false))?;
    parse_openai_response(body)
}

async fn generate_image(
    input: ImageGenerationRequest,
    cancellation: CancellationToken,
) -> AppResult<GeneratedImage> {
    if input.provider_kind == "aihubmix" {
        return generate_aihubmix_image(input, cancellation).await;
    }
    let size = match input.image_ratio.as_str() {
        "3:4" => "768x1024",
        "4:3" => "1024x768",
        "9:16" => "576x1024",
        "16:9" => "1024x576",
        _ => "1024x1024",
    };
    let mut body = serde_json::json!({
        "model": input.model_id,
        "prompt": input.prompt,
        "n": 1
    });
    let object = body
        .as_object_mut()
        .ok_or_else(|| AppError::new("UNKNOWN", "无法生成图片请求。", false))?;
    object.insert(
        if input.provider_kind == "siliconflow" {
            "image_size".into()
        } else {
            "size".into()
        },
        size.into(),
    );
    if let Some(image) = input.reference_image {
        object.insert("image".into(), image.into());
    }

    let request = http_client()?
        .post(image_endpoint(input.base_url))
        .bearer_auth(input.api_key)
        .json(&body)
        .send();
    let response = tokio::select! {
        _ = cancellation.cancelled() => return Err(AppError::new("CANCELLED", "本次运行已结束。", false)),
        response = request => response.map_err(|error| connection_error(&error, "图片模型接口"))?,
    };
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(provider_error(status, &body));
    }
    parse_image_response(
        response.json().await.map_err(|_| {
            AppError::new("RESPONSE_INVALID", "图片模型返回了无法识别的数据。", false)
        })?,
    )
}

async fn generate_aihubmix_image(
    input: ImageGenerationRequest,
    cancellation: CancellationToken,
) -> AppResult<GeneratedImage> {
    if input.model_id.to_ascii_lowercase().contains("gemini") {
        return generate_aihubmix_gemini_image(input, cancellation).await;
    }

    let size = if input.model_id.to_ascii_lowercase().contains("qwen") {
        match input.image_ratio.as_str() {
            "3:4" => "768*1024",
            "4:3" => "1024*768",
            "9:16" => "576*1024",
            "16:9" => "1024*576",
            _ => "1024*1024",
        }
    } else {
        "1K"
    };
    let mut body = serde_json::json!({
        "prompt": input.prompt,
        "size": size,
        "n": 1
    });
    if let Some(image) = input.reference_image {
        body.as_object_mut()
            .expect("image request is an object")
            .insert("image".into(), image.into());
    }
    let request = http_client()?
        .post(aihubmix_prediction_endpoint(
            input.base_url,
            &input.model_id,
        ))
        .bearer_auth(input.api_key)
        .json(&body)
        .send();
    let response = tokio::select! {
        _ = cancellation.cancelled() => return Err(AppError::new("CANCELLED", "本次运行已结束。", false)),
        response = request => response.map_err(|error| connection_error(&error, "AIHubMix 图片模型接口"))?,
    };
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(provider_error(status, &body));
    }
    parse_image_response(response.json().await.map_err(|_| {
        AppError::new(
            "RESPONSE_INVALID",
            "AIHubMix 返回了无法识别的图片数据。",
            false,
        )
    })?)
}

async fn generate_aihubmix_gemini_image(
    input: ImageGenerationRequest,
    cancellation: CancellationToken,
) -> AppResult<GeneratedImage> {
    let mut parts = vec![serde_json::json!({ "text": input.prompt })];
    if let Some(image) = input.reference_image {
        let (mime_type, data) = parse_data_url(&image)
            .ok_or_else(|| AppError::new("VALIDATION_ERROR", "参考图片格式无法识别。", false))?;
        parts.push(serde_json::json!({
            "inlineData": { "mimeType": mime_type, "data": data }
        }));
    }
    let body = serde_json::json!({
        "contents": [{ "role": "user", "parts": parts }],
        "generationConfig": {
            "responseModalities": ["TEXT", "IMAGE"],
            "imageConfig": {
                "aspectRatio": input.image_ratio,
                "imageSize": "1K"
            }
        }
    });
    let request = http_client()?
        .post(aihubmix_gemini_endpoint(input.base_url, &input.model_id))
        .header("x-goog-api-key", &input.api_key)
        .bearer_auth(&input.api_key)
        .json(&body)
        .send();
    let response = tokio::select! {
        _ = cancellation.cancelled() => return Err(AppError::new("CANCELLED", "本次运行已结束。", false)),
        response = request => response.map_err(|error| connection_error(&error, "AIHubMix Gemini 生图接口"))?,
    };
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(provider_error(status, &body));
    }
    parse_gemini_image_response(response.json().await.map_err(|_| {
        AppError::new(
            "RESPONSE_INVALID",
            "Gemini 返回了无法识别的图片数据。",
            false,
        )
    })?)
}

fn parse_data_url(value: &str) -> Option<(String, String)> {
    let value = value.strip_prefix("data:")?;
    let (mime_type, data) = value.split_once(";base64,")?;
    (!mime_type.is_empty() && !data.is_empty()).then(|| (mime_type.to_string(), data.to_string()))
}

fn parse_gemini_image_response(body: serde_json::Value) -> AppResult<GeneratedImage> {
    let parts = body
        .pointer("/candidates/0/content/parts")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| AppError::new("RESPONSE_INVALID", "Gemini 没有返回图片。", false))?;
    let image = parts.iter().find_map(|part| {
        let inline = part.get("inlineData").or_else(|| part.get("inline_data"))?;
        let data = inline.get("data")?.as_str()?;
        let mime = inline
            .get("mimeType")
            .or_else(|| inline.get("mime_type"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("image/png");
        (!data.is_empty()).then(|| format!("data:{mime};base64,{data}"))
    });
    image
        .map(|image| GeneratedImage { image, usage: None })
        .ok_or_else(|| AppError::new("RESPONSE_INVALID", "Gemini 没有返回可展示的图片。", false))
}

fn parse_image_response(body: serde_json::Value) -> AppResult<GeneratedImage> {
    let image = body
        .pointer("/data/0/url")
        .or_else(|| body.pointer("/images/0/url"))
        .or_else(|| body.pointer("/output/0"))
        .or_else(|| body.get("output"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            body.pointer("/data/0/b64_json")
                .and_then(serde_json::Value::as_str)
                .map(|data| format!("data:image/png;base64,{data}"))
        })
        .filter(|image| !image.is_empty())
        .ok_or_else(|| AppError::new("RESPONSE_INVALID", "模型没有返回可展示的图片。", false))?;
    let usage = body.get("usage").and_then(|value| {
        let input_tokens = value
            .get("input_tokens")
            .or_else(|| value.get("prompt_tokens"))
            .and_then(serde_json::Value::as_u64);
        let output_tokens = value
            .get("output_tokens")
            .or_else(|| value.get("completion_tokens"))
            .and_then(serde_json::Value::as_u64);
        let total_tokens = value
            .get("total_tokens")
            .and_then(serde_json::Value::as_u64);
        (input_tokens.is_some() || output_tokens.is_some() || total_tokens.is_some()).then_some(
            Usage {
                input_tokens,
                output_tokens,
                total_tokens,
            },
        )
    });
    Ok(GeneratedImage { image, usage })
}

async fn response_data_url(
    response: reqwest::Response,
    fallback_mime: &str,
    max_bytes: usize,
    cancellation: CancellationToken,
) -> AppResult<String> {
    let mime = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback_mime)
        .to_string();
    let bytes = tokio::select! {
        _ = cancellation.cancelled() => return Err(AppError::new("CANCELLED", "本次运行已结束。", false)),
        bytes = response.bytes() => bytes.map_err(|error| connection_error(&error, "媒体下载接口"))?,
    };
    if bytes.len() > max_bytes {
        return Err(AppError::new(
            "RESPONSE_INVALID",
            "生成结果过大，当前版本无法在卡片内加载。",
            false,
        ));
    }
    Ok(format!("data:{mime};base64,{}", BASE64.encode(bytes)))
}

async fn generate_audio(
    provider_kind: String,
    base_url: Url,
    api_key: String,
    model_id: String,
    prompt: String,
    voice: String,
    cancellation: CancellationToken,
) -> AppResult<GeneratedMedia> {
    let response_format = if model_id.to_lowercase().contains("gemini") {
        "wav"
    } else {
        "mp3"
    };
    let voice = if provider_kind == "siliconflow" {
        match voice.as_str() {
            "echo" => "alex",
            "fable" => "claire",
            "onyx" => "benjamin",
            "nova" => "diana",
            "shimmer" => "bella",
            _ => "anna",
        }
        .to_string()
    } else {
        voice
    };
    let mut body = serde_json::json!({
        "model": model_id.clone(),
        "input": prompt,
        "voice": voice,
        "response_format": response_format
    });
    if provider_kind == "siliconflow" {
        if model_id.to_lowercase().contains("moss-ttsd") {
            body.as_object_mut()
                .expect("audio request is an object")
                .remove("voice");
        } else if let Some(voice) = body.get_mut("voice") {
            *voice = format!("{model_id}:{}", voice.as_str().unwrap_or("anna")).into();
        }
    }
    let request = http_client()?
        .post(audio_endpoint(base_url))
        .bearer_auth(api_key)
        .json(&body)
        .send();
    let response = tokio::select! {
        _ = cancellation.cancelled() => return Err(AppError::new("CANCELLED", "本次运行已结束。", false)),
        response = request => response.map_err(|error| connection_error(&error, "音频生成接口"))?,
    };
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(provider_error(status, &body));
    }
    Ok(GeneratedMedia {
        data_url: response_data_url(
            response,
            if response_format == "wav" {
                "audio/wav"
            } else {
                "audio/mpeg"
            },
            25 * 1024 * 1024,
            cancellation,
        )
        .await?,
    })
}

async fn transcribe_audio(
    base_url: Url,
    api_key: String,
    model_id: String,
    audio_input: String,
    cancellation: CancellationToken,
) -> AppResult<GeneratedText> {
    let (header, encoded) = audio_input
        .split_once(',')
        .ok_or_else(|| AppError::new("VALIDATION_ERROR", "无法读取音频文件。", false))?;
    let bytes = BASE64
        .decode(encoded)
        .map_err(|_| AppError::new("VALIDATION_ERROR", "无法读取音频文件。", false))?;
    if bytes.len() > 50 * 1024 * 1024 {
        return Err(AppError::new(
            "VALIDATION_ERROR",
            "音频文件不能超过 50 MB。",
            false,
        ));
    }
    let mime = header
        .strip_prefix("data:")
        .and_then(|value| value.split(';').next())
        .unwrap_or("audio/mpeg");
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name("audio")
        .mime_str(mime)
        .map_err(|_| AppError::new("VALIDATION_ERROR", "音频格式不受支持。", false))?;
    let request = http_client()?
        .post(transcription_endpoint(base_url))
        .bearer_auth(api_key)
        .multipart(
            reqwest::multipart::Form::new()
                .text("model", model_id)
                .part("file", part),
        )
        .send();
    let response = tokio::select! {
        _ = cancellation.cancelled() => return Err(AppError::new("CANCELLED", "本次运行已结束。", false)),
        response = request => response.map_err(|error| connection_error(&error, "音频转文本接口"))?,
    };
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(provider_error(status, &body));
    }
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|_| AppError::new("RESPONSE_INVALID", "模型返回了无法识别的数据。", false))?;
    let text = body
        .get("text")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| AppError::new("RESPONSE_INVALID", "模型没有返回转写文本。", false))?
        .to_string();
    Ok(GeneratedText { text, usage: None })
}

async fn response_to_file(
    response: reqwest::Response,
    output_path: &Path,
    max_bytes: usize,
    cancellation: CancellationToken,
) -> AppResult<String> {
    let bytes = tokio::select! {
        _ = cancellation.cancelled() => return Err(AppError::new("CANCELLED", "本次运行已结束。", false)),
        bytes = response.bytes() => bytes.map_err(|error| connection_error(&error, "媒体下载接口"))?,
    };
    if bytes.len() > max_bytes {
        return Err(AppError::new(
            "RESPONSE_INVALID",
            "生成结果过大，当前版本无法在卡片内加载。",
            false,
        ));
    }
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|_| AppError::new("FILE_WRITE_FAILED", "无法创建媒体缓存目录。", true))?;
    }
    fs::write(output_path, &bytes)
        .map_err(|_| AppError::new("FILE_WRITE_FAILED", "无法保存生成的视频。", true))?;
    Ok(output_path.to_string_lossy().into_owned())
}

fn silicon_video_endpoint(mut base: Url, action: &str) -> Url {
    let path = format!("{}/video/{action}", base.path().trim_end_matches('/'));
    base.set_path(&path);
    base
}

fn silicon_video_size(ratio: &str) -> &'static str {
    match ratio {
        "9:16" | "3:4" => "720x1280",
        "1:1" => "960x960",
        _ => "1280x720",
    }
}

async fn download_video(
    client: &Client,
    url: Url,
    api_key: &str,
    authenticated_host: Option<&str>,
    output_path: &Path,
    cancellation: CancellationToken,
) -> AppResult<String> {
    let mut request = client.get(url.clone());
    if url.host_str() == authenticated_host {
        request = request.bearer_auth(api_key);
    }
    let response = tokio::select! {
        _ = cancellation.cancelled() => return Err(AppError::new("CANCELLED", "本次运行已结束。", false)),
        response = request.send() => response.map_err(|error| connection_error(&error, "视频下载接口"))?,
    };
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(provider_error(status, &body));
    }
    response_to_file(response, output_path, 200 * 1024 * 1024, cancellation).await
}

async fn generate_siliconflow_video(
    input: VideoGenerationRequest,
    cancellation: CancellationToken,
) -> AppResult<GeneratedMedia> {
    let client = http_client()?;
    let submit_url = silicon_video_endpoint(input.base_url.clone(), "submit");
    let status_url = silicon_video_endpoint(input.base_url.clone(), "status");
    let mut body = serde_json::json!({
        "model": input.model_id,
        "prompt": input.prompt,
        "image_size": silicon_video_size(&input.ratio)
    });
    if let Some(image) = input.reference_image {
        body.as_object_mut()
            .ok_or_else(|| AppError::new("UNKNOWN", "无法生成视频请求。", false))?
            .insert("image".into(), image.into());
    }
    let response = tokio::select! {
        _ = cancellation.cancelled() => return Err(AppError::new("CANCELLED", "本次运行已结束。", false)),
        response = client.post(submit_url).bearer_auth(&input.api_key).json(&body).send() =>
            response.map_err(|error| connection_error(&error, "硅基流动视频生成接口"))?,
    };
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(provider_error(status, &body));
    }
    let task: serde_json::Value = response.json().await.map_err(|_| {
        AppError::new(
            "RESPONSE_INVALID",
            "硅基流动没有返回可识别的视频任务。",
            false,
        )
    })?;
    let request_id = task
        .get("requestId")
        .and_then(serde_json::Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| AppError::new("RESPONSE_INVALID", "硅基流动没有返回视频任务 ID。", false))?
        .to_string();
    loop {
        tokio::select! {
            _ = cancellation.cancelled() => return Err(AppError::new("CANCELLED", "本次运行已结束。", false)),
            _ = tokio::time::sleep(Duration::from_secs(5)) => {}
        }
        let response = tokio::select! {
            _ = cancellation.cancelled() => return Err(AppError::new("CANCELLED", "本次运行已结束。", false)),
            response = client.post(status_url.clone()).bearer_auth(&input.api_key).json(&serde_json::json!({"requestId": request_id})).send() =>
                response.map_err(|error| connection_error(&error, "硅基流动视频任务接口"))?,
        };
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(provider_error(status, &body));
        }
        let task: serde_json::Value = response.json().await.map_err(|_| {
            AppError::new(
                "RESPONSE_INVALID",
                "硅基流动视频任务返回了无法识别的数据。",
                false,
            )
        })?;
        match task
            .get("status")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
        {
            "Succeed" => {
                let url = task
                    .pointer("/results/videos/0/url")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|url| Url::parse(url).ok())
                    .ok_or_else(|| {
                        AppError::new("RESPONSE_INVALID", "硅基流动没有返回视频地址。", false)
                    })?;
                let path = download_video(
                    &client,
                    url,
                    &input.api_key,
                    input.base_url.host_str(),
                    &input.output_path,
                    cancellation,
                )
                .await?;
                return Ok(GeneratedMedia { data_url: path });
            }
            "Failed" => {
                let message = task
                    .get("reason")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("视频生成失败。");
                return Err(AppError::new("PROVIDER_ERROR", message, false));
            }
            _ => {}
        }
    }
}

async fn generate_video(
    input: VideoGenerationRequest,
    cancellation: CancellationToken,
) -> AppResult<GeneratedMedia> {
    if input.provider_kind == "siliconflow" {
        return generate_siliconflow_video(input, cancellation).await;
    }
    let client = http_client()?;
    let endpoint = video_endpoint(input.base_url.clone());
    let size = match input.ratio.as_str() {
        "9:16" => "720x1280",
        "4:3" => "1024x768",
        "3:4" => "768x1024",
        "1:1" => "1024x1024",
        _ => "1280x720",
    };
    let mut body = serde_json::json!({
        "model": input.model_id,
        "prompt": input.prompt,
        "size": size,
        "seconds": input.seconds
    });
    if let Some(image) = input.reference_image {
        body.as_object_mut()
            .ok_or_else(|| AppError::new("UNKNOWN", "无法生成视频请求。", false))?
            .insert("input_reference".into(), image.into());
    }
    let request = client
        .post(endpoint.clone())
        .bearer_auth(&input.api_key)
        .json(&body)
        .send();
    let response = tokio::select! {
        _ = cancellation.cancelled() => return Err(AppError::new("CANCELLED", "本次运行已结束。", false)),
        response = request => response.map_err(|error| connection_error(&error, "视频生成接口"))?,
    };
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(provider_error(status, &body));
    }
    let mut task: serde_json::Value = response
        .json()
        .await
        .map_err(|_| AppError::new("RESPONSE_INVALID", "视频接口返回了无法识别的数据。", false))?;
    let video_id = task
        .get("id")
        .and_then(serde_json::Value::as_str)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| AppError::new("RESPONSE_INVALID", "视频接口没有返回任务 ID。", false))?
        .to_string();
    let mut status_url = endpoint.clone();
    status_url.set_path(&format!(
        "{}/{}",
        endpoint.path().trim_end_matches('/'),
        video_id
    ));
    loop {
        match task
            .get("status")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
        {
            "completed" => break,
            "failed" | "cancelled" => {
                let message = task
                    .pointer("/error/message")
                    .or_else(|| task.get("error"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("视频生成失败。");
                return Err(AppError::new("PROVIDER_ERROR", message, false));
            }
            _ => {}
        }
        tokio::select! {
            _ = cancellation.cancelled() => return Err(AppError::new("CANCELLED", "本次运行已结束。", false)),
            _ = tokio::time::sleep(Duration::from_secs(5)) => {}
        }
        let request = client
            .get(status_url.clone())
            .bearer_auth(&input.api_key)
            .send();
        let response = tokio::select! {
            _ = cancellation.cancelled() => return Err(AppError::new("CANCELLED", "本次运行已结束。", false)),
            response = request => response.map_err(|error| connection_error(&error, "视频任务接口"))?,
        };
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(provider_error(status, &body));
        }
        task = response.json().await.map_err(|_| {
            AppError::new("RESPONSE_INVALID", "视频任务返回了无法识别的数据。", false)
        })?;
    }
    let content_url = task
        .get("url")
        .and_then(serde_json::Value::as_str)
        .and_then(|url| Url::parse(url).ok())
        .unwrap_or_else(|| {
            let mut url = status_url.clone();
            url.set_path(&format!(
                "{}/content",
                status_url.path().trim_end_matches('/')
            ));
            url
        });
    let path = download_video(
        &client,
        content_url,
        &input.api_key,
        input.base_url.host_str(),
        &input.output_path,
        cancellation,
    )
    .await?;
    Ok(GeneratedMedia { data_url: path })
}

fn parse_openai_response(body: OpenAiResponse) -> AppResult<GeneratedText> {
    let text = body
        .choices
        .first()
        .and_then(|choice| choice.message.content.as_deref())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| AppError::new("RESPONSE_INVALID", "模型没有返回可展示的文本。", false))?
        .to_string();
    let usage = body.usage.map(|usage| Usage {
        input_tokens: usage.prompt_tokens,
        output_tokens: usage.completion_tokens,
        total_tokens: usage.total_tokens,
    });
    Ok(GeneratedText { text, usage })
}

fn parse_catalog(body: CatalogResponse, output_type: &str) -> AppResult<Vec<CatalogOption>> {
    let mut models: Vec<CatalogOption> = body
        .data
        .into_iter()
        .filter_map(|item| {
            let model_id = item.id.or(item.model_id).or(item.model)?.trim().to_string();
            (!model_id.is_empty()).then(|| CatalogOption {
                model_id,
                output_type: output_type.to_string(),
                supports_reference_image: item
                    .input_modalities
                    .as_deref()
                    .unwrap_or_default()
                    .split(',')
                    .any(|modality| modality.trim() == "image"),
            })
        })
        .collect();
    models.sort_unstable_by_key(|model| model.model_id.to_lowercase());
    let mut deduplicated: Vec<CatalogOption> = Vec::with_capacity(models.len());
    for model in models {
        if let Some(existing) = deduplicated
            .last_mut()
            .filter(|existing| existing.model_id == model.model_id)
        {
            existing.supports_reference_image |= model.supports_reference_image;
        } else {
            deduplicated.push(model);
        }
    }
    let models = deduplicated;
    if models.is_empty() {
        return Err(AppError::new(
            "RESPONSE_INVALID",
            "平台没有返回可选择的模型。",
            false,
        ));
    }
    Ok(models)
}

#[tauri::command]
fn settings_get(state: State<'_, AppState>) -> AppResult<AppConfig> {
    load_config(&state.config_path)
}

#[tauri::command]
fn connection_key_get(connection_id: String, state: State<'_, AppState>) -> AppResult<String> {
    let config = load_config(&state.config_path)?;
    if !config
        .connections
        .iter()
        .any(|connection| connection.id == connection_id)
    {
        return Err(AppError::new("NOT_FOUND", "没有找到这个连接。", false));
    }
    get_key(state.inner(), &connection_id)
}

fn activate_output_type(config: &mut AppConfig, output_type: &str) {
    config.active_arena_type = output_type.to_string();
}

#[tauri::command]
fn arena_type_set(output_type: String, state: State<'_, AppState>) -> AppResult<AppConfig> {
    validate_output_type(&output_type)?;
    let mut config = load_config(&state.config_path)?;
    if !config
        .connections
        .iter()
        .flat_map(|connection| &connection.models)
        .any(|model| model.output_type == output_type)
    {
        return Err(AppError::new(
            "VALIDATION_ERROR",
            "当前没有这一类模型。",
            false,
        ));
    }
    activate_output_type(&mut config, &output_type);
    save_config(&state.config_path, &config)?;
    Ok(config)
}

async fn fetch_catalog(
    client: &Client,
    endpoint: Url,
    key: &str,
    target: &str,
    output_type: &str,
) -> AppResult<Vec<CatalogOption>> {
    let response = client
        .get(endpoint)
        .bearer_auth(key)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|error| connection_error(&error, target))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(provider_error(status, &body));
    }
    parse_catalog(
        response.json().await.map_err(|_| {
            AppError::new("RESPONSE_INVALID", "平台返回了无法识别的模型目录。", false)
        })?,
        output_type,
    )
}

fn text_probe_model(models: &[CatalogOption]) -> Option<&str> {
    for preferred in ["gpt-4o-mini", "gpt-5-nano"] {
        if let Some(model) = models
            .iter()
            .find(|model| model.model_id.eq_ignore_ascii_case(preferred))
        {
            return Some(&model.model_id);
        }
    }
    models
        .iter()
        .find(|model| model.model_id.to_ascii_lowercase().contains("-free"))
        .or_else(|| {
            models.iter().find(|model| {
                let id = model.model_id.to_ascii_lowercase();
                id.contains("mini") || id.contains("flash")
            })
        })
        .or_else(|| models.first())
        .map(|model| model.model_id.as_str())
}

async fn validate_integrated_key(
    client: &Client,
    provider_kind: &str,
    base_url: Url,
    api_key: &str,
    text_models: &[CatalogOption],
) -> AppResult<()> {
    let response = if provider_kind == "siliconflow" {
        let mut endpoint = base_url;
        endpoint.set_path(&format!(
            "{}/user/info",
            endpoint.path().trim_end_matches('/')
        ));
        client
            .get(endpoint)
            .bearer_auth(api_key)
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .map_err(|error| connection_error(&error, "硅基流动"))?
    } else {
        let model_id = text_probe_model(text_models).ok_or_else(|| {
            AppError::new(
                "RESPONSE_INVALID",
                "平台没有可用于验证 Key 的文本模型。",
                false,
            )
        })?;
        client
            .post(chat_endpoint(base_url))
            .bearer_auth(api_key)
            .timeout(Duration::from_secs(30))
            .json(&chat_request_body(
                model_id,
                serde_json::Value::String("1".into()),
                Some(1),
            ))
            .send()
            .await
            .map_err(|error| connection_error(&error, "AIHubMix"))?
    };
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let body = response.text().await.unwrap_or_default();
    Err(provider_error(status, &body))
}

async fn probe_text_model(
    client: Client,
    base_url: Url,
    api_key: String,
    model_id: String,
    provider_name: &'static str,
) -> AppResult<()> {
    let response = client
        .post(chat_endpoint(base_url))
        .bearer_auth(api_key)
        .timeout(Duration::from_secs(30))
        .json(&chat_request_body(
            &model_id,
            serde_json::Value::String("1".into()),
            Some(1),
        ))
        .send()
        .await
        .map_err(|error| connection_error(&error, provider_name))?;
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let body = response.text().await.unwrap_or_default();
    Err(provider_error(status, &body))
}

fn can_skip_failed_model(error: &AppError) -> bool {
    matches!(
        error.code,
        "MODEL_UNAVAILABLE" | "MODEL_UNSUPPORTED" | "MODEL_ACCESS_DENIED"
    )
}

fn disable_unavailable_models(config: &mut AppConfig, models: &[AutoDisabledModel]) -> bool {
    let mut changed = false;
    for connection in &mut config.connections {
        for model in &mut connection.models {
            if model.enabled
                && models
                    .iter()
                    .any(|failed| failed.model_config_id == model.id)
            {
                model.enabled = false;
                changed = true;
            }
        }
    }
    changed
}

fn provider_name(provider_kind: &str) -> &'static str {
    match provider_kind {
        "aihubmix" => "AIHubMix",
        "siliconflow" => "硅基流动",
        _ => "模型平台",
    }
}

fn should_probe_selected_models(provider_kind: &str, output_type: &str) -> bool {
    output_type == "text" && provider_kind != "openai_compatible"
}

#[tauri::command]
async fn provider_models(
    input: ConnectionProbeInput,
    state: State<'_, AppState>,
) -> AppResult<CatalogResult> {
    validate_output_type(&input.output_type)?;
    let (normalized_base, base_url) = normalize_base(&input.base_url)?;
    let key = effective_key(
        state.inner(),
        input.connection_id.as_deref(),
        input.api_key.as_deref(),
    )?;
    let target = provider_name(&input.provider_kind);
    let client = http_client()?;
    let mut models = fetch_catalog(
        &client,
        models_endpoint(&input.provider_kind, &input.output_type, base_url.clone()),
        &key,
        target,
        &input.output_type,
    )
    .await?;
    if input
        .api_key
        .as_deref()
        .map(str::trim)
        .is_some_and(|key| !key.is_empty())
    {
        let text_models = if input.provider_kind != "aihubmix" || input.output_type == "text" {
            None
        } else {
            Some(
                fetch_catalog(
                    &client,
                    models_endpoint(&input.provider_kind, "text", base_url.clone()),
                    &key,
                    target,
                    "text",
                )
                .await?,
            )
        };
        validate_integrated_key(
            &client,
            &input.provider_kind,
            base_url.clone(),
            &key,
            text_models.as_deref().unwrap_or(&models),
        )
        .await?;
    }
    if input.output_type == "audio" {
        let speech_to_text = fetch_catalog(
            &client,
            models_endpoint(&input.provider_kind, "audio_to_text", base_url),
            &key,
            target,
            "audio_to_text",
        )
        .await?;
        if input.provider_kind == "siliconflow" {
            models.retain(|model| {
                !speech_to_text
                    .iter()
                    .any(|speech| speech.model_id == model.model_id)
            });
        }
        models.extend(speech_to_text);
        models.sort_unstable_by_key(|model| model.model_id.to_lowercase());
    }
    let fingerprint = connection_fingerprint(
        input.connection_id.as_deref(),
        &input.provider_kind,
        &normalized_base,
        &key,
        &input.output_type,
    );
    Ok(CatalogResult {
        validation_token: remember_validation(state.inner(), fingerprint)?,
        models,
    })
}

#[tauri::command]
async fn connection_test(
    input: ConnectionProbeInput,
    state: State<'_, AppState>,
) -> AppResult<TestResult> {
    validate_output_type(&input.output_type)?;
    let model_id = input
        .model_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| AppError::new("VALIDATION_ERROR", "请输入模型 ID。", false))?;
    let (normalized_base, base_url) = normalize_base(&input.base_url)?;
    let key = effective_key(
        state.inner(),
        input.connection_id.as_deref(),
        input.api_key.as_deref(),
    )?;
    let started = Instant::now();
    let has_usage = match input.output_type.as_str() {
        "image" => {
            generate_image(
                ImageGenerationRequest {
                    provider_kind: input.provider_kind.clone(),
                    base_url,
                    api_key: key.clone(),
                    model_id: model_id.to_string(),
                    prompt: "一枚简洁的蓝色圆点".into(),
                    reference_image: None,
                    image_ratio: "1:1".into(),
                },
                CancellationToken::new(),
            )
            .await?;
            false
        }
        "audio" => {
            generate_audio(
                input.provider_kind.clone(),
                base_url,
                key.clone(),
                model_id.to_string(),
                "连接成功".into(),
                "alloy".into(),
                CancellationToken::new(),
            )
            .await?;
            false
        }
        "audio_to_text" => {
            transcribe_audio(
                base_url,
                key.clone(),
                model_id.to_string(),
                input.audio_input.ok_or_else(|| {
                    AppError::new("VALIDATION_ERROR", "请添加一段测试音频。", false)
                })?,
                CancellationToken::new(),
            )
            .await?;
            false
        }
        // 视频测试会真实计费且耗时较长；自定义连接在首次运行时验证具体协议。
        "video" => false,
        _ => generate_text(
            base_url,
            key.clone(),
            model_id.to_string(),
            CONNECTION_TEST_PROMPT.into(),
            None,
            CancellationToken::new(),
        )
        .await?
        .usage
        .is_some(),
    };
    let fingerprint = connection_fingerprint(
        input.connection_id.as_deref(),
        &input.provider_kind,
        &normalized_base,
        &key,
        &input.output_type,
    );
    Ok(TestResult {
        validation_token: remember_validation(state.inner(), fingerprint)?,
        has_usage,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

#[tauri::command]
async fn connection_save(
    mut input: ConnectionInput,
    state: State<'_, AppState>,
) -> AppResult<ConnectionSaveResult> {
    validate_output_type(&input.output_type)?;
    if input.display_name.trim().is_empty() {
        return Err(AppError::new("VALIDATION_ERROR", "请输入连接名称。", false));
    }
    if input.models.is_empty()
        || input.models.iter().any(|model| {
            model.model_id.trim().is_empty()
                || output_category(&model.output_type) != output_category(&input.output_type)
        })
    {
        return Err(AppError::new(
            "VALIDATION_ERROR",
            "请至少选择或填写一个模型。",
            false,
        ));
    }
    let (normalized_base, base_url) = normalize_base(&input.base_url)?;
    let key = effective_key(
        state.inner(),
        input.connection_id.as_deref(),
        input.api_key.as_deref(),
    )?;
    let expected = connection_fingerprint(
        input.connection_id.as_deref(),
        &input.provider_kind,
        &normalized_base,
        &key,
        &input.output_type,
    );
    let tested = state
        .validated_connections
        .lock()
        .map_err(|_| lock_error())?
        .get(&input.validation_token)
        .copied();
    if tested != Some(expected) {
        return Err(AppError::new(
            "VALIDATION_ERROR",
            "连接信息已变化，请重新验证后保存。",
            false,
        ));
    }

    let mut skipped_models = Vec::new();
    if should_probe_selected_models(&input.provider_kind, &input.output_type) {
        let target = provider_name(&input.provider_kind);
        let client = http_client()?;
        let probes = input.models.iter().map(|model| {
            probe_text_model(
                client.clone(),
                base_url.clone(),
                key.clone(),
                model.model_id.trim().to_string(),
                target,
            )
        });
        let results = join_all(probes).await;
        let mut available = Vec::new();
        for (model, result) in input.models.into_iter().zip(results) {
            match result {
                Ok(()) => available.push(model),
                Err(error) if can_skip_failed_model(&error) => skipped_models.push(SkippedModel {
                    model_id: model.model_id,
                    reason: error.message,
                }),
                Err(error) => return Err(error),
            }
        }
        input.models = available;
        if input.models.is_empty() {
            return Err(AppError::new(
                "VALIDATION_ERROR",
                format!("{target} 中所选模型当前都无法调用，已全部跳过，不会进入主页。"),
                false,
            ));
        }
    }
    state
        .validated_connections
        .lock()
        .map_err(|_| lock_error())?
        .remove(&input.validation_token);

    let mut config = load_config(&state.config_path)?;
    let existing_index = input
        .connection_id
        .as_ref()
        .and_then(|id| {
            config
                .connections
                .iter()
                .position(|connection| &connection.id == id)
        })
        .or_else(|| {
            (input.provider_kind != "openai_compatible").then(|| {
                config
                    .connections
                    .iter()
                    .position(|connection| connection.provider_kind == input.provider_kind)
            })?
        });
    let connection_id = input.connection_id.clone().unwrap_or_else(|| {
        existing_index
            .map(|index| config.connections[index].id.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string())
    });
    let existing_models = if input.connection_id.is_some() {
        existing_index
            .map(|index| config.connections[index].models.clone())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let output_type = input.output_type.clone();
    let edited_category = output_category(&output_type).to_string();
    let _ = input.supports_reference_image;
    let selected_models = input
        .models
        .into_iter()
        .map(|model| {
            let existing = existing_models.iter().find(|saved| {
                saved.model_id == model.model_id && saved.output_type == model.output_type
            });
            ModelConfig {
                id: existing
                    .map(|saved| saved.id.clone())
                    .unwrap_or_else(|| Uuid::new_v4().to_string()),
                model_id: model.model_id.trim().to_string(),
                display_name: model.display_name.trim().to_string(),
                output_type: model.output_type,
                supports_reference_image: model.supports_reference_image,
                enabled: existing.map(|saved| saved.enabled).unwrap_or(model.enabled),
            }
        })
        .collect::<Vec<_>>();
    let activation_output = selected_models
        .iter()
        .find(|model| model.enabled && model.output_type == config.active_arena_type)
        .or_else(|| selected_models.iter().find(|model| model.enabled))
        .map(|model| model.output_type.clone());
    let mut models = existing_models
        .into_iter()
        .filter(|model| {
            output_category(&model.output_type) != edited_category
                && !selected_models
                    .iter()
                    .any(|selected| selected.model_id == model.model_id)
        })
        .collect::<Vec<_>>();
    models.extend(selected_models);
    let connection = Connection {
        id: connection_id.clone(),
        display_name: input.display_name.trim().to_string(),
        provider_kind: input.provider_kind,
        base_url: normalized_base,
        has_credential: true,
        models,
    };

    let old_key = existing_index.and_then(|_| get_key(state.inner(), &connection_id).ok());
    set_key(state.inner(), &connection_id, &key)?;
    if let Some(active_output) = activation_output {
        activate_output_type(&mut config, &active_output);
    }
    if let Some(index) = existing_index {
        config.connections[index] = connection.clone();
    } else {
        config.connections.push(connection.clone());
    }
    if let Err(error) = save_config(&state.config_path, &config) {
        if let Some(old) = old_key {
            let _ = set_key(state.inner(), &connection_id, &old);
        } else {
            let _ = delete_key(state.inner(), &connection_id);
        }
        return Err(error);
    }
    Ok(ConnectionSaveResult {
        connection,
        skipped_models,
    })
}

#[tauri::command]
fn model_set_enabled(
    connection_id: String,
    model_config_id: String,
    enabled: bool,
    state: State<'_, AppState>,
) -> AppResult<ModelConfig> {
    let mut config = load_config(&state.config_path)?;
    let model = config
        .connections
        .iter_mut()
        .find(|connection| connection.id == connection_id)
        .and_then(|connection| {
            connection
                .models
                .iter_mut()
                .find(|model| model.id == model_config_id)
        })
        .ok_or_else(|| AppError::new("VALIDATION_ERROR", "没有找到这个模型。", false))?;
    model.enabled = enabled;
    let updated = model.clone();
    save_config(&state.config_path, &config)?;
    Ok(updated)
}

#[tauri::command]
fn model_remove(
    connection_id: String,
    model_config_id: String,
    state: State<'_, AppState>,
) -> AppResult<bool> {
    let mut config = load_config(&state.config_path)?;
    let connection = config
        .connections
        .iter_mut()
        .find(|connection| connection.id == connection_id)
        .ok_or_else(|| AppError::new("VALIDATION_ERROR", "没有找到这个连接。", false))?;
    let old_len = connection.models.len();
    connection
        .models
        .retain(|model| model.id != model_config_id);
    if old_len == connection.models.len() {
        return Err(AppError::new(
            "VALIDATION_ERROR",
            "没有找到这个模型。",
            false,
        ));
    }
    save_config(&state.config_path, &config)?;
    Ok(true)
}

fn clear_models(config: &mut AppConfig, output_type: &str, provider_kinds: &[String]) -> usize {
    let mut removed = 0;
    for connection in &mut config.connections {
        if !provider_kinds.contains(&connection.provider_kind) {
            continue;
        }
        let old_len = connection.models.len();
        connection
            .models
            .retain(|model| model.output_type != output_type);
        removed += old_len - connection.models.len();
    }
    removed
}

#[tauri::command]
fn models_clear(
    output_type: String,
    provider_kinds: Vec<String>,
    state: State<'_, AppState>,
) -> AppResult<usize> {
    validate_output_type(&output_type)?;
    let mut config = load_config(&state.config_path)?;
    let removed = clear_models(&mut config, &output_type, &provider_kinds);
    if removed > 0 {
        save_config(&state.config_path, &config)?;
    }
    Ok(removed)
}

#[tauri::command]
fn connection_remove(connection_id: String, state: State<'_, AppState>) -> AppResult<bool> {
    let mut config = load_config(&state.config_path)?;
    let original = config.clone();
    let old_len = config.connections.len();
    config
        .connections
        .retain(|connection| connection.id != connection_id);
    if old_len == config.connections.len() {
        return Err(AppError::new(
            "VALIDATION_ERROR",
            "没有找到这个连接。",
            false,
        ));
    }
    save_config(&state.config_path, &config)?;
    if let Err(error) = delete_key(state.inner(), &connection_id) {
        let _ = save_config(&state.config_path, &original);
        return Err(error);
    }
    Ok(true)
}

#[tauri::command]
fn text_run_start(
    input: RunInput,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<RunStarted> {
    let RunInput {
        prompt,
        reference_image,
        audio_input,
        image_ratio,
        audio_voice,
        video_seconds,
    } = input;
    let prompt = prompt.trim().to_string();
    let config = load_config(&state.config_path)?;
    let active_output_type = config.active_arena_type.clone();
    if active_output_type == "audio_to_text" {
        if audio_input.is_none() {
            return Err(AppError::new(
                "VALIDATION_ERROR",
                "请添加要转写的音频文件。",
                false,
            ));
        }
    } else if prompt.is_empty() {
        return Err(AppError::new("VALIDATION_ERROR", "请输入提示词。", false));
    }
    let mut targets = Vec::new();
    for connection in config.connections {
        let enabled: Vec<_> = connection
            .models
            .into_iter()
            .filter(|model| model.enabled && model.output_type == active_output_type)
            .collect();
        if enabled.is_empty() {
            continue;
        }
        let key = get_key(state.inner(), &connection.id)?;
        let (_, base_url) = normalize_base(&connection.base_url)?;
        for model in enabled {
            targets.push(RunTarget {
                model_config_id: model.id,
                model_id: model.model_id,
                provider_kind: connection.provider_kind.clone(),
                output_type: model.output_type,
                base_url: base_url.clone(),
                api_key: key.clone(),
            });
        }
    }
    if targets.is_empty() {
        return Err(AppError::new(
            "VALIDATION_ERROR",
            "请先在模型配置中启用至少一个同类型模型。",
            false,
        ));
    }

    let run_id = Uuid::new_v4().to_string();
    let media_dir = app
        .path()
        .app_cache_dir()
        .map_err(|_| AppError::new("FILE_WRITE_FAILED", "无法读取应用缓存目录。", true))?
        .join("generated")
        .join(&run_id);
    fs::create_dir_all(&media_dir)
        .map_err(|_| AppError::new("FILE_WRITE_FAILED", "无法创建媒体缓存目录。", true))?;
    let cancellation = CancellationToken::new();
    {
        let mut current = state.current_run.lock().map_err(|_| lock_error())?;
        if current.is_some() {
            return Err(AppError::new(
                "VALIDATION_ERROR",
                "当前运行尚未结束。",
                false,
            ));
        }
        *current = Some(CurrentRun {
            id: run_id.clone(),
            started: Instant::now(),
            cancellation: cancellation.clone(),
        });
    }

    let spawned_state = state.inner().clone();
    let spawned_run_id = run_id.clone();
    tauri::async_runtime::spawn(async move {
        let tasks = targets.into_iter().map(|target| {
            let app = app.clone();
            let prompt = prompt.clone();
            let reference_image = reference_image.clone();
            let audio_input = audio_input.clone();
            let image_ratio = image_ratio.clone().unwrap_or_else(|| "1:1".into());
            let audio_voice = audio_voice.clone().unwrap_or_else(|| "alloy".into());
            let video_seconds = video_seconds.clone().unwrap_or_else(|| "5".into());
            let cancellation = cancellation.clone();
            let run_id = spawned_run_id.clone();
            let media_dir = media_dir.clone();
            async move {
                let started = Instant::now();
                let model_config_id = target.model_config_id;
                let failed_model_id = target.model_id.clone();
                let failed_provider_name = provider_name(&target.provider_kind).to_string();
                let video_path = media_dir.join(format!("{model_config_id}.mp4"));
                let result: AppResult<GeneratedOutput> = match target.output_type.as_str() {
                    "image" => generate_image(
                        ImageGenerationRequest {
                            provider_kind: target.provider_kind,
                            base_url: target.base_url,
                            api_key: target.api_key,
                            model_id: target.model_id,
                            prompt,
                            reference_image,
                            image_ratio,
                        },
                        cancellation,
                    )
                    .await
                    .map(|output| GeneratedOutput {
                        text: None,
                        image: Some(output.image),
                        audio: None,
                        video: None,
                        usage: output.usage,
                    }),
                    "audio" => generate_audio(
                        target.provider_kind,
                        target.base_url,
                        target.api_key,
                        target.model_id,
                        prompt,
                        audio_voice,
                        cancellation,
                    )
                    .await
                    .map(|output| GeneratedOutput {
                        text: None,
                        image: None,
                        audio: Some(output.data_url),
                        video: None,
                        usage: None,
                    }),
                    "audio_to_text" => transcribe_audio(
                        target.base_url,
                        target.api_key,
                        target.model_id,
                        audio_input.unwrap_or_default(),
                        cancellation,
                    )
                    .await
                    .map(|output| GeneratedOutput {
                        text: Some(output.text),
                        image: None,
                        audio: None,
                        video: None,
                        usage: output.usage,
                    }),
                    "video" => generate_video(
                        VideoGenerationRequest {
                            provider_kind: target.provider_kind,
                            base_url: target.base_url,
                            api_key: target.api_key,
                            model_id: target.model_id,
                            prompt,
                            reference_image,
                            ratio: image_ratio,
                            seconds: video_seconds,
                            output_path: video_path,
                        },
                        cancellation,
                    )
                    .await
                    .map(|output| GeneratedOutput {
                        text: None,
                        image: None,
                        audio: None,
                        video: Some(output.data_url),
                        usage: None,
                    }),
                    _ => generate_text(
                        target.base_url,
                        target.api_key,
                        target.model_id,
                        prompt,
                        reference_image,
                        cancellation,
                    )
                    .await
                    .map(|output| GeneratedOutput {
                        text: Some(output.text),
                        image: None,
                        audio: None,
                        video: None,
                        usage: output.usage,
                    }),
                };
                let auto_disabled = result.as_ref().err().and_then(|error| {
                    can_skip_failed_model(error).then(|| AutoDisabledModel {
                        model_config_id: model_config_id.clone(),
                        model_id: failed_model_id,
                        provider_name: failed_provider_name,
                        reason: error.message.clone(),
                    })
                });
                let payload = match result {
                    Ok(output) => ModelRunFinished {
                        run_id,
                        model_config_id,
                        status: "completed",
                        output_text: output.text,
                        output_image: output.image,
                        output_audio: output.audio,
                        output_video: output.video,
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        usage: output.usage,
                        error: None,
                    },
                    Err(error) => ModelRunFinished {
                        run_id,
                        model_config_id,
                        status: if error.code == "CANCELLED" {
                            "ended"
                        } else {
                            "failed"
                        },
                        output_text: None,
                        output_image: None,
                        output_audio: None,
                        output_video: None,
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        usage: None,
                        error: Some(error),
                    },
                };
                let _ = app.emit("text-model-finished", payload);
                auto_disabled
            }
        });
        let auto_disabled = join_all(tasks)
            .await
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        if !auto_disabled.is_empty()
            && let Ok(mut config) = load_config(&spawned_state.config_path)
            && disable_unavailable_models(&mut config, &auto_disabled)
            && save_config(&spawned_state.config_path, &config).is_ok()
        {
            let _ = app.emit("models-auto-disabled", &auto_disabled);
        }
        let elapsed_ms = {
            let Ok(mut current) = spawned_state.current_run.lock() else {
                return;
            };
            let Some(active) = current.as_ref() else {
                return;
            };
            if active.id != spawned_run_id {
                return;
            }
            let elapsed = active.started.elapsed().as_millis() as u64;
            current.take();
            elapsed
        };
        let _ = app.emit(
            "text-run-finished",
            RunFinished {
                run_id: spawned_run_id,
                status: "completed",
                elapsed_ms,
            },
        );
    });
    Ok(RunStarted { run_id })
}

#[tauri::command]
fn text_run_cancel(
    run_id: String,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> AppResult<CancelResult> {
    let active = {
        let mut current = state.current_run.lock().map_err(|_| lock_error())?;
        match current.as_ref() {
            Some(active) if active.id == run_id => current.take(),
            _ => None,
        }
    };
    let Some(active) = active else {
        return Ok(CancelResult {
            status: "completed",
        });
    };
    active.cancellation.cancel();
    let _ = app.emit(
        "text-run-finished",
        RunFinished {
            run_id,
            status: "ended",
            elapsed_ms: active.started.elapsed().as_millis() as u64,
        },
    );
    Ok(CancelResult { status: "ended" })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let config_path = app.path().app_config_dir()?.join("config.json");
            app.manage(AppState {
                config_path,
                key_vault: Arc::new(Mutex::new(None)),
                validated_connections: Arc::new(Mutex::new(HashMap::new())),
                current_run: Arc::new(Mutex::new(None)),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            settings_get,
            connection_key_get,
            provider_models,
            connection_test,
            connection_save,
            arena_type_set,
            model_set_enabled,
            model_remove,
            models_clear,
            connection_remove,
            text_run_start,
            text_run_cancel
        ])
        .run(tauri::generate_context!())
        .expect("error while running model battle");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_and_usage() {
        let result = parse_openai_response(OpenAiResponse {
            choices: vec![OpenAiChoice {
                message: OpenAiMessage {
                    content: Some("  连接成功  ".into()),
                },
            }],
            usage: Some(OpenAiUsage {
                prompt_tokens: Some(4),
                completion_tokens: Some(2),
                total_tokens: Some(6),
            }),
        })
        .expect("response should parse");
        assert_eq!(result.text, "连接成功");
        assert_eq!(result.usage.and_then(|usage| usage.total_tokens), Some(6));
    }

    #[test]
    fn parses_mixed_model_catalog_fields() {
        let models = parse_catalog(
            CatalogResponse {
                data: vec![
                    CatalogModel {
                        id: Some("gpt-5".into()),
                        model_id: None,
                        model: None,
                        input_modalities: Some("text,image".into()),
                    },
                    CatalogModel {
                        id: None,
                        model_id: Some("deepseek-v3".into()),
                        model: None,
                        input_modalities: Some("text".into()),
                    },
                    CatalogModel {
                        id: Some("gpt-5".into()),
                        model_id: None,
                        model: None,
                        input_modalities: None,
                    },
                ],
            },
            "text",
        )
        .expect("catalog should parse");
        assert_eq!(
            models,
            vec![
                CatalogOption {
                    model_id: "deepseek-v3".into(),
                    output_type: "text".into(),
                    supports_reference_image: false,
                },
                CatalogOption {
                    model_id: "gpt-5".into(),
                    output_type: "text".into(),
                    supports_reference_image: true,
                },
            ]
        );
    }

    #[test]
    fn catalog_endpoints_filter_by_output_type() {
        let aihubmix = models_endpoint(
            "aihubmix",
            "image",
            Url::parse("https://aihubmix.com/v1").expect("url"),
        );
        assert_eq!(
            aihubmix.as_str(),
            "https://aihubmix.com/api/v1/models?type=image_generation"
        );
        let audio = models_endpoint(
            "aihubmix",
            "audio",
            Url::parse("https://aihubmix.com/v1").expect("url"),
        );
        assert_eq!(
            audio.as_str(),
            "https://aihubmix.com/api/v1/models?type=tts"
        );
        let speech_to_text = models_endpoint(
            "aihubmix",
            "audio_to_text",
            Url::parse("https://aihubmix.com/v1").expect("url"),
        );
        assert_eq!(
            speech_to_text.as_str(),
            "https://aihubmix.com/api/v1/models?type=stt"
        );
        let video = models_endpoint(
            "aihubmix",
            "video",
            Url::parse("https://aihubmix.com/v1").expect("url"),
        );
        assert_eq!(
            video.as_str(),
            "https://aihubmix.com/api/v1/models?type=video"
        );
        let siliconflow = models_endpoint(
            "siliconflow",
            "text",
            Url::parse("https://api.siliconflow.cn/v1").expect("url"),
        );
        assert!(siliconflow.as_str().contains("type=text"));
        assert!(siliconflow.as_str().contains("sub_type=chat"));
        let siliconflow_audio = models_endpoint(
            "siliconflow",
            "audio",
            Url::parse("https://api.siliconflow.cn/v1").expect("url"),
        );
        assert_eq!(
            siliconflow_audio.as_str(),
            "https://api.siliconflow.cn/v1/models?type=audio"
        );
        let siliconflow_stt = models_endpoint(
            "siliconflow",
            "audio_to_text",
            Url::parse("https://api.siliconflow.cn/v1").expect("url"),
        );
        assert_eq!(
            siliconflow_stt.as_str(),
            "https://api.siliconflow.cn/v1/models?sub_type=speech-to-text"
        );
        let siliconflow_video = models_endpoint(
            "siliconflow",
            "video",
            Url::parse("https://api.siliconflow.cn/v1").expect("url"),
        );
        assert_eq!(
            siliconflow_video.as_str(),
            "https://api.siliconflow.cn/v1/models?type=video"
        );
        assert_eq!(
            silicon_video_endpoint(
                Url::parse("https://api.siliconflow.cn/v1").expect("url"),
                "submit"
            )
            .as_str(),
            "https://api.siliconflow.cn/v1/video/submit"
        );
        assert_eq!(silicon_video_size("1:1"), "960x960");
        assert_eq!(silicon_video_size("9:16"), "720x1280");
        assert_eq!(silicon_video_size("16:9"), "1280x720");
    }

    #[test]
    fn changing_arena_type_keeps_other_models_enabled() {
        let mut config = AppConfig {
            schema_version: SCHEMA_VERSION,
            active_arena_type: "text".into(),
            connections: vec![Connection {
                id: "connection".into(),
                display_name: "test".into(),
                provider_kind: "aihubmix".into(),
                base_url: "https://aihubmix.com/v1".into(),
                has_credential: true,
                models: vec![
                    ModelConfig {
                        id: "text".into(),
                        model_id: "gpt-5".into(),
                        display_name: "gpt-5".into(),
                        output_type: "text".into(),
                        supports_reference_image: false,
                        enabled: true,
                    },
                    ModelConfig {
                        id: "image".into(),
                        model_id: "gpt-image-1".into(),
                        display_name: "gpt-image-1".into(),
                        output_type: "image".into(),
                        supports_reference_image: true,
                        enabled: false,
                    },
                ],
            }],
        };
        activate_output_type(&mut config, "image");
        assert_eq!(config.active_arena_type, "image");
        assert!(config.connections[0].models[0].enabled);
    }

    #[test]
    fn keeps_latest_integrated_platform_connection() {
        let model = |id: &str, output_type: &str| ModelConfig {
            id: format!("{id}-{output_type}"),
            model_id: id.into(),
            display_name: id.into(),
            output_type: output_type.into(),
            supports_reference_image: false,
            enabled: true,
        };
        let mut config = AppConfig {
            schema_version: SCHEMA_VERSION,
            active_arena_type: "text".into(),
            connections: vec![
                Connection {
                    id: "first".into(),
                    display_name: "AIHubMix".into(),
                    provider_kind: "aihubmix".into(),
                    base_url: "https://aihubmix.com/v1".into(),
                    has_credential: true,
                    models: vec![model("gpt-5", "text")],
                },
                Connection {
                    id: "second".into(),
                    display_name: "AIHubMix".into(),
                    provider_kind: "aihubmix".into(),
                    base_url: "https://aihubmix.com/v1".into(),
                    has_credential: true,
                    models: vec![model("gpt-5", "text"), model("flux", "image")],
                },
                Connection {
                    id: "siliconflow".into(),
                    display_name: "硅基流动".into(),
                    provider_kind: "siliconflow".into(),
                    base_url: "https://api.siliconflow.cn/v1".into(),
                    has_credential: true,
                    models: vec![model("deepseek", "text")],
                },
            ],
        };

        assert!(keep_latest_platform_connections(&mut config));
        assert_eq!(config.connections.len(), 2);
        assert_eq!(config.connections[0].id, "second");
        assert_eq!(config.connections[0].models.len(), 2);
        assert_eq!(config.connections[1].id, "siliconflow");
    }

    #[test]
    fn chooses_a_small_text_model_for_key_validation() {
        let model = |id: &str| CatalogOption {
            model_id: id.into(),
            output_type: "text".into(),
            supports_reference_image: false,
        };
        let models = vec![
            model("expensive-pro"),
            model("gpt-4o-mini"),
            model("free-model"),
        ];
        assert_eq!(text_probe_model(&models), Some("gpt-4o-mini"));
    }

    #[test]
    fn clears_one_model_type_from_selected_sources() {
        let model = |id: &str, output_type: &str| ModelConfig {
            id: format!("{id}-{output_type}"),
            model_id: id.into(),
            display_name: id.into(),
            output_type: output_type.into(),
            supports_reference_image: false,
            enabled: true,
        };
        let connection = |id: &str, provider_kind: &str, models: Vec<ModelConfig>| Connection {
            id: id.into(),
            display_name: id.into(),
            provider_kind: provider_kind.into(),
            base_url: "https://example.com/v1".into(),
            has_credential: true,
            models,
        };
        let mut config = AppConfig {
            schema_version: SCHEMA_VERSION,
            active_arena_type: "text".into(),
            connections: vec![
                connection("aihubmix", "aihubmix", vec![model("gpt", "text")]),
                connection(
                    "siliconflow",
                    "siliconflow",
                    vec![model("deepseek", "text"), model("flux", "image")],
                ),
            ],
        };

        assert_eq!(
            clear_models(&mut config, "text", &["siliconflow".into()]),
            1
        );
        assert_eq!(config.connections[0].models.len(), 1);
        assert_eq!(config.connections[1].models.len(), 1);
        assert_eq!(config.connections[1].models[0].output_type, "image");
    }

    #[test]
    fn migrates_legacy_gemini_image_model_type() {
        let mut config = AppConfig {
            schema_version: SCHEMA_VERSION,
            active_arena_type: "text".into(),
            connections: vec![Connection {
                id: "connection".into(),
                display_name: "AIHubMix".into(),
                provider_kind: "aihubmix".into(),
                base_url: "https://aihubmix.com/v1".into(),
                has_credential: true,
                models: vec![
                    ModelConfig {
                        id: "text".into(),
                        model_id: "gpt-5".into(),
                        display_name: "gpt-5".into(),
                        output_type: "text".into(),
                        supports_reference_image: false,
                        enabled: true,
                    },
                    ModelConfig {
                        id: "legacy-image".into(),
                        model_id: "gemini-3-pro-image".into(),
                        display_name: "gemini-3-pro-image".into(),
                        output_type: "text".into(),
                        supports_reference_image: false,
                        enabled: true,
                    },
                ],
            }],
        };

        assert!(migrate_legacy_model_types(&mut config));
        assert_eq!(config.connections[0].models[1].output_type, "image");
        assert!(config.connections[0].models[1].supports_reference_image);
        assert!(config.connections[0].models[1].enabled);
        assert_eq!(config.active_arena_type, "text");
    }

    #[test]
    fn parses_image_url_and_base64_responses() {
        let url = parse_image_response(serde_json::json!({
            "images": [{"url": "https://example.com/result.png"}]
        }))
        .expect("url image");
        assert_eq!(url.image, "https://example.com/result.png");
        let base64 = parse_image_response(serde_json::json!({
            "data": [{"b64_json": "YWJj"}]
        }))
        .expect("base64 image");
        assert_eq!(base64.image, "data:image/png;base64,YWJj");
    }

    #[test]
    fn builds_provider_specific_aihubmix_image_endpoints() {
        let base = Url::parse("https://aihubmix.com/v1").expect("base url");
        assert_eq!(
            aihubmix_prediction_endpoint(base.clone(), "qwen-image").as_str(),
            "https://aihubmix.com/v1/models/qianfan/qwen-image/predictions"
        );
        assert_eq!(
            aihubmix_gemini_endpoint(base, "gemini-3-pro-image").as_str(),
            "https://aihubmix.com/gemini/v1beta/models/gemini-3-pro-image:generateContent"
        );
    }

    #[test]
    fn parses_gemini_inline_image_response() {
        let output = parse_gemini_image_response(serde_json::json!({
            "candidates": [{
                "content": {"parts": [{
                    "inlineData": {"mimeType": "image/webp", "data": "YWJj"}
                }]}
            }]
        }))
        .expect("inline image");
        assert_eq!(output.image, "data:image/webp;base64,YWJj");
    }

    #[test]
    fn surfaces_provider_error_detail_without_raw_body() {
        let error = provider_error(
            StatusCode::BAD_REQUEST,
            r#"{"error":{"message":"model does not support this endpoint"}}"#,
        );
        assert!(
            error
                .message
                .contains("model does not support this endpoint")
        );
        assert_eq!(error.provider_status, Some(400));
    }

    #[test]
    fn classifies_model_access_errors_without_calling_the_key_invalid() {
        let denied = provider_error(
            StatusCode::FORBIDDEN,
            r#"{"error":{"message":"Access denied"}}"#,
        );
        assert_eq!(denied.code, "MODEL_ACCESS_DENIED");
        assert!(!denied.message.contains("Key 无效"));

        let disabled = provider_error(
            StatusCode::FORBIDDEN,
            r#"{"error":{"message":"Model disabled."}}"#,
        );
        assert_eq!(disabled.code, "MODEL_UNAVAILABLE");
        assert!(disabled.message.contains("停用"));
    }

    #[test]
    fn disables_qwen_thinking_for_non_streaming_requests() {
        let qwen = chat_request_body("qwen3-14b", serde_json::Value::String("hi".into()), None);
        assert_eq!(qwen["enable_thinking"], false);

        let other = chat_request_body("gpt-5", serde_json::Value::String("hi".into()), None);
        assert!(other.get("enable_thinking").is_none());
    }

    #[test]
    fn only_skips_permanently_unavailable_models_when_saving() {
        let unavailable = AppError::new("MODEL_UNAVAILABLE", "disabled", false);
        let timeout = AppError::new("CONNECTION_TIMEOUT", "timeout", true);
        assert!(can_skip_failed_model(&unavailable));
        assert!(!can_skip_failed_model(&timeout));
    }

    #[test]
    fn probes_selected_text_models_for_every_integrated_provider() {
        assert!(should_probe_selected_models("aihubmix", "text"));
        assert!(should_probe_selected_models("siliconflow", "text"));
        assert!(!should_probe_selected_models("openai_compatible", "text"));
        assert!(!should_probe_selected_models("siliconflow", "image"));
    }

    #[test]
    fn disables_permanently_failed_models_after_a_run() {
        let mut config = AppConfig {
            schema_version: SCHEMA_VERSION,
            active_arena_type: "text".into(),
            connections: vec![Connection {
                id: "siliconflow".into(),
                display_name: "硅基流动".into(),
                provider_kind: "siliconflow".into(),
                base_url: "https://api.siliconflow.cn/v1".into(),
                has_credential: true,
                models: vec![ModelConfig {
                    id: "failed-model-config".into(),
                    model_id: "broken-model".into(),
                    display_name: "broken-model".into(),
                    output_type: "text".into(),
                    supports_reference_image: false,
                    enabled: true,
                }],
            }],
        };
        let failed_id = config.connections[0].models[0].id.clone();
        let disabled = disable_unavailable_models(
            &mut config,
            &[AutoDisabledModel {
                model_config_id: failed_id,
                model_id: "broken-model".into(),
                provider_name: "硅基流动".into(),
                reason: "当前 API Key 无权调用该模型。".into(),
            }],
        );
        assert!(disabled);
        assert!(!config.connections[0].models[0].enabled);
    }

    #[test]
    fn timeout_error_names_target_and_recovery() {
        let error = connection_error_for(true, "AIHubMix");
        assert_eq!(error.code, "CONNECTION_TIMEOUT");
        assert!(error.message.contains("AIHubMix"));
        assert!(error.message.contains("全局代理"));
    }

    #[test]
    fn rejects_empty_response() {
        let result = parse_openai_response(OpenAiResponse {
            choices: vec![],
            usage: None,
        });
        assert_eq!(
            result.expect_err("empty response must fail").code,
            "RESPONSE_INVALID"
        );
    }

    #[test]
    fn config_round_trip_and_backup_recovery() {
        let directory = std::env::temp_dir().join(format!("model-battle-{}", Uuid::new_v4()));
        let path = directory.join("config.json");
        let mut config = AppConfig::default();
        save_config(&path, &config).expect("first save");
        config.active_arena_type = "text-updated".into();
        save_config(&path, &config).expect("second save");
        fs::write(&path, b"not json").expect("corrupt current config");
        let recovered = load_config(&path).expect("backup should recover");
        assert_eq!(recovered.active_arena_type, "text");
        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[test]
    fn rejects_newer_config_schema() {
        let result =
            parse_config(br#"{"schemaVersion":999,"activeArenaType":"text","connections":[]}"#);
        assert_eq!(
            result.expect_err("new schema must fail").code,
            "CONFIG_CORRUPTED"
        );
    }
}
