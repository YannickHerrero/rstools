use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use crate::model::HttpMethod;

/// Command sent from the UI thread to the executor thread.
#[derive(Debug)]
pub struct HttpRequestCmd {
    pub method: HttpMethod,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

/// Result received from the executor thread.
#[derive(Debug)]
pub struct HttpResponseResult {
    pub status_code: u16,
    pub status_text: String,
    pub elapsed_ms: u128,
    pub size_bytes: usize,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

/// Error from a failed request.
#[derive(Debug)]
pub struct HttpRequestError {
    pub message: String,
}

/// The result type sent back from the executor.
pub type ExecutorResult = Result<HttpResponseResult, HttpRequestError>;

/// Sender/Receiver pair for communicating with the executor.
pub struct HttpExecutor {
    pub sender: mpsc::Sender<HttpRequestCmd>,
    pub receiver: mpsc::Receiver<ExecutorResult>,
}

impl HttpExecutor {
    /// Spawn the background executor thread with a tokio runtime.
    pub fn spawn() -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<HttpRequestCmd>();
        let (result_tx, result_rx) = mpsc::channel::<ExecutorResult>();

        thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create tokio runtime");

            rt.block_on(async move {
                while let Ok(cmd) = cmd_rx.recv() {
                    let result = execute_request(cmd).await;
                    if result_tx.send(result).is_err() {
                        break; // Main thread dropped the receiver
                    }
                }
            });
        });

        Self {
            sender: cmd_tx,
            receiver: result_rx,
        }
    }

    /// Send a request command (non-blocking).
    pub fn send(&self, cmd: HttpRequestCmd) -> Result<(), mpsc::SendError<HttpRequestCmd>> {
        self.sender.send(cmd)
    }

    /// Try to receive a result (non-blocking).
    pub fn try_recv(&self) -> Option<ExecutorResult> {
        self.receiver.try_recv().ok()
    }
}

/// Execute an HTTP request using reqwest.
async fn execute_request(cmd: HttpRequestCmd) -> ExecutorResult {
    let client = reqwest::Client::new();

    let method = match cmd.method {
        HttpMethod::Get => reqwest::Method::GET,
        HttpMethod::Post => reqwest::Method::POST,
        HttpMethod::Put => reqwest::Method::PUT,
        HttpMethod::Patch => reqwest::Method::PATCH,
        HttpMethod::Delete => reqwest::Method::DELETE,
        HttpMethod::Head => reqwest::Method::HEAD,
        HttpMethod::Options => reqwest::Method::OPTIONS,
    };

    let mut builder = client.request(method, &cmd.url);

    for (key, value) in &cmd.headers {
        builder = builder.header(key, value);
    }

    if !cmd.body.is_empty() {
        builder = builder.body(cmd.body);
    }

    let start = Instant::now();

    match builder.send().await {
        Ok(response) => {
            let elapsed = start.elapsed().as_millis();
            let status_code = response.status().as_u16();
            let status_text = response
                .status()
                .canonical_reason()
                .unwrap_or("")
                .to_string();

            let headers: Vec<(String, String)> = response
                .headers()
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                .collect();

            match response.bytes().await {
                Ok(bytes) => {
                    let size_bytes = bytes.len();
                    let body = String::from_utf8_lossy(&bytes).to_string();

                    Ok(HttpResponseResult {
                        status_code,
                        status_text,
                        elapsed_ms: elapsed,
                        size_bytes,
                        headers,
                        body,
                    })
                }
                Err(e) => Err(HttpRequestError {
                    message: format!("Failed to read response body: {e}"),
                }),
            }
        }
        Err(e) => Err(HttpRequestError {
            message: format!("{e}"),
        }),
    }
}
