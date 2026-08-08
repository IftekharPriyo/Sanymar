use std::time::Duration;

use async_trait::async_trait;
use reqwest::{redirect::Policy, Client, StatusCode};
use serde::{Deserialize, Serialize};
use url::Url;

use super::{
    contract::{
        corrective_instruction, output_schema, parse_and_validate_output, system_instruction,
    },
    prompt::build_prompt,
    ScriptCandidate, ScriptGenerator, ScriptGeneratorError, ScriptRequest,
};

const API_BASE: &str = "https://api.groq.com/openai/v1/";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);
const MAX_OUTPUT_ATTEMPTS: usize = 2;

#[derive(Clone, Debug)]
pub struct GroqConfiguration {
    base_url: Url,
    model: String,
    api_key: String,
}

impl GroqConfiguration {
    pub fn new(base_url: &str, model: &str, api_key: &str) -> Result<Self, ScriptGeneratorError> {
        let base_url = validate_https_base_url(base_url)?;
        let model = validate_model(model)?;
        let api_key = api_key.trim();
        if api_key.is_empty() || api_key.len() > 512 || api_key.chars().any(char::is_control) {
            return Err(ScriptGeneratorError::InvalidConfiguration);
        }
        Ok(Self {
            base_url,
            model,
            api_key: api_key.to_owned(),
        })
    }

    fn endpoint(&self, path: &str) -> Result<Url, ScriptGeneratorError> {
        self.base_url
            .join(path)
            .map_err(|_| ScriptGeneratorError::InvalidConfiguration)
    }
}

pub fn default_base_url() -> String {
    API_BASE.into()
}

pub fn default_model() -> String {
    "qwen/qwen3.6-27b".into()
}

pub fn validate_https_base_url(value: &str) -> Result<Url, ScriptGeneratorError> {
    let mut url = Url::parse(value).map_err(|_| ScriptGeneratorError::InvalidConfiguration)?;
    let valid_scheme = url.scheme() == "https" || cfg!(test) && url.scheme() == "http";
    if !valid_scheme
        || url.host_str().is_none()
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(ScriptGeneratorError::InvalidConfiguration);
    }
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(url)
}

pub fn validate_model(value: &str) -> Result<String, ScriptGeneratorError> {
    let model = value.trim();
    if model.is_empty()
        || model.len() > 200
        || model.chars().any(char::is_control)
        || model.contains(char::is_whitespace)
    {
        return Err(ScriptGeneratorError::InvalidConfiguration);
    }
    Ok(model.to_owned())
}

#[derive(Clone)]
pub struct GroqScriptGenerator {
    configuration: GroqConfiguration,
    client: Client,
}

impl GroqScriptGenerator {
    pub fn new(configuration: GroqConfiguration) -> Result<Self, ScriptGeneratorError> {
        Self::with_timeout(configuration, REQUEST_TIMEOUT)
    }

