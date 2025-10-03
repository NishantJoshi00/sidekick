pub mod neovim;

/// Buffer status information
#[derive(Debug, Clone)]
pub struct BufferStatus {
    pub is_current: bool,
    pub has_unsaved_changes: bool,
}

/// Trait for editor actions
pub trait Action {
    /// Get the status of a buffer
    fn buffer_status(&self, file_path: &str) -> anyhow::Result<BufferStatus>;

    /// Refresh the buffer (reload from disk)
    fn refresh_buffer(&self, file_path: &str) -> anyhow::Result<()>;

    /// Send a message to the editor
    fn send_message(&self, message: &str) -> anyhow::Result<()>;

    /// Delete/close a buffer
    fn delete_buffer(&self, file_path: &str) -> anyhow::Result<()>;
}
