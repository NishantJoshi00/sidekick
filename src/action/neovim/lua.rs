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

/// Lua code to get visual selection from the current buffer
pub fn get_visual_selection_lua() -> &'static str {
    r#"
    local start_pos = vim.fn.getpos("'<")
    local end_pos = vim.fn.getpos("'>")

    -- Check if visual marks are set (line numbers > 0)
    if start_pos[2] == 0 or end_pos[2] == 0 then
        return nil
    end

    local start_line = start_pos[2]
    local end_line = end_pos[2]

    -- Get current buffer file path
    local file_path = vim.api.nvim_buf_get_name(0)
    if file_path == "" then
        return nil
    end

    -- Get the lines (0-indexed API, so subtract 1 from start)
    local lines = vim.api.nvim_buf_get_lines(0, start_line - 1, end_line, false)
    local content = table.concat(lines, "\n")

    return vim.fn.json_encode({
        file_path = file_path,
        start_line = start_line,
        end_line = end_line,
        content = content
    })
    "#
}
