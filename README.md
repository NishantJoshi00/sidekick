# Sidekick: Your AI Assistant Should Be a Sidekick, Not the Pilot

[![GitHub stars](https://img.shields.io/github/stars/NishantJoshi00/sidekick?style=social)](https://github.com/NishantJoshi00/sidekick)

Ever had Claude Code overwrite your unsaved work mid-refactor?

Sidekick keeps you in control while AI assists. No plugins. No deep integrations. Just clean boundaries between you and your AI tools.

[![asciicast](https://asciinema.org/a/746395.svg)](https://asciinema.org/a/746395?t=80)

*Skip to 1:20 to see the real power*

---

## The Problem

You're 30 minutes into a complex refactor. Multiple files open. Unsaved changes everywhere.

You ask Claude Code to "add error handling to utils.py"

Claude Code obliterates your unsaved work.

Your changes are gone. **Sound familiar?**

Sidekick prevents this. Every. Single. Time.

## What It Does

1. **Neovim Protection**: Prevents Claude Code from modifying files you're actively editing in Neovim with unsaved changes
2. **Smart Neovim Launcher**: Opens Neovim instances with per-directory socket paths for seamless integration

Sidekick acts as a safety layer, blocking Claude Code's file modifications when they would overwrite your unsaved work.

## Installation

### One-Line Install (Automatic Setup)

```bash
curl -sSL https://raw.githubusercontent.com/NishantJoshi00/sidekick/main/scripts/install.sh | bash
```

This script will:
- ✓ Check dependencies (Rust, Python)
- ✓ Install Sidekick
- ✓ Configure Claude Code hooks
- ✓ Add shell alias automatically

---

<details>
<summary><b>Manual Installation</b></summary>

### Quick Install

```bash
cargo install sidekick
```

**Don't have Rust?** Install it first:
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

**Alternative:** Install from source
```bash
git clone https://github.com/NishantJoshi00/sidekick
cd sidekick
cargo install --path .
```

</details>

---

## Manual Setup

> **Note:** If you used the one-line installer above, you can skip this section!

### 1. Shell Alias (Recommended)

Add this to your shell configuration (`~/.bashrc`, `~/.zshrc`, etc.):

```bash
alias nvim='sidekick neovim'
```

Now every time you run `nvim`, you'll automatically get the Claude Code integration without thinking about it.

### 2. Configure Claude Code Hooks

Add to your Claude Code configuration (`~/.claude/settings.json` or `.claude/settings.json`):

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "MultiEdit|Edit|Write",
        "hooks": [
          {
            "type": "command",
            "command": "sidekick hook"
          }
        ]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "MultiEdit|Edit|Write",
        "hooks": [
          {
            "type": "command",
            "command": "sidekick hook"
          }
        ]
      }
    ]
  }
}
```

That's it! Now when Claude Code attempts to edit a file:
- **Allowed**: File not open in Neovim, or open in background buffer
- **Allowed**: File open but no unsaved changes
- **Blocked**: File is in current buffer with unsaved changes
- **Auto-refresh**: Neovim buffers are automatically reloaded after Claude Code modifies files

## Usage

Just use Neovim like you always do:

```bash
nvim <file>
```

That's it. Seriously.

Because of the shell alias, `nvim` now launches with Sidekick integration automatically. You won't notice anything different—until Claude Code tries to overwrite your unsaved work, and Sidekick quietly blocks it. Work the way you want. Sidekick stays out of your way.

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

**Neovim Community**

For creating the most extensible, keyboard-driven editor that makes coding feel like a superpower. Your commitment to backwards compatibility, clean architecture, and RPC-first design made this integration possible.

**Claude Code Community**

For pushing the boundaries of AI-assisted development with a tool that actually understands context and respects developer workflows. The hook system is a brilliant design choice that enables tools like this to exist.

This project exists because both communities care deeply about craft, extensibility, and putting developers first.

## Contributing

Issues and pull requests welcome! If you're using this with other editors (Emacs, VS Code with Neovim plugin, Helix), contributions to support them would be amazing.
