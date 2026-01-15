# Sidekick

Protects your unsaved Neovim work from Claude Code.

[![GitHub stars](https://img.shields.io/github/stars/NishantJoshi00/sidekick?style=social)](https://github.com/NishantJoshi00/sidekick)

**How it works:**
- You're editing a file in Neovim with unsaved changes
- Claude Code tries to modify that file
- Sidekick blocks it — your work is safe
- Save when you're ready, Claude Code proceeds

[![asciicast](https://asciinema.org/a/746395.svg)](https://asciinema.org/a/746395?t=80)

*Demo: Skip to 1:20 to see Sidekick block an edit*

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

### Official Method

**1. Install Sidekick**
```bash
cargo install sidekick
```

**Don't have Rust?** Install it first:
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

**2. Configure Claude Code Integration**
```bash
# In Claude Code
/plugin marketplace add NishantJoshi00/claude-plugins
/plugin install sidekick@nishant-plugins
```

That's it! The plugin automatically configures the necessary hooks for Claude Code integration.

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

### 3. Visual Selection Context (Optional)

Want Claude Code to see what you've selected in Neovim? Add this hook:

```json
{
  "hooks": {
    "UserPromptSubmit": [
      {
        "matcher": "",
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

Now when you submit a prompt to Claude Code, any visual selection in Neovim is automatically injected as context:

1. Select code in Neovim (visual mode)
2. Exit visual mode (the selection marks are preserved)
3. Submit your prompt to Claude Code
4. Claude sees your selection as additional context

No selection? Nothing happens — the hook is a no-op.

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

## Contributing

Issues and pull requests welcome!

Interested in extending Sidekick to other editors or AI tools? See [PHILOSOPHY.md](PHILOSOPHY.md) for the roadmap and vision.
