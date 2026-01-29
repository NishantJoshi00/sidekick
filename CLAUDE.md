# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Sidekick is a Rust-based CLI tool that provides two main functionalities:
1. **Claude Code Hook Handler**: Intercepts Claude Code tool calls to prevent file modifications when files are being edited in Neovim or VSCode with unsaved changes
2. **Editor Integration**: Launches Neovim instances with deterministic Unix socket paths based on the current working directory and process ID (using blake3 hashing). VSCode integration is provided via a TypeScript extension.

## Architecture

### Core Components

- **`main.rs`**: CLI entry point with three subcommands:
  - `hook`: Handles Claude Code PreToolUse hooks via stdin/stdout JSON protocol
  - `neovim`: Launches Neovim with computed socket path based on CWD hash and PID
  - `info`: Shows socket information and discovered editor instances

- **`handler.rs`**: Hook processing logic that:
  - Parses PreToolUse hook JSON from stdin
  - Discovers and connects to all editor instances (Neovim and VSCode) for the current directory via Unix sockets
  - Checks buffer status across all instances for file modification tools (Edit, Write, MultiEdit)
  - Returns permission decision (Allow/Deny) based on whether ANY instance has the file with unsaved changes in the current buffer
  - Automatically refreshes buffers in ALL instances after Claude Code modifies files

- **`hook.rs`**: Data structures for Claude Code hooks:
  - `PreToolUseHook`: Incoming hook payload with discriminated union for tool types
  - `Tool` enum: Read, Write, Edit, MultiEdit, Bash
  - `HookOutput`: Response structure with permission decisions for Claude Code
  - `PermissionDecision`: Allow, Deny, Ask

- **`action.rs`**: `Action` trait defining editor operations:
  - `buffer_status()`: Check if buffer is current and has unsaved changes
  - `refresh_buffer()`: Reload buffer from disk
  - `send_message()`: Display message in editor
  - `get_visual_selections()`: Get visual selections for context injection

- **`action/neovim.rs`**: Neovim RPC implementation:
  - Supports connecting to multiple Neovim instances via Unix sockets
  - Uses Lua code execution for buffer operations to preserve cursor positions
  - Implements buffer finding by canonicalized file paths
  - Aggregates status checks across all instances (denies if ANY has unsaved changes)
  - Refreshes buffers in all instances that have the file open

- **`action/vscode.rs`**: VSCode RPC implementation:
  - Supports connecting to multiple VSCode instances via Unix sockets
  - Uses JSON-RPC protocol (newline-delimited JSON)
  - Implements buffer status checks via VSCode extension API
  - Refreshes buffers using VSCode's revert command
  - Gets visual selections from active editor

- **`utils.rs`**: Utility functions for socket path management:
  - `compute_neovim_socket_path()`: Generates Neovim socket path with PID suffix
  - `compute_vscode_socket_path()`: Generates VSCode socket path with PID suffix
  - `find_neovim_sockets()`: Discovers all Neovim socket paths for the current directory
  - `find_vscode_sockets()`: Discovers all VSCode socket paths for the current directory

### VSCode Extension (`plugins/vscode-sidekick/`)

A TypeScript VSCode extension that:
- Creates a Unix socket server at `/tmp/<blake3(cwd)>-vscode-<pid>.sock`
- Listens for JSON-RPC requests from the Sidekick CLI
- Implements handlers for:
  - `buffer_status`: Check if file has unsaved changes via `TextDocument.isDirty`
  - `refresh_buffer`: Reload file via `workbench.action.files.revert` command
  - `send_message`: Show notification via `vscode.window.showWarningMessage`
  - `get_visual_selection`: Get current selection via `vscode.window.activeTextEditor.selection`

### Key Design Patterns

1. **Multi-Instance Socket Pattern**:
   - Neovim: `/tmp/<blake3(cwd)>-<pid>.sock`
   - VSCode: `/tmp/<blake3(cwd)>-vscode-<pid>.sock`
   - Hook command discovers all matching sockets using glob patterns
   - PID-based naming enables multiple instances per directory
   - Stale sockets are easily identified (process no longer running)

2. **Buffer Protection**: Denies modifications when:
   - File has unsaved changes in ANY editor instance (Neovim or VSCode)
   - File is in the current buffer (visible to user) in ANY instance
   - Rationale: Prevents losing user work across all instances while allowing background file updates

3. **Multi-Instance Actions**:
   - `buffer_status()`: Checks ALL instances, returns true if ANY has unsaved changes
   - `refresh_buffer()`: Refreshes file in ALL instances that have it open
   - `send_message()`: Sends message to ALL instances

4. **Graceful Degradation**: If no editor sockets exist, hooks allow all operations (no-op)

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
cargo run -- neovim <file>  # Opens Neovim with socket at /tmp/<blake3_hash>-<pid>.sock
```

### Show Socket Information
```bash
cargo run -- info           # Shows expected and discovered sockets for current directory
```

### Multiple Instances
```bash
# Open multiple Neovim instances in the same directory
cargo run -- neovim file1.txt  # Creates /tmp/<hash>-<pid1>.sock
cargo run -- neovim file2.txt  # Creates /tmp/<hash>-<pid2>.sock

# Hook handler automatically discovers and checks both instances
```

### VSCode Extension Development
```bash
cd plugins/vscode-sidekick
npm install              # Install dependencies
npm run compile          # Compile TypeScript
npm run package          # Create .vsix package
```

## Important Notes

- The project uses Rust edition 2024
- Neovim socket paths: `/tmp/<blake3(canonicalized_cwd)>-<pid>.sock`
- VSCode socket paths: `/tmp/<blake3(canonicalized_cwd)>-vscode-<pid>.sock`
- Multiple editor instances per directory are supported (both Neovim and VSCode)
- Hook handler discovers all instances using glob pattern matching
- Hook handler reads JSON from stdin and writes JSON to stdout (Claude Code protocol)
- RPC connection timeout is 2 seconds per instance
- Buffer refresh uses Lua (Neovim) or revert command (VSCode) to preserve cursor positions
- VSCode extension requires `blake3` npm package for hash computation
