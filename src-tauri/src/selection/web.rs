// Adapted from DeepSeek Harness web-deepseek-api (MIT); see THIRD_PARTY_NOTICES.txt.
use super::*;

const API_BASE: &str = "https://api.deepseek.com/v1/tools";

pub(super) struct DeepSeekWeb<'a> {
    client: Client,
    base: String,
    key: &'a str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    fn mock(
        status: &str,
        body: &str,
        extra_headers: &str,
    ) -> (
        DeepSeekWeb<'static>,
        thread::JoinHandle<(String, serde_json::Value)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/v1/tools", listener.local_addr().unwrap());
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{extra_headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut received = Vec::new();
            let (headers, body) = loop {
                let mut chunk = [0; 4096];
                let len = stream.read(&mut chunk).unwrap();
                assert!(len > 0);
                received.extend_from_slice(&chunk[..len]);
                if let Some(end) = received.windows(4).position(|item| item == b"\r\n\r\n") {
                    let headers = String::from_utf8(received[..end].to_vec()).unwrap();
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|value| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    if received.len() >= end + 4 + length {
                        break (
                            headers,
                            serde_json::from_slice(&received[end + 4..end + 4 + length]).unwrap(),
                        );
                    }
                }
            };
            stream.write_all(response.as_bytes()).unwrap();
            (headers, body)
        });
        let mut web = DeepSeekWeb::new("test-only-key").unwrap();
        web.base = base;
        web.client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        (web, server)
    }

    #[tokio::test]
    async fn uses_the_harness_search_contract_without_an_auxiliary_model_call() {
        let (web, server) = mock(
            "200 OK",
            r#"{"results":[{"title":"Official docs","url":"https://example.com/docs","snippet":"verified excerpt","date":null}]}"#,
            "",
        );
        let pages = web.search("model documentation").await.unwrap();
        assert_eq!(pages[0].url, "https://example.com/docs");
        let (headers, body) = server.join().unwrap();
        assert!(headers.starts_with("POST /v1/tools/search "));
        assert!(
            headers
                .to_lowercase()
                .contains("authorization: bearer test-only-key")
        );
        assert_eq!(
            body,
            serde_json::json!({"query":"model documentation","count":3,"need_content":true})
        );
        assert!(body.get("model").is_none());
    }

    #[tokio::test]
    async fn fetches_markdown_and_rejects_malformed_search_results() {
        let (web, server) = mock(
            "200 OK",
            r##"{"title":"Docs","content":"# Model details\nVerified content"}"##,
            "",
        );
        assert!(
            web.fetch("https://example.com/docs")
                .await
                .unwrap()
                .contains("Verified content")
        );
        let (headers, body) = server.join().unwrap();
        assert!(headers.starts_with("POST /v1/tools/fetch "));
        assert_eq!(body, serde_json::json!({"url":"https://example.com/docs"}));
        let (web, server) = mock(
            "200 OK",
            r#"{"results":[{"url":"https://example.com"}]}"#,
            "",
        );
        assert!(web.search("docs").await.is_err());
        server.join().unwrap();
    }

    #[tokio::test]
    async fn bounds_search_queries_to_the_official_128_unicode_character_limit() {
        let (web, server) = mock("200 OK", r#"{"results":[]}"#, "");
        assert!(web.search(&"模🧩".repeat(100)).await.unwrap().is_empty());
        let (_, body) = server.join().unwrap();
        let query = body["query"].as_str().unwrap();
        assert_eq!(query.chars().count(), 128);
        assert!(query.ends_with('🧩'));
        assert!(web.search("   ").await.is_err());
        assert!(
            status_error("DeepSeek 官网网页读取", 404)
                .message
                .contains("目标网页正文")
        );
    }

    #[tokio::test]
    async fn keeps_http_causes_without_leaking_keys_or_following_redirects() {
        for (status, headers, expected) in [
            ("401 Unauthorized", "", "认证失败"),
            ("429 Too Many Requests", "", "限流"),
            (
                "302 Found",
                "Location: https://example.invalid/steal\r\n",
                "重定向",
            ),
        ] {
            let (web, server) = mock(
                status,
                r#"{"error":{"message":"private detail sk-secret"}}"#,
                headers,
            );
            let error = web.search("docs").await.err().unwrap();
            assert!(error.message.contains(expected));
            assert!(!error.message.contains("sk-secret"));
            assert!(!error.message.contains("test-only-key"));
            server.join().unwrap();
        }
    }
}

