use std::time::Duration;

use async_trait::async_trait;
use reqwest::{redirect::Policy, Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use super::{
    contract::{
        corrective_instruction, output_schema, parse_and_validate_output, system_instruction,
    },
    prompt::build_prompt,
    ScriptCandidate, ScriptGenerator, ScriptGeneratorError, ScriptRequest,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_OUTPUT_ATTEMPTS: usize = 2;

#[derive(Clone, Debug)]
pub struct OllamaConfiguration {
    base_url: Url,
    model: String,
}

impl OllamaConfiguration {
    pub fn new(base_url: &str, model: &str) -> Result<Self, ScriptGeneratorError> {
        let base_url = validate_local_base_url(base_url)?;
        let model = model.trim();
        if model.is_empty() || model.len() > 200 || model.chars().any(char::is_control) {
            return Err(ScriptGeneratorError::InvalidConfiguration);
        }
        Ok(Self {
            base_url,
            model: model.to_owned(),
        })
    }

    fn endpoint(&self, path: &str) -> Result<Url, ScriptGeneratorError> {
        self.base_url
            .join(path)
            .map_err(|_| ScriptGeneratorError::InvalidConfiguration)
    }
}

pub fn validate_local_base_url(value: &str) -> Result<Url, ScriptGeneratorError> {
    let mut url = Url::parse(value).map_err(|_| ScriptGeneratorError::InvalidConfiguration)?;
    let is_loopback = match url.host_str() {
        Some("localhost") => true,
        Some(host) => host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback()),
        None => false,
    };
    if url.scheme() != "http"
        || !is_loopback
        || url.port().is_none()
        || (url.path() != "/" && !url.path().is_empty())
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(ScriptGeneratorError::InvalidConfiguration);
    }
    url.set_path("/");
    Ok(url)
}

#[derive(Clone)]
pub struct OllamaScriptGenerator {
    configuration: OllamaConfiguration,
    client: Client,
}

impl OllamaScriptGenerator {
    pub fn new(configuration: OllamaConfiguration) -> Result<Self, ScriptGeneratorError> {
        Self::with_timeout(configuration, REQUEST_TIMEOUT)
    }