    fn with_timeout(
        configuration: GroqConfiguration,
        request_timeout: Duration,
    ) -> Result<Self, ScriptGeneratorError> {
        let client = Client::builder()
            .redirect(Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(request_timeout)
            .build()
            .map_err(|_| ScriptGeneratorError::InvalidConfiguration)?;
        Ok(Self {
            configuration,
            client,
        })
    }

    pub async fn health_check(&self) -> Result<GroqHealth, ScriptGeneratorError> {
        let model_available = self.model_available().await?;
        let response = self
            .send_chat(
                "Return only this JSON object: {\"dialogue\":\"ready\",\"factIds\":[]}",
                "{}",
                Some(128),
            )
            .await?;
        let ready = response
            .content()
            .and_then(extract_json_object)
            .is_some_and(|content| serde_json::from_str::<serde_json::Value>(content).is_ok());
        Ok(GroqHealth {
            reachable: true,
            authenticated: true,
            model: self.configuration.model.clone(),
            model_available,
            ready,
        })
    }

    async fn model_available(&self) -> Result<bool, ScriptGeneratorError> {
        let response = self
            .client
            .get(self.configuration.endpoint("models")?)
            .bearer_auth(&self.configuration.api_key)
            .send()
            .await
            .map_err(map_request_error)?;
        if !response.status().is_success() {
            return Err(map_status_response(response).await);
        }
        let models: ModelsResponse = response
            .json()
            .await
            .map_err(|_| ScriptGeneratorError::MalformedOutput)?;
        Ok(models
            .data
            .iter()
            .any(|model| model.id == self.configuration.model))
    }

    async fn generate_inner(
        &self,
        request: &ScriptRequest,
    ) -> Result<Vec<ScriptCandidate>, ScriptGeneratorError> {
        let prompt = build_prompt(request)?;
        let schema = output_schema();
        let mut system = system_instruction(&prompt.system, &schema, None)?;
        let mut last_error = None;

        for attempt in 0..MAX_OUTPUT_ATTEMPTS {
            let response = self.send_chat(&system, &prompt.user, None).await?;
            let content = response
                .content()
                .ok_or(ScriptGeneratorError::MalformedOutput)?;
            let candidate_json = extract_json_object(content).unwrap_or(content);
            match parse_and_validate_output(request, &prompt.verified_fact_ids, candidate_json) {
                Ok(output) => {
                    return Ok(vec![ScriptCandidate {
                        dialogue: output.dialogue.trim().to_owned(),
                        fact_ids: output.fact_ids,
                    }]);
                }
                Err(error) => {
                    if attempt + 1 < MAX_OUTPUT_ATTEMPTS {
                        let correction = corrective_instruction(&error, request.maximum_words);
                        system = system_instruction(&prompt.system, &schema, Some(&correction))?;
                    }
                    last_error = Some(error);
                }
            }
        }

        Err(last_error
            .map(ScriptGeneratorError::from)
            .unwrap_or(ScriptGeneratorError::MalformedOutput))
    }

    async fn send_chat(
        &self,
        system: &str,
        user: &str,
        max_completion_tokens: Option<u16>,
    ) -> Result<ChatCompletionResponse, ScriptGeneratorError> {
        let response = self
            .client
            .post(self.configuration.endpoint("chat/completions")?)
            .bearer_auth(&self.configuration.api_key)
            .json(&ChatCompletionRequest {
                model: &self.configuration.model,
                messages: [
                    ChatMessage {
                        role: "system",
                        content: system,
                    },
                    ChatMessage {
                        role: "user",
                        content: user,
                    },
                ],
                stream: false,
                temperature: 0.0,
                reasoning_effort: "none",
                max_completion_tokens,
            })
            .send()
            .await
            .map_err(map_request_error)?;
        if !response.status().is_success() {
            return Err(map_status_response(response).await);
        }
        response
            .json()
            .await
            .map_err(|_| ScriptGeneratorError::MalformedOutput)
    }
}

#[async_trait]
impl ScriptGenerator for GroqScriptGenerator {
    async fn generate(
        &self,
        request: ScriptRequest,
    ) -> Result<Vec<ScriptCandidate>, ScriptGeneratorError> {
        let cancellation = request.cancellation.clone();
        tokio::select! {
            _ = cancellation.cancelled() => Err(ScriptGeneratorError::Cancelled),
            result = self.generate_inner(&request) => result,
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GroqHealth {
    pub reachable: bool,
    pub authenticated: bool,
    pub model: String,
    pub model_available: bool,
    pub ready: bool,
}

#[derive(Serialize)]
struct ChatCompletionRequest<'a> {
    model: &'a str,
    messages: [ChatMessage<'a>; 2],
    stream: bool,
    temperature: f32,
    reasoning_effort: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_completion_tokens: Option<u16>,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

impl ChatCompletionResponse {
    fn content(&self) -> Option<&str> {
        self.choices
            .first()
            .and_then(|choice| choice.message.content.as_deref())
    }
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(Deserialize)]
struct ChatChoiceMessage {
    content: Option<String>,
}

#[derive(Deserialize)]
struct ModelsResponse {
    data: Vec<ModelRecord>,
}

#[derive(Deserialize)]
struct ModelRecord {
    id: String,
}

fn map_request_error(error: reqwest::Error) -> ScriptGeneratorError {
    if error.is_timeout() {
        ScriptGeneratorError::Timeout
    } else {
        ScriptGeneratorError::Unavailable
    }
}

async fn map_status_response(response: reqwest::Response) -> ScriptGeneratorError {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ScriptGeneratorError::Authentication,
        StatusCode::BAD_REQUEST => ScriptGeneratorError::ProviderRejected(provider_reason(&body)),
        StatusCode::NOT_FOUND => ScriptGeneratorError::ModelUnavailable,
        StatusCode::TOO_MANY_REQUESTS => ScriptGeneratorError::RateLimited,
        _ => {
            if body.trim().is_empty() {
                ScriptGeneratorError::Unavailable
            } else {
                ScriptGeneratorError::ProviderRejected(provider_reason(&body))
            }
        }
    }
}

fn provider_reason(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return "Groq returned an empty error response".into();
    }
    let parsed = serde_json::from_str::<serde_json::Value>(trimmed).ok();
    let message = parsed
        .as_ref()
        .and_then(|value| value.pointer("/error/message"))
        .and_then(|value| value.as_str())
        .or_else(|| {
            parsed
                .as_ref()
                .and_then(|value| value.get("message"))
                .and_then(|value| value.as_str())
        })
        .unwrap_or(trimmed);
    sanitize_provider_reason(message)
}

fn sanitize_provider_reason(message: &str) -> String {
    let mut sanitized = crate::errors::redact_sensitive(message);
    sanitized = sanitized.replace(['\n', '\r'], " ");
    sanitized.truncate(240);
    sanitized
}

fn extract_json_object(content: &str) -> Option<&str> {
    let mut start = None;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (index, character) in content.char_indices() {
        if start.is_none() {
            if character == '{' {
                start = Some(index);
                depth = 1;
            }
            continue;
        }

        if escaped {
            escaped = false;
            continue;
        }
        if in_string {
            match character {
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }

        match character {
            '"' => in_string = true,
            '{' => depth = depth.saturating_add(1),
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let start = start?;
                    return content.get(start..=index);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use chrono::NaiveDate;
    use serde_json::json;
    use tokio_util::sync::CancellationToken;
    use wiremock::{
        matchers::{body_partial_json, header, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    use crate::{
        music_provider::{Album, Artist, Track, TrackVariant},
        rj_engine::{BroadcastMemory, DjProfile, SegmentFocus, SegmentPlan, SegmentType},
    };

    use super::*;

    fn request(cancellation: CancellationToken) -> ScriptRequest {
        ScriptRequest {
            plan: SegmentPlan {
                segment_type: SegmentType::SimpleTransition,
                focus: SegmentFocus::PreviousTrack,
                target_words: 20,
                fact_ids: Vec::new(),
                use_station_lore: false,
            },
            profile: DjProfile::default(),
            previous_track: Some(Track {
                provider_id: "spotify-secret-id".into(),
                title: "Quiet Signal".into(),
                artists: vec![Artist {
                    provider_id: Some("artist-secret-id".into()),
                    name: "Signal Club".into(),
                }],
                album: Some(Album {
                    provider_id: Some("album-secret-id".into()),
                    title: "After Midnight".into(),
                    release_date: NaiveDate::from_ymd_opt(2024, 1, 1),
                    artwork_url: Some("https://secret.invalid/art.jpg".into()),
                }),
                duration_ms: 100_000,
                isrc: Some("SECRET-ISRC".into()),
                release_date: NaiveDate::from_ymd_opt(2024, 1, 1),
                explicit: false,
                variant: TrackVariant::Studio,
                artwork_url: Some("https://secret.invalid/track.jpg".into()),
            }),
            next_track: None,
            facts: Vec::new(),
            memory: BroadcastMemory::default(),
            maximum_words: 30,
            cancellation,
        }
    }

    fn generator(server: &MockServer) -> GroqScriptGenerator {
        let configuration = GroqConfiguration::new(&server.uri(), "qwen/qwen3.6-27b", "test-key")
            .unwrap_or_else(|error| panic!("test configuration failed: {error}"));
        GroqScriptGenerator::with_timeout(configuration, Duration::from_secs(2))
            .unwrap_or_else(|error| panic!("test client failed: {error}"))
    }

    #[tokio::test]
    async fn sends_openai_compatible_structured_chat_without_provider_secrets() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("authorization", "Bearer test-key"))
            .and(body_partial_json(json!({
                "model": "qwen/qwen3.6-27b",
                "stream": false,
                "temperature": 0.0,
                "reasoning_effort": "none"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": "{\"dialogue\":\"That track left a clean little echo behind.\",\"factIds\":[]}"
                    }
                }]
            })))
            .mount(&server)
            .await;

        let candidates = generator(&server)
            .generate(request(CancellationToken::new()))
            .await
            .unwrap_or_else(|error| panic!("generation failed: {error}"));
        assert_eq!(candidates.len(), 1);

        let received = server
            .received_requests()
            .await
            .unwrap_or_else(|| panic!("mock server did not retain requests"));
        let body = String::from_utf8_lossy(&received[0].body);
        assert!(body.contains("Required JSON schema"));
        assert!(!body.contains("spotify-secret-id"));
        assert!(!body.contains("artist-secret-id"));
        assert!(!body.contains("SECRET-ISRC"));
        assert!(!body.contains("secret.invalid"));
    }

    #[tokio::test]
    async fn health_check_lists_models_before_chat_probe() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"id": "qwen/qwen3.6-27b"}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "message": {
                        "content": "{\"dialogue\":\"ready\",\"factIds\":[]}"
                    }
                }]
            })))
            .mount(&server)
            .await;

        let health = generator(&server)
            .health_check()
            .await
            .unwrap_or_else(|error| panic!("health check failed: {error}"));
        assert!(health.model_available);
        assert!(health.ready);
    }

    #[tokio::test]
    async fn maps_bad_request_body_without_secrets() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "error": {"message": "unsupported parameter access_token=secret-value"}
            })))
            .mount(&server)
            .await;

        let result = generator(&server)
            .generate(request(CancellationToken::new()))
            .await;
        match result {
            Err(ScriptGeneratorError::ProviderRejected(message)) => {
                assert!(message.contains("unsupported parameter"));
                assert!(!message.contains("secret-value"));
            }
            other => panic!("expected provider rejection, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn extracts_json_object_from_wrapped_qwen_output() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "message": {
                        "content": "<think>brief plan</think>\n```json\n{\"dialogue\":\"That track left a clean little echo behind.\",\"factIds\":[]}\n```"
                    }
                }]
            })))
            .mount(&server)
            .await;

        let candidates = generator(&server)
            .generate(request(CancellationToken::new()))
            .await
            .unwrap_or_else(|error| panic!("generation failed: {error}"));
        assert_eq!(
            candidates[0].dialogue,
            "That track left a clean little echo behind."
        );
    }

    #[tokio::test]
    async fn maps_cloud_authentication_failure() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let result = generator(&server)
            .generate(request(CancellationToken::new()))
            .await;
        assert!(matches!(result, Err(ScriptGeneratorError::Authentication)));
    }

    #[tokio::test]
    async fn cancellation_stops_in_flight_cloud_request() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_secs(1))
                    .set_body_json(json!({"choices":[{"message":{"content":"{}"}}]})),
            )
            .mount(&server)
            .await;
        let cancellation = CancellationToken::new();
        let cancellation_for_task = cancellation.clone();
        let generator = generator(&server);
        let task =
            tokio::spawn(async move { generator.generate(request(cancellation_for_task)).await });
        tokio::time::sleep(Duration::from_millis(30)).await;
        cancellation.cancel();

        let result = task
            .await
            .unwrap_or_else(|error| panic!("generation task failed: {error}"));
        assert!(matches!(result, Err(ScriptGeneratorError::Cancelled)));
    }
}