#[derive(Deserialize)]
pub(super) struct SearchPage {
    pub(super) title: String,
    pub(super) url: String,
    snippet: String,
    #[serde(default)]
    pub(super) content: String,
}

pub(super) fn status_error(service: &str, status: u16) -> AppError {
    let reason = match status {
        400 => "请求参数无效，请检查搜索词长度和参数格式",
        401 => "认证失败，请在模型配置中检查官网 API Key",
        402 => "账户额度受限，请检查官网账户状态",
        403 => "拒绝访问，可能需要服务权限或网站访问验证",
        404 if service == "DeepSeek 官网网页读取" => {
            "未取得目标网页正文，可能被网站策略拦截或没有可提取内容"
        }
        404 => "接口或页面不存在，当前来源不可用",
        429 => "请求被限流，请稍后重试",
        500..=599 => "服务暂时异常，请稍后重试",
        300..=399 => "返回了重定向，已停止请求以保护 API Key",
        _ => "请求失败",
    };
    validation(&format!("{service}：{reason}（HTTP {status}）。"))
}

pub(super) async fn bounded_text(
    mut response: reqwest::Response,
    service: &str,
) -> AppResult<String> {
    if !response.status().is_success() {
        return Err(status_error(service, response.status().as_u16()));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| validation(&format!("{service}：响应读取中断。")))?
    {
        if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
            return Err(validation(&format!(
                "{service}：响应超过 2 MB，本页未采用。"
            )));
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes).map_err(|_| validation(&format!("{service}：响应编码无效。")))
}

impl<'a> DeepSeekWeb<'a> {
    pub(super) fn new(key: &'a str) -> AppResult<Self> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(8))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("Model-Battle/1.1")
            .build()
            .map_err(|_| validation("无法初始化联网检索。"))?;
        Ok(Self {
            client,
            base: API_BASE.into(),
            key,
        })
    }

    async fn request(
        &self,
        operation: &str,
        body: serde_json::Value,
    ) -> AppResult<serde_json::Value> {
        let service = if operation == "search" {
            "DeepSeek 官网搜索"
        } else {
            "DeepSeek 官网网页读取"
        };
        let response = self
            .client
            .post(format!("{}/{operation}", self.base))
            .bearer_auth(self.key)
            .header("Accept", "application/json")
            .json(&body)
            .timeout(Duration::from_secs(25))
            .send()
            .await
            .map_err(|error| {
                validation(&format!(
                    "{service}：{}。",
                    if error.is_timeout() {
                        "请求超过 25 秒，请稍后重试"
                    } else {
                        "连接失败，请检查网络或代理"
                    }
                ))
            })?;
        let text = bounded_text(response, service).await?;
        serde_json::from_str(&text).map_err(|_| validation(&format!("{service}：响应格式无效。")))
    }

    pub(super) async fn search(&self, query: &str) -> AppResult<Vec<SearchPage>> {
        #[derive(Deserialize)]
        struct SearchResponse {
            results: Vec<SearchPage>,
        }
        let query: String = query
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(128)
            .collect();
        if query.is_empty() {
            return Err(validation("搜索词不能为空。"));
        }
        let response = self
            .request(
                "search",
                serde_json::json!({"query":query,"count":3,"need_content":true}),
            )
            .await?;
        let parsed: SearchResponse = serde_json::from_value(response)
            .map_err(|_| validation("DeepSeek 官网搜索：结果格式无效。"))?;
        if parsed.results.iter().any(|item| {
            item.url.is_empty()
                || item.snippet.len() > 2 * 1024 * 1024
                || item.content.len() > 2 * 1024 * 1024
        }) {
            return Err(validation("DeepSeek 官网搜索：结果格式无效。"));
        }
        Ok(parsed.results)
    }

    pub(super) async fn fetch(&self, url: &str) -> AppResult<String> {
        let response = self
            .request("fetch", serde_json::json!({"url":url}))
            .await?;
        response
            .get("content")
            .and_then(|item| item.as_str())
            .filter(|text| !text.trim().is_empty())
            .map(str::to_owned)
            .ok_or_else(|| validation("DeepSeek 官网网页读取：未返回有效正文。"))
    }
}