    fn with_timeout(
        configuration: OllamaConfiguration,
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

    pub async fn health_check(&self) -> Result<OllamaHealth, ScriptGeneratorError> {
        let version_response = self
            .client
            .get(self.configuration.endpoint("api/version")?)
            .send()
            .await
            .map_err(map_request_error)?;
        if !version_response.status().is_success() {
            return Err(map_status(version_response.status()));
        }
        let version: VersionResponse = version_response
            .json()
            .await
            .map_err(|_| ScriptGeneratorError::MalformedOutput)?;

        let tags_response = self
            .client
            .get(self.configuration.endpoint("api/tags")?)
            .send()
            .await
            .map_err(map_request_error)?;
        if !tags_response.status().is_success() {
            return Err(map_status(tags_response.status()));
        }
        let tags: TagsResponse = tags_response
            .json()
            .await
            .map_err(|_| ScriptGeneratorError::MalformedOutput)?;
        let model_installed = tags.models.iter().any(|model| {
            model.name == self.configuration.model || model.model == self.configuration.model
        });
        Ok(OllamaHealth {
            reachable: true,
            model_configured: true,
            model_installed,
            model: self.configuration.model.clone(),
            version: version.version,
        })
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
            let response = self
                .send_chat(&system, &prompt.user, schema.clone())
                .await?;
            match parse_and_validate_output(
                request,
                &prompt.verified_fact_ids,
                &response.message.content,
            ) {
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
        format: Value,
    ) -> Result<ChatResponse, ScriptGeneratorError> {
        let response = self
            .client
            .post(self.configuration.endpoint("api/chat")?)
            .json(&ChatRequest {
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
                format,
                options: ChatOptions { temperature: 0 },
            })
            .send()
            .await
            .map_err(map_request_error)?;
        if !response.status().is_success() {
            return Err(map_status(response.status()));
        }
        response
            .json()
            .await
            .map_err(|_| ScriptGeneratorError::MalformedOutput)
    }
}

#[async_trait]
impl ScriptGenerator for OllamaScriptGenerator {
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
pub struct OllamaHealth {
    pub reachable: bool,
    pub model_configured: bool,
    pub model_installed: bool,
    pub model: String,
    pub version: Option<String>,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: [ChatMessage<'a>; 2],
    stream: bool,
    format: Value,
    options: ChatOptions,
}

#[derive(Serialize)]
struct ChatOptions {
    temperature: u8,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatResponse {
    message: ChatResponseMessage,
}

#[derive(Deserialize)]
struct ChatResponseMessage {
    content: String,
}

#[derive(Deserialize)]
struct VersionResponse {
    version: Option<String>,
}

#[derive(Deserialize)]
struct TagsResponse {
    #[serde(default)]
    models: Vec<TaggedModel>,
}

#[derive(Deserialize)]
struct TaggedModel {
    #[serde(default)]
    name: String,
    #[serde(default)]
    model: String,
}

fn map_request_error(error: reqwest::Error) -> ScriptGeneratorError {
    if error.is_timeout() {
        ScriptGeneratorError::Timeout
    } else {
        ScriptGeneratorError::Unavailable
    }
}

fn map_status(status: StatusCode) -> ScriptGeneratorError {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ScriptGeneratorError::Authentication,
        StatusCode::NOT_FOUND => ScriptGeneratorError::ModelUnavailable,
        StatusCode::TOO_MANY_REQUESTS => ScriptGeneratorError::RateLimited,
        _ => ScriptGeneratorError::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use chrono::NaiveDate;
    use serde_json::json;
    use tokio_util::sync::CancellationToken;
    use wiremock::{
        matchers::{body_partial_json, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    use crate::{
        llm::ScriptOutputError,
        music_provider::{Album, Artist, Track, TrackVariant},
        rj_engine::{
            BroadcastMemory, DjProfile, SegmentFocus, SegmentPlan, SegmentType, ValidationIssue,
        },
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
            profile: DjProfile {
                running_jokes: vec!["private fixed callback must not enter the prompt".into()],
                ..DjProfile::default()
            },
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

    fn generator(server: &MockServer) -> OllamaScriptGenerator {
        let configuration = OllamaConfiguration::new(&server.uri(), "llama-test:latest")
            .unwrap_or_else(|error| panic!("test configuration failed: {error}"));
        OllamaScriptGenerator::with_timeout(configuration, Duration::from_secs(2))
            .unwrap_or_else(|error| panic!("test client failed: {error}"))
    }

    #[test]
    fn normalizes_generated_dialogue_before_local_validation() {
        let script_request = request(CancellationToken::new());
        let response = ChatResponse {
            message: ChatResponseMessage {
                content: serde_json::to_string(&json!({
                    "dialogue": "Sanymar: ‘Quiet Signal’ is still glowing. [warm] Keep moving. 🎧",
                    "factIds": []
                }))
                .unwrap_or_else(|error| panic!("fixture failed: {error}")),
            },
        };

        let output = parse_and_validate_output(
            &script_request,
            &std::collections::HashSet::new(),
            &response.message.content,
        )
        .unwrap_or_else(|error| panic!("output failed: {error}"));
        assert_eq!(
            output.dialogue,
            "Quiet Signal is still glowing. Keep moving."
        );
    }

    #[tokio::test]
    async fn health_check_reports_installed_model() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/version"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"version":"0.9.0"})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "models": [{"name":"llama-test:latest", "model":"llama-test:latest"}]
            })))
            .mount(&server)
            .await;

        let health = generator(&server)
            .health_check()
            .await
            .unwrap_or_else(|error| panic!("health check failed: {error}"));
        assert!(health.reachable);
        assert!(health.model_installed);
        assert_eq!(health.version.as_deref(), Some("0.9.0"));
    }

    #[tokio::test]
    async fn health_check_reports_missing_model_without_installing_it() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/version"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"version":"0.9.0"})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "models": [{"name":"another-model:latest"}]
            })))
            .mount(&server)
            .await;

        let health = generator(&server)
            .health_check()
            .await
            .unwrap_or_else(|error| panic!("health check failed: {error}"));
        assert!(!health.model_installed);
        let received = server
            .received_requests()
            .await
            .unwrap_or_else(|| panic!("mock server did not retain requests"));
        assert!(received
            .iter()
            .all(|request| request.method.as_str() == "GET"));
    }

    #[tokio::test]
    async fn sends_non_streaming_structured_chat_without_provider_secrets() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .and(body_partial_json(json!({
                "model": "llama-test:latest",
                "stream": false,
                "options": {"temperature": 0}
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "message": {"role":"assistant", "content":"{\"dialogue\":\"That track left a clean little echo behind.\",\"factIds\":[]}"},
                "done": true
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
        assert!(body.contains("additionalProperties"));
        assert!(body.contains("Required JSON schema"));
        assert!(!body.contains("spotify-secret-id"));
        assert!(!body.contains("artist-secret-id"));
        assert!(!body.contains("SECRET-ISRC"));
        assert!(!body.contains("secret.invalid"));
        assert!(!body.contains("private fixed callback"));
    }

    #[tokio::test]
    async fn rejects_malformed_structured_output() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "message": {"content":"not json"}
            })))
            .mount(&server)
            .await;

        let result = generator(&server)
            .generate(request(CancellationToken::new()))
            .await;
        assert!(matches!(
            result,
            Err(ScriptGeneratorError::InvalidOutput(
                ScriptOutputError::JsonContract
            ))
        ));
        let received = server
            .received_requests()
            .await
            .unwrap_or_else(|| panic!("mock server did not retain requests"));
        assert_eq!(received.len(), MAX_OUTPUT_ATTEMPTS);
    }

    #[tokio::test]
    async fn retries_once_with_safe_correction_after_dialogue_validation_fails() {
        let server = MockServer::start().await;
        let script_request = request(CancellationToken::new());
        let prompt = build_prompt(&script_request)
            .unwrap_or_else(|error| panic!("test prompt failed: {error}"));
        let schema = output_schema();
        let base_system = system_instruction(&prompt.system, &schema, None)
            .unwrap_or_else(|error| panic!("test system prompt failed: {error}"));
        let retry_error = ScriptOutputError::Dialogue {
            issues: vec![ValidationIssue::TooLong],
        };
        let correction = corrective_instruction(&retry_error, script_request.maximum_words);
        let retry_system = system_instruction(&prompt.system, &schema, Some(&correction))
            .unwrap_or_else(|error| panic!("test retry prompt failed: {error}"));
        let too_long = vec!["word"; 31].join(" ");

        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .and(body_partial_json(json!({
                "messages": [
                    {"role":"system", "content":base_system},
                    {"role":"user", "content":prompt.user}
                ]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "message": {"content": serde_json::to_string(&json!({
                    "dialogue": too_long,
                    "factIds": []
                })).unwrap_or_else(|error| panic!("test response failed: {error}"))}
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .and(body_partial_json(json!({
                "messages": [
                    {"role":"system", "content":retry_system},
                    {"role":"user", "content":prompt.user}
                ]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "message": {"content":"{\"dialogue\":\"A clean little echo to carry into the next tune.\",\"factIds\":[]}"}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let candidates = generator(&server)
            .generate(script_request)
            .await
            .unwrap_or_else(|error| panic!("generation failed: {error}"));
        assert_eq!(
            candidates[0].dialogue,
            "A clean little echo to carry into the next tune."
        );

        let received = server
            .received_requests()
            .await
            .unwrap_or_else(|| panic!("mock server did not retain requests"));
        let retry_body = String::from_utf8_lossy(&received[1].body);
        assert!(retry_body.contains("corrective retry"));
        assert!(retry_body.contains("no more than 22 words"));
        assert!(!retry_body.contains(&too_long));
    }

    #[tokio::test]
    async fn cancellation_stops_an_in_flight_request() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_secs(1))
                    .set_body_json(json!({"message":{"content":"{}"}})),
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

    #[tokio::test]
    async fn maps_request_deadline_to_typed_timeout() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(100))
                    .set_body_json(json!({"message":{"content":"{}"}})),
            )
            .mount(&server)
            .await;
        let configuration = OllamaConfiguration::new(&server.uri(), "llama-test:latest")
            .unwrap_or_else(|error| panic!("test configuration failed: {error}"));
        let generator =
            OllamaScriptGenerator::with_timeout(configuration, Duration::from_millis(10))
                .unwrap_or_else(|error| panic!("test client failed: {error}"));

        let result = generator.generate(request(CancellationToken::new())).await;
        assert!(matches!(result, Err(ScriptGeneratorError::Timeout)));
    }

    #[test]
    fn rejects_non_loopback_base_url() {
        assert!(matches!(
            OllamaConfiguration::new("http://example.com:11434", "model"),
            Err(ScriptGeneratorError::InvalidConfiguration)
        ));
    }
}
