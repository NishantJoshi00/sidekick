//! Lua code templates for Neovim buffer operations.

/// Lua code to refresh a buffer while preserving cursor positions across all windows
pub fn refresh_buffer_lua(buf_number: i64) -> String {
    format!(
        r#"
        local buf = {}
        local cursor_positions = {{}}
        local is_current_buf = vim.api.nvim_get_current_buf() == buf

        -- Save cursor positions for all windows displaying this buffer
        for _, win in ipairs(vim.api.nvim_list_wins()) do
            if vim.api.nvim_win_get_buf(win) == buf then
                cursor_positions[win] = vim.api.nvim_win_get_cursor(win)
            end
        end

        -- Refresh the buffer (checktime triggers file change detection)
        vim.api.nvim_buf_call(buf, function()
            vim.cmd('checktime')
            vim.cmd('edit')
        end)

        -- Restore cursor positions
        for win, pos in pairs(cursor_positions) do
            if vim.api.nvim_win_is_valid(win) then
                pcall(vim.api.nvim_win_set_cursor, win, pos)
            end
        end

        -- Force redraw only if this is the current buffer
        if is_current_buf then
            vim.cmd('redraw')
        end
        "#,
        buf_number
    )
}

/// Lua code to send a notification message to Neovim
pub fn send_notification_lua(message: &str) -> String {
    format!(
        r#"vim.notify("{}", vim.log.levels.WARN)"#,
        message.replace('"', r#"\""#)
    )
}
