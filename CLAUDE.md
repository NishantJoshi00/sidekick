# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Sidekick is a Rust-based CLI tool that provides two main functionalities:
1. **Claude Code Hook Handler**: Intercepts Claude Code tool calls to prevent file modifications when files are being edited in Neovim with unsaved changes across multiple Neovim instances
2. **Neovim Integration**: Launches Neovim instances with deterministic Unix socket paths based on the current working directory and process ID (using blake3 hashing)

## Installation

### As a Claude Code Plugin

1. **Install the sidekick binary globally**:
   ```bash
   cargo install --path .
   ```

2. **Install the plugin locally** (from the plugin directory):
   ```bash
   # Add this repository as a local plugin
   /plugin add local /path/to/sidekick

   # Install the plugin
   /plugin install sidekick
   ```

3. **Launch Neovim with sidekick**:
   ```bash
   sidekick neovim <file>
   ```

The plugin will automatically hook into Claude Code's Edit, Write, and MultiEdit operations, preventing file modifications when you have unsaved changes in Neovim.

### Manual Installation (without plugin)

If you prefer to configure hooks manually, install the binary and configure the hooks in your Claude Code settings to call `sidekick hook` for PreToolUse and PostToolUse events.

## Architecture

### Core Components

- **`main.rs`**: CLI entry point with two subcommands:
  - `hook`: Handles Claude Code PreToolUse hooks via stdin/stdout JSON protocol
  - `neovim`: Launches Neovim with computed socket path based on CWD hash and PID

- **`handler.rs`**: Hook processing logic that:
  - Parses PreToolUse hook JSON from stdin
  - Discovers and connects to all Neovim instances for the current directory via Unix sockets
  - Checks buffer status across all instances for file modification tools (Edit, Write, MultiEdit)
  - Returns permission decision (Allow/Deny) based on whether ANY instance has the file with unsaved changes in the current buffer
  - Automatically refreshes Neovim buffers in ALL instances after Claude Code modifies files

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
  - Supports connecting to multiple Neovim instances via Unix sockets
  - Uses Lua code execution for buffer operations to preserve cursor positions
  - Implements buffer finding by canonicalized file paths
  - Aggregates status checks across all instances (denies if ANY has unsaved changes)
  - Refreshes buffers in all instances that have the file open

- **`utils.rs`**: Utility functions for socket path management:
  - `compute_socket_path_with_pid()`: Generates socket path with PID suffix
  - `find_matching_sockets()`: Discovers all socket paths for the current directory

### Key Design Patterns

1. **Multi-Instance Socket Pattern**:
   - `neovim` command creates socket with pattern: `/tmp/<blake3(cwd)>-<pid>.sock`
   - `hook` command discovers all matching sockets using glob: `/tmp/<blake3(cwd)>-*.sock`
   - PID-based naming enables multiple instances per directory
   - Stale sockets are easily identified (process no longer running)

2. **Buffer Protection**: Denies modifications when:
   - File has unsaved changes in ANY Neovim instance
   - File is in the current buffer (visible to user) in ANY instance
   - Rationale: Prevents losing user work across all instances while allowing background file updates

3. **Multi-Instance Actions**:
   - `buffer_status()`: Checks ALL instances, returns true if ANY has unsaved changes
   - `refresh_buffer()`: Refreshes file in ALL instances that have it open
   - `send_message()`: Sends message to ALL instances

4. **Graceful Degradation**: If no Neovim sockets exist, hooks allow all operations (no-op)

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

### Multiple Instances
```bash
# Open multiple Neovim instances in the same directory
cargo run -- neovim file1.txt  # Creates /tmp/<hash>-<pid1>.sock
cargo run -- neovim file2.txt  # Creates /tmp/<hash>-<pid2>.sock

# Hook handler automatically discovers and checks both instances
```

## Important Notes

- The project uses Rust edition 2024
- Neovim socket paths include PID: `/tmp/<blake3(canonicalized_cwd)>-<pid>.sock`
- Multiple Neovim instances per directory are supported
- Hook handler discovers all instances using glob pattern matching
- Hook handler reads JSON from stdin and writes JSON to stdout (Claude Code protocol)
- RPC connection timeout is 2 seconds per instance
- Buffer refresh uses Lua to preserve cursor positions across all windows displaying the buffer
