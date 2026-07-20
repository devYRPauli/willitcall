#![allow(dead_code)]

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{Json, State};
use axum::http::{header, Response, StatusCode};
use axum::routing::{get, post};
use axum::Router;
use tokio::task::JoinHandle;

const JSON_FIXTURE: &str = include_str!("../fixtures/chat-completion.json");
const SSE_FIXTURE: &str = include_str!("../fixtures/chat-completion.sse");

pub struct MockServer {
    address: SocketAddr,
    task: JoinHandle<()>,
    state: Arc<Mutex<MockState>>,
}

pub enum ScriptedResponse {
    Json(String),
    Sse(String),
    Status(u16, String),
}

struct MockState {
    model: String,
    models_status: StatusCode,
    responses: VecDeque<ScriptedResponse>,
    requests: Vec<serde_json::Value>,
}

impl MockServer {
    pub async fn start() -> Self {
        Self::start_with("fixture-model", StatusCode::OK, Vec::new()).await
    }

    pub async fn start_scripted(model: &str, responses: Vec<ScriptedResponse>) -> Self {
        Self::start_with(model, StatusCode::OK, responses).await
    }

    pub async fn start_preflight_failure() -> Self {
        Self::start_with("fixture-model", StatusCode::SERVICE_UNAVAILABLE, Vec::new()).await
    }

    async fn start_with(
        model: &str,
        models_status: StatusCode,
        responses: Vec<ScriptedResponse>,
    ) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let address = listener.local_addr().expect("mock server address");
        let state = Arc::new(Mutex::new(MockState {
            model: model.to_owned(),
            models_status,
            responses: responses.into(),
            requests: Vec::new(),
        }));
        let app = Router::new()
            .route("/v1/models", get(models))
            .route("/v1/chat/completions", post(chat_completions))
            .route("/api/version", get(server_version))
            .route("/props", get(llamacpp_props))
            .route("/version", get(server_version))
            .with_state(Arc::clone(&state));
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("run mock server");
        });

        Self {
            address,
            task,
            state,
        }
    }

    pub fn chat_completions_url(&self) -> String {
        format!("http://{}/v1/chat/completions", self.address)
    }

    pub fn endpoint(&self) -> String {
        format!("http://{}/v1", self.address)
    }

    pub fn port(&self) -> u16 {
        self.address.port()
    }

    pub fn requests(&self) -> Vec<serde_json::Value> {
        self.state.lock().expect("mock state lock").requests.clone()
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn models(State(state): State<Arc<Mutex<MockState>>>) -> Response<Body> {
    let state = state.lock().expect("mock state lock");
    let body = if state.models_status.is_success() {
        serde_json::json!({"object": "list", "data": [{"id": state.model}]}).to_string()
    } else {
        serde_json::json!({"error": "injected preflight failure"}).to_string()
    };
    Response::builder()
        .status(state.models_status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .expect("valid models response")
}

async fn server_version() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({"version": "mock-1.0"}))
}

async fn llamacpp_props() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({"build_info": "mock-build"}))
}

async fn chat_completions(
    State(state): State<Arc<Mutex<MockState>>>,
    Json(request): Json<serde_json::Value>,
) -> Response<Body> {
    let stream = request
        .get("stream")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let scripted = {
        let mut state = state.lock().expect("mock state lock");
        state.requests.push(request);
        state.responses.pop_front()
    };

    let (status, content_type, body) = match scripted {
        Some(ScriptedResponse::Json(body)) => (StatusCode::OK, "application/json", body),
        Some(ScriptedResponse::Sse(body)) => (StatusCode::OK, "text/event-stream", body),
        Some(ScriptedResponse::Status(status, body)) => (
            StatusCode::from_u16(status).expect("valid scripted status"),
            "application/json",
            body,
        ),
        None if stream => (StatusCode::OK, "text/event-stream", SSE_FIXTURE.to_owned()),
        None => (StatusCode::OK, "application/json", JSON_FIXTURE.to_owned()),
    };

    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(body))
        .expect("valid fixture response")
}
