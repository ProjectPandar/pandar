use anyhow::{Context, Result, bail};
use axum::{
    Router,
    extract::State,
    http::{HeaderValue, StatusCode, Uri, header::CONNECTION},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{process::Command, sync::Arc, time::Duration};
use tokio::{net::TcpListener, sync::Mutex, sync::oneshot};

use super::secret::SecretString;

pub(crate) const DEFAULT_BROWSER_LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_CALLBACK_QUERY_BYTES: usize = 4096;

#[derive(Clone)]
struct CallbackState {
    expected_state: String,
    sender: Arc<Mutex<Option<oneshot::Sender<SecretString>>>>,
}

#[derive(Debug, Serialize)]
struct ExchangeRequest<'a> {
    ticket: &'a str,
    code_verifier: &'a str,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ExchangeResponse {
    pub token: SecretString,
    #[allow(dead_code)]
    pub expires_at: String,
    pub profile: ExchangeProfile,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub(crate) struct ExchangeProfile {
    pub user_id: String,
    pub user_name: String,
    pub tenant_id: String,
    pub tenant_name: String,
}

pub(crate) struct BrowserLoginSession {
    pub authorize_url: String,
    pub callback_url: String,
    code_verifier: SecretString,
    receiver: oneshot::Receiver<SecretString>,
    shutdown_tx: oneshot::Sender<()>,
    server_task: tokio::task::JoinHandle<()>,
}

impl BrowserLoginSession {
    pub(crate) async fn start(web_url: &str, tenant_id: Option<&str>) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .context("failed to bind local CLI callback listener")?;
        let port = listener
            .local_addr()
            .context("failed to inspect local CLI callback address")?
            .port();
        let callback_url = format!("http://127.0.0.1:{port}/callback");
        let (code_verifier, code_challenge) = generate_pkce_pair();
        let state = uuid::Uuid::new_v4().simple().to_string();

        let mut authorize_url = reqwest::Url::parse(&format!("{web_url}/cli-sign-in"))
            .context("failed to build CLI sign-in URL")?;
        {
            let mut pairs = authorize_url.query_pairs_mut();
            pairs.append_pair("redirect_url", &callback_url);
            pairs.append_pair("code_challenge", &code_challenge);
            pairs.append_pair("state", &state);
            if let Some(tenant_id) = tenant_id {
                pairs.append_pair("tenant", tenant_id);
            }
        }

        let (tx, rx) = oneshot::channel();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let callback_state = CallbackState {
            expected_state: state,
            sender: Arc::new(Mutex::new(Some(tx))),
        };
        let app = Router::new()
            .route("/callback", get(handle_callback))
            .with_state(callback_state);

        let server_task = tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await;
        });

        Ok(Self {
            authorize_url: authorize_url.to_string(),
            callback_url,
            code_verifier,
            receiver: rx,
            shutdown_tx,
            server_task,
        })
    }

    pub(crate) async fn wait_and_exchange(
        mut self,
        http: &reqwest::Client,
        hub_url: &str,
        timeout: Duration,
    ) -> Result<ExchangeResponse> {
        let received = tokio::time::timeout(timeout, self.receiver).await;
        let _ = self.shutdown_tx.send(());
        if tokio::time::timeout(Duration::from_millis(500), &mut self.server_task)
            .await
            .is_err()
        {
            self.server_task.abort();
        }

        let ticket = match received {
            Ok(Ok(ticket)) => ticket,
            Ok(Err(_)) => bail!("CLI sign-in callback channel closed before receiving a ticket"),
            Err(_) => bail!("timed out waiting for browser sign-in callback"),
        };

        let url = format!("{hub_url}/api/v1/cli/login-tickets/exchange");
        let response = http
            .post(&url)
            .json(&ExchangeRequest {
                ticket: ticket.expose(),
                code_verifier: self.code_verifier.expose(),
            })
            .send()
            .await
            .with_context(|| format!("failed to exchange CLI login ticket at {url}"))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .context("failed to read CLI ticket exchange response body")?;
        if !status.is_success() {
            bail!("CLI login ticket exchange failed with HTTP {status}: {body}");
        }
        serde_json::from_str::<ExchangeResponse>(&body)
            .context("failed to parse CLI ticket exchange response")
    }
}

