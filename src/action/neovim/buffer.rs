//! Buffer operations for Neovim instances.

use super::lua;
use crate::action::{BufferStatus, EditorContext};
use anyhow::{Context, Result};
use neovim_lib::{Neovim, NeovimApi, neovim_api::Buffer};
use std::path::PathBuf;

/// Find buffer by file path
pub fn find_buffer(nvim: &mut Neovim, file_path: &str) -> Result<Buffer> {
    let buffers = nvim.list_bufs().context("Failed to list buffers")?;

    let target_path = PathBuf::from(file_path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(file_path));

    for buffer in buffers {
        let buf_name = buffer.get_name(nvim).context("Failed to get buffer name")?;

        if buf_name.is_empty() {
            continue;
        }

        let buf_path = PathBuf::from(&buf_name)
            .canonicalize()
            .unwrap_or_else(|_| PathBuf::from(&buf_name));

        if buf_path == target_path {
            return Ok(buffer);
        }
    }

    anyhow::bail!("Buffer not found for file: {}", file_path)
}

/// Get buffer status (whether it's current and has unsaved changes)
pub fn get_buffer_status(nvim: &mut Neovim, file_path: &str) -> Result<BufferStatus> {
    let buffer = find_buffer(nvim, file_path)?;
    let current_buf = nvim.get_current_buf()?;
    let is_current = buffer == current_buf;

    let modified = buffer.get_option(nvim, "modified")?;
    let has_unsaved_changes = modified.as_bool().unwrap_or(false);

    Ok(BufferStatus {
        is_current,
        has_unsaved_changes,
    })
}

/// Refresh buffer from disk while preserving cursor positions
pub fn refresh_buffer(nvim: &mut Neovim, file_path: &str) -> Result<()> {
    let buffer = find_buffer(nvim, file_path)?;
    let buf_number = buffer.get_number(nvim)?;

    let lua_code = lua::refresh_buffer_lua(buf_number);

    nvim.execute_lua(&lua_code, vec![])
        .map(|_| ())
        .context("Failed to reload buffer")
}

/// Get visual selection from current buffer
pub fn get_visual_selection(nvim: &mut Neovim) -> Result<Option<EditorContext>> {
    let lua_code = lua::get_visual_selection_lua();

    let result = nvim
        .execute_lua(lua_code, vec![])
        .context("Failed to get visual selection")?;

    // Lua returns nil if no selection, or a JSON string
    if result.is_nil() {
        return Ok(None);
    }

    let json_str = result.as_str().context("Expected string from Lua")?;

    #[derive(serde::Deserialize)]
    struct SelectionData {
        file_path: String,
        start_line: u32,
        end_line: u32,
        content: String,
    }

    let data: SelectionData =
        serde_json::from_str(json_str).context("Failed to parse selection JSON")?;

    Ok(Some(EditorContext {
        file_path: data.file_path,
        start_line: data.start_line,
        end_line: data.end_line,
        content: data.content,
    }))
}
