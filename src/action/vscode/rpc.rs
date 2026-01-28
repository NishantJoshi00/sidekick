//! JSON-RPC client for communicating with VSCode extension.
//!
//! Protocol: Newline-delimited JSON over Unix socket
//!
//! Request format:
//! ```json
//! { "id": 1, "method": "buffer_status", "params": { "file_path": "/path/to/file" } }
//! ```
//!
//! Response format:
//! ```json
//! { "id": 1, "result": { "is_current": true, "has_unsaved_changes": false } }
//! ```

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};

/// Global request ID counter
static REQUEST_ID: AtomicU64 = AtomicU64::new(1);

/// JSON-RPC request
#[derive(Debug, Serialize)]
pub struct RPCRequest<T: Serialize> {
    pub id: u64,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<T>,
}

/// JSON-RPC response
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // Protocol type - all fields must be present for deserialization
pub struct RPCResponse<T> {
    pub id: Option<u64>,
    pub result: Option<T>,
    pub error: Option<RPCError>,
}

/// JSON-RPC error
#[derive(Debug, Deserialize)]
pub struct RPCError {
    pub code: i32,
    pub message: String,
}

/// Buffer status params
#[derive(Debug, Serialize)]
pub struct BufferStatusParams {
    pub file_path: String,
}

/// Buffer status result
#[derive(Debug, Deserialize)]
pub struct BufferStatusResult {
    pub is_current: bool,
    pub has_unsaved_changes: bool,
}

/// Refresh buffer params
#[derive(Debug, Serialize)]
pub struct RefreshBufferParams {
    pub file_path: String,
}

/// Refresh buffer result
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // Protocol type
pub struct RefreshBufferResult {
    pub success: bool,
}

/// Send message params
#[derive(Debug, Serialize)]
pub struct SendMessageParams {
    pub message: String,
}

/// Send message result
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // Protocol type
pub struct SendMessageResult {
    pub success: bool,
}

/// Visual selection context (matches EditorContext)
#[derive(Debug, Deserialize)]
pub struct VisualSelectionResult {
    pub file_path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub content: String,
}

/// RPC client for a single VSCode instance
pub struct RPCClient {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
}

impl RPCClient {
    /// Create a new RPC client connected to the given socket path
    pub fn connect(socket_path: &std::path::Path) -> Result<Self> {
        let stream =
            UnixStream::connect(socket_path).context("Failed to connect to VSCode socket")?;

        // Set read timeout
        stream
            .set_read_timeout(Some(crate::constants::VSCODE_RPC_TIMEOUT))
            .context("Failed to set read timeout")?;

        let reader = BufReader::new(stream.try_clone()?);

        Ok(Self { stream, reader })
    }

    /// Send a request and wait for response
    fn send_request<P: Serialize, R: for<'de> Deserialize<'de>>(
        &mut self,
        method: &str,
        params: Option<P>,
    ) -> Result<R> {
        let id = REQUEST_ID.fetch_add(1, Ordering::SeqCst);

        let request = RPCRequest {
            id,
            method: method.to_string(),
            params,
        };

        // Serialize and send request
        let request_json = serde_json::to_string(&request)?;
        writeln!(self.stream, "{}", request_json)?;
        self.stream.flush()?;

        // Read response
        let mut response_line = String::new();
        self.reader.read_line(&mut response_line)?;

        // Parse response
        let response: RPCResponse<R> =
            serde_json::from_str(&response_line).context("Failed to parse RPC response")?;

        if let Some(error) = response.error {
            anyhow::bail!("RPC error {}: {}", error.code, error.message);
        }

        response
            .result
            .ok_or_else(|| anyhow::anyhow!("RPC response missing result"))
    }

    /// Get buffer status for a file
    pub fn buffer_status(&mut self, file_path: &str) -> Result<BufferStatusResult> {
        self.send_request(
            "buffer_status",
            Some(BufferStatusParams {
                file_path: file_path.to_string(),
            }),
        )
    }

    /// Refresh buffer from disk
    pub fn refresh_buffer(&mut self, file_path: &str) -> Result<RefreshBufferResult> {
        self.send_request(
            "refresh_buffer",
            Some(RefreshBufferParams {
                file_path: file_path.to_string(),
            }),
        )
    }

    /// Send a notification message
    pub fn send_message(&mut self, message: &str) -> Result<SendMessageResult> {
        self.send_request(
            "send_message",
            Some(SendMessageParams {
                message: message.to_string(),
            }),
        )
    }

    /// Get visual selection from the active editor
    pub fn get_visual_selection(&mut self) -> Result<Option<VisualSelectionResult>> {
        self.send_request::<(), Option<VisualSelectionResult>>("get_visual_selection", None)
    }
}