fn html_response(status: StatusCode, body: &'static str) -> Response {
    let mut response = (status, Html(body)).into_response();
    response
        .headers_mut()
        .insert(CONNECTION, HeaderValue::from_static("close"));
    response
}

async fn handle_callback(State(state): State<CallbackState>, uri: Uri) -> Response {
    let raw_query = uri.query().unwrap_or("");
    if raw_query.len() > MAX_CALLBACK_QUERY_BYTES {
        return html_response(
            StatusCode::URI_TOO_LONG,
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>Pandar CLI</title></head>\
             <body><h1>Callback query too large</h1></body></html>",
        );
    }

    let parsed = match parse_callback_query(raw_query) {
        Ok(parsed) => parsed,
        Err(_) => {
            return html_response(
                StatusCode::BAD_REQUEST,
                "<!doctype html><html><head><meta charset=\"utf-8\"><title>Pandar CLI</title></head>\
                 <body><h1>Invalid CLI sign-in callback</h1><p>Return to your terminal and try again.</p></body></html>",
            );
        }
    };

    if parsed.state != state.expected_state {
        return html_response(
            StatusCode::BAD_REQUEST,
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>Pandar CLI</title></head>\
             <body><h1>Sign-in state mismatch</h1><p>Return to your terminal and restart pandar login.</p></body></html>",
        );
    }

    let mut guard = state.sender.lock().await;
    let Some(sender) = guard.take() else {
        return html_response(
            StatusCode::GONE,
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>Pandar CLI</title></head>\
             <body><h1>Sign-in already completed</h1><p>You can close this tab.</p></body></html>",
        );
    };
    let _ = sender.send(parsed.ticket);

    html_response(
        StatusCode::OK,
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Pandar CLI</title></head>\
         <body style=\"font-family:system-ui,sans-serif;max-width:32rem;margin:4rem auto;padding:0 1rem\">\
         <h1>Signed in to Pandar CLI</h1>\
         <p>You can close this tab and return to your terminal.</p>\
         </body></html>",
    )
}

struct ParsedCallbackQuery {
    ticket: SecretString,
    state: String,
}

fn parse_callback_query(raw_query: &str) -> Result<ParsedCallbackQuery> {
    let synthetic = reqwest::Url::parse(&format!("http://127.0.0.1/callback?{raw_query}"))
        .context("malformed callback query")?;
    let mut ticket: Option<SecretString> = None;
    let mut state: Option<String> = None;
    for (key, value) in synthetic.query_pairs() {
        match key.as_ref() {
            "ticket" => {
                if ticket.is_some() || value.trim().is_empty() {
                    bail!("duplicate or empty ticket parameter");
                }
                ticket = Some(SecretString::new(value.into_owned()));
            }
            "state" => {
                if state.is_some() || value.trim().is_empty() {
                    bail!("duplicate or empty state parameter");
                }
                state = Some(value.into_owned());
            }
            _ => {}
        }
    }
    let Some(ticket) = ticket else {
        bail!("missing ticket parameter");
    };
    let Some(state) = state else {
        bail!("missing state parameter");
    };
    Ok(ParsedCallbackQuery { ticket, state })
}

pub(crate) fn generate_pkce_pair() -> (SecretString, String) {
    let verifier = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    (SecretString::new(verifier), challenge)
}

pub(crate) fn try_open_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    let status = Command::new("open").arg(url).status();

    #[cfg(target_os = "windows")]
    let status = Command::new("cmd").args(["/C", "start", "", url]).status();

    #[cfg(all(unix, not(target_os = "macos")))]
    let status = Command::new("xdg-open").arg(url).status();

    let status = status.context("failed to launch system browser")?;
    if !status.success() {
        bail!("browser launcher exited with status {status}");
    }
    Ok(())
}
