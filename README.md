# Sidekick

**Protects your unsaved Neovim work from Claude Code, opencode, and pi.**

[![crates.io](https://img.shields.io/crates/v/sidekick.svg)](https://crates.io/crates/sidekick) [![MIT License](https://img.shields.io/crates/l/sidekick.svg)](LICENSE)

[![asciicast](https://asciinema.org/a/746395.svg)](https://asciinema.org/a/746395?t=80)

*Skip to 1:20 to see a block. The same recording is bundled in the binary — run `sidekick demo` after install to play it back offline.*

---

## The problem

You're mid-edit in `main.rs`. Claude Code (or opencode, or pi), running in the next pane, decides to refactor the same file. Without sidekick, your unsaved work just got overwritten.

Sidekick sits between the two. **If you have unsaved changes in the buffer the AI is about to touch, the AI waits.** The moment you `:w`, it proceeds. No flags, no confirmation prompts, no policy file — the 99% of edits that don't conflict with you go through untouched.

It also works the other direction: when the AI modifies a file you have open, sidekick refreshes the buffer in every Neovim instance, cursor position preserved.

## What changes in your workflow

- A file you're editing is now off-limits to the AI until you save it. You'll see this in Neovim's status line: `Edit blocked — file has unsaved changes`.
- A file the AI edits while you have it open is auto-reloaded — no `:e!` dance.
- A visual selection in Neovim is auto-injected into your next Claude Code, opencode, or pi prompt as context. Select code, type the prompt, hit enter.

Everything else stays the same. You keep using `nvim` like normal.

## Install

The one-liner installs the binary, registers the Claude Code hooks, and adds the shell alias. Pipe through `less` first if you want to read it.

```bash
curl -sSL https://raw.githubusercontent.com/NishantJoshi00/sidekick/main/scripts/install.sh | bash
```

Then verify with `sidekick doctor`:

```
  ✓ sidekick v0.4.0 on PATH
  ✓ nvim v0.10.0 on PATH
  ✓ Claude Code hook registered
  ✓ nvim alias set (zsh)
```

<details>
<summary><b>Manual install (cargo + Claude Code plugin)</b></summary>

```bash
# 1. Install the binary
cargo install sidekick

# 2. Inside Claude Code, register the hooks
/plugin marketplace add NishantJoshi00/claude-plugins
/plugin install sidekick@nishant-plugins

# 3. Add the shell alias to ~/.zshrc or ~/.bashrc
alias nvim='sidekick neovim'
```

Don't have Rust? `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`.

</details>

<details>
<summary><b>Manual hook configuration (no plugin)</b></summary>

Add to `~/.claude/settings.json`:

```json
{
  "hooks": {
    "PreToolUse": [
      { "matcher": "MultiEdit|Edit|Write", "hooks": [{ "type": "command", "command": "sidekick hook" }] }
    ],
    "PostToolUse": [
      { "matcher": "MultiEdit|Edit|Write", "hooks": [{ "type": "command", "command": "sidekick hook" }] }
    ],
    "UserPromptSubmit": [
      { "matcher": "", "hooks": [{ "type": "command", "command": "sidekick hook" }] }
    ]
  }
}
```

The `UserPromptSubmit` entry is optional — it's the one that injects your Neovim visual selection into Claude's prompt context.

</details>

<details>
<summary><b>Use with opencode</b></summary>

opencode uses a plugin system instead of CLI hooks. After installing the `sidekick` binary, drop the plugin into your opencode config:

```bash
mkdir -p ~/.config/opencode/plugin
curl -sSL https://raw.githubusercontent.com/NishantJoshi00/sidekick/main/plugins/opencode/sidekick.ts \
  -o ~/.config/opencode/plugin/sidekick.ts
```

opencode loads it at startup. Run `sidekick doctor` to confirm. See [`plugins/opencode/`](plugins/opencode/) for details.

</details>

<details>
<summary><b>Use with pi</b></summary>

pi uses a TypeScript extension system instead of CLI hooks. After installing the `sidekick` binary, drop the extension into your pi config:

```bash
mkdir -p ~/.pi/agent/extensions
curl -sSL https://raw.githubusercontent.com/NishantJoshi00/sidekick/main/plugins/pi/sidekick.ts \
  -o ~/.pi/agent/extensions/sidekick.ts
```

pi loads it at startup. Run `sidekick doctor` to confirm. See [`plugins/pi/`](plugins/pi/) for details.

</details>

## Usage

Just use `nvim`. The shell alias routes through sidekick so the hook can find your editor.

```bash
nvim src/main.rs
```

You won't notice anything different until Claude Code, opencode, or pi tries to overwrite unsaved work, at which point sidekick blocks it and tells you why.

## Commands

| Command | What it does |
|---------|--------------|
| `sidekick neovim <args>` | Launches Neovim with a per-directory socket the hook can find. Aliased as `nvim`. |
| `sidekick hook` | Reads hook JSON on stdin and decides allow/deny, refreshes buffers, injects visual selections. You don't run this directly — Claude Code does (and the opencode/pi bridges pipe to it). |
| `sidekick doctor` | Checks your install: binary on PATH, nvim on PATH, hooks registered, alias active, sockets open in this directory, last hook decision. |
| `sidekick demo` | Plays the demo cast inline in a ratatui frame. Useful for showing a coworker. |
| `sidekick stats [--range week\|month\|year\|all]` | Local activity dashboard: launches, allows, denies, refreshes. ASCII bars. Nothing leaves your machine. |

## How it works

1. `sidekick neovim` launches `nvim --listen /tmp/<blake3(cwd)>-<pid>.sock`. The socket path is deterministic, so the hook can find every Neovim instance running in the same directory.
2. Claude Code calls `sidekick hook` before any `Edit | Write | MultiEdit` (the bridges in [`plugins/`](plugins/) do the same for opencode's `tool.execute.before` and pi's `tool_call` events). The hook globs `/tmp/<blake3(cwd)>-*.sock`, connects to every instance over msgpack-rpc, and asks: *is this file the current buffer, and is it dirty?* If any instance says yes, the edit is denied; otherwise allowed.
3. On `PostToolUse`, the hook tells every instance with the file open to reload — buffer refreshes, cursor and view preserved.
4. On `UserPromptSubmit`, if there's a visual selection (or recent visual marks) in the active buffer, the hook injects it as a fenced code block in the additional context Claude sees. The opencode and pi bridges do the same on their prompt events, appending the selection to your prompt.

No daemons, no shared config, no per-project setup. Two binaries talking through `/tmp`.

## Requirements

Neovim (RPC + Lua) and a Unix-like system. Rust 2024 to build from source.

## What's next

Sidekick is Phase 1 — Neovim plus Claude Code, opencode, and pi. The longer arc is a small protocol any editor can expose and any AI tool can query before writing: *is this file being edited by a human right now?* [PHILOSOPHY.md](PHILOSOPHY.md) has the roadmap (Helix, Zed, VS Code; Aider, Goose, Continue) and the extension points.

## Contributing

Issues and PRs welcome. If you want to add a new editor or AI tool, the `Action` trait in `src/action.rs` is the contract — [PHILOSOPHY.md](PHILOSOPHY.md) covers the architecture.
