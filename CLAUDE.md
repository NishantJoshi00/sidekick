# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Sidekick is a Rust-based CLI tool that provides two main functionalities:
1. **Claude Code Hook Handler**: Intercepts Claude Code tool calls to prevent file modifications when files are being edited in Neovim with unsaved changes
2. **Neovim Integration**: Launches Neovim instances with deterministic Unix socket paths based on the current working directory (using blake3 hashing)

## Architecture

### Core Components

- **`main.rs`**: CLI entry point with two subcommands:
  - `hook`: Handles Claude Code PreToolUse hooks via stdin/stdout JSON protocol
  - `neovim`: Launches Neovim with computed socket path based on CWD hash

- **`handler.rs`**: Hook processing logic that:
  - Parses PreToolUse hook JSON from stdin
  - Connects to Neovim instance via Unix socket (if exists)
  - Checks buffer status for file modification tools (Edit, Write, MultiEdit)
  - Returns permission decision (Allow/Deny) based on whether the file has unsaved changes in the current buffer
  - Automatically refreshes Neovim buffers after Claude Code modifies files

- **`hook.rs`**: Data structures for Claude Code hooks:
  - `PreToolUseHook`: Incoming hook payload with discriminated union for tool types
  - `Tool` enum: Read, Write, Edit, MultiEdit, Bash
  - `HookOutput`: Response structure with permission decisions for Claude Code
  - `PermissionDecision`: Allow, Deny, Ask

- **`action.rs`**: `Action` trait defining editor operations:
  - `buffer_status()`: Check if buffer is current and has unsaved changes
  - `refresh_buffer()`: Reload buffer from disk
  - `send_message()`: Display message in editor

- **`action/neovim.rs`**: Neovim RPC implementation:
  - Connects via Unix socket using `neovim-lib`
  - Uses Lua code execution for buffer operations to preserve cursor positions
  - Implements buffer finding by canonicalized file paths

### Key Design Patterns

1. **Socket Path Computation**: Both `hook` and `neovim` commands compute the same socket path by hashing the canonicalized CWD with blake3, ensuring they connect to the same instance

2. **Buffer Protection**: Only denies modifications when:
   - File has unsaved changes in Neovim
   - File is in the current buffer (visible to user)
   - Rationale: Prevents losing user work while allowing background file updates

3. **Graceful Degradation**: If Neovim socket doesn't exist, hooks allow all operations (no-op)

## Common Commands

### Build and Development
```bash
cargo build              # Build the project
cargo check              # Fast type checking without code generation
cargo clippy             # Run linter
cargo fmt                # Format code
cargo run -- <subcommand> # Run with arguments
```

### Testing Hook Handler
```bash
# Simulate Claude Code PreToolUse hook
echo '{"session_id":"test","transcript_path":"test","cwd":".","hook_event_name":"PreToolUse","tool_name":"Edit","tool_input":{"file_path":"test.txt"}}' | cargo run -- hook
```

### Launch Neovim with Socket
```bash
cargo run -- neovim <file>  # Opens Neovim with socket at /tmp/<blake3_hash>.sock
```

## Important Notes

- The project uses Rust edition 2024
- Neovim socket paths are computed deterministically: `/tmp/<blake3(canonicalized_cwd)>.sock`
- Hook handler reads JSON from stdin and writes JSON to stdout (Claude Code protocol)
- RPC connection timeout is 2 seconds
- Buffer refresh uses Lua to preserve cursor positions across all windows displaying the buffer
