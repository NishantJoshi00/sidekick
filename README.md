# Sidekick

A Rust CLI tool that bridges Claude Code and Neovim, preventing conflicts when AI-assisted coding meets human editing.

## What It Does

Sidekick provides two key features:

1. **Neovim Protection**: Prevents Claude Code from modifying files you're actively editing in Neovim with unsaved changes
2. **Smart Neovim Launcher**: Opens Neovim instances with per-directory socket paths for seamless integration

## Why?

When using Claude Code to edit files while working in Neovim, you risk losing unsaved work if Claude modifies a file you're currently editing. Sidekick acts as a safety layer, blocking Claude Code's file modifications when they would overwrite your unsaved changes.

## Installation

```bash
cargo build --release
```

The binary will be available at `target/release/sidekick`.

### Quick Setup (Recommended)

Add this to your shell configuration (`~/.bashrc`, `~/.zshrc`, etc.):

```bash
alias nvim='sidekick neovim'
```

Now every time you run `nvim`, you'll automatically get the Claude Code integration without thinking about it.

## Usage

### Launch Neovim with Integration

```bash
sidekick neovim <file>
```

This launches Neovim with a Unix socket at `/tmp/<hash>.sock`, where the hash is deterministically computed from your current working directory.

### Configure Claude Code Hook

Add to your Claude Code configuration:

```json
{
  "hooks": {
    "PreToolUse": {
      "command": "sidekick hook"
    }
  }
}
```

Now when Claude Code attempts to edit a file:
- ✅ **Allowed**: File not open in Neovim, or open in background buffer
- ✅ **Allowed**: File open but no unsaved changes
- ❌ **Blocked**: File is in current buffer with unsaved changes
- 🔄 **Auto-refresh**: Neovim buffers are automatically reloaded after Claude Code modifies files

## How It Works

1. **Socket Path**: Both the `neovim` launcher and `hook` handler compute the same socket path using `blake3(cwd)`, ensuring they connect to the same Neovim instance

2. **Hook Interception**: When Claude Code calls Edit/Write tools, Sidekick intercepts the call via stdin/stdout JSON protocol

3. **Buffer Check**: Connects to Neovim via RPC to check if the target file:
   - Is the current buffer (visible to user)
   - Has unsaved changes (`modified` flag)

4. **Decision**: Returns `Allow` or `Deny` permission to Claude Code, with a message displayed in Neovim when blocked

## Requirements

- Rust 2024 edition
- Neovim with RPC support
- Unix-like system (uses Unix sockets)

## Example Workflow

```bash
# Terminal 1: Launch Neovim with integration
cd ~/my-project
sidekick neovim src/main.rs

# Make some edits in Neovim, don't save yet

# Terminal 2: Use Claude Code in the same directory
cd ~/my-project
claude "refactor main.rs to use async/await"

# Result: Claude Code is blocked from modifying main.rs
# You see a message in Neovim: "Claude Code blocked: File src/main.rs has unsaved changes"
# Save your changes, then Claude Code can proceed
```

## Built with Love For

**Neovim Community** 💚
For creating the most extensible, keyboard-driven editor that makes coding feel like a superpower. Your commitment to backwards compatibility, clean architecture, and RPC-first design made this integration possible.

**Claude Code Community** 🤖
For pushing the boundaries of AI-assisted development with a tool that actually understands context and respects developer workflows. The hook system is a brilliant design choice that enables tools like this to exist.

This project exists because both communities care deeply about craft, extensibility, and putting developers first.

## Contributing

Issues and pull requests welcome! If you're using this with other editors (Emacs, VS Code with Neovim plugin, Helix), contributions to support them would be amazing.

## License

MIT
