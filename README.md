# Sidekick

**Protects your unsaved Neovim work from Claude Code, opencode, and pi.**

[![crates.io](https://img.shields.io/crates/v/sidekick.svg)](https://crates.io/crates/sidekick) [![MIT License](https://img.shields.io/crates/l/sidekick.svg)](LICENSE)

[![asciicast](https://asciinema.org/a/cpLOxHxSC0BKVK6K.svg)](https://asciinema.org/a/cpLOxHxSC0BKVK6K?t=80)

*Skip to 1:20 to see a block. The same recording is bundled in the binary — run `sidekick demo` after install to play it back offline.*

---

## The problem

You're mid-edit in `main.rs`. Claude Code (or opencode, or pi), running in the next pane, decides to refactor the same file. Without sidekick, your unsaved work can get overwritten.

Sidekick sits between the two. **If you have unsaved changes in the buffer the AI is about to touch, the edit is blocked.** Save the file and the next attempt proceeds. No flags, no confirmation prompts, no policy file — the 99% of edits that don't conflict with you go through untouched.

It also works the other direction: when the AI modifies a file you have open, sidekick refreshes the buffer in every Neovim instance, cursor position preserved.

## What changes in your workflow

- A file you're editing is now off-limits to the AI until you save it. You'll see this in Neovim: `Edit blocked — file has unsaved changes`.
- A file the AI edits while you have it open is auto-reloaded — no `:e!` dance.
- A current or recent visual selection in Neovim can be added to your next Claude Code, opencode, or pi prompt as context. Select code, type the prompt, hit enter.

Everything else stays the same. You keep using `nvim` like normal.

## Install

The one-liner installs the binary with Cargo, registers Claude Code edit/refresh hooks, and adds the shell alias. Pipe through `less` first if you want to read it.

```bash
curl -sSL https://raw.githubusercontent.com/NishantJoshi00/sidekick/main/scripts/install.sh | bash
```

Then restart your shell, or source the rc file the installer updated, and verify with `sidekick doctor`:

```
  sidekick doctor

  ✓ sidekick v0.6.0 on PATH
      ~/.cargo/bin/sidekick
  ✓ NVIM v0.10.0 on PATH
  · AI harnesses: Claude Code
  ✓ Claude Code hook registered
  ✓ nvim alias: nvim → sidekick neovim (zsh)
  · no Neovim opened here
  · last activity: never
```

If a row fails, `sidekick doctor --fix` offers consent-gated repairs for the Claude Code hook, opencode plugin, and `nvim` alias. It shows the diff before writing anything.

The installer covers protection and buffer refresh for Claude Code. For Claude Code prompt-context injection, install the Claude Code plugin or add the `UserPromptSubmit` hook shown below.

<details>
<summary><b>Manual install (cargo + Claude Code plugin)</b></summary>

```bash
# 1. Install the binary
cargo install sidekick

# 2. Inside Claude Code, register the hooks bundled in this repo
/plugin marketplace add NishantJoshi00/claude-plugins
/plugin install sidekick@nishant-plugins

# 3. Add the shell alias to ~/.zshrc or ~/.bashrc
alias nvim='sidekick neovim'
```

Don't have Rust? `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`.

</details>

<details>
<summary><b>Manual Claude Code hook configuration (no plugin)</b></summary>

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

The `UserPromptSubmit` entry is optional. It is the one that adds your Neovim visual selection to Claude's prompt context.

</details>

<details>
<summary><b>Use with opencode</b></summary>

opencode uses a plugin system instead of CLI hooks. After installing the `sidekick` binary, drop the plugin into your global opencode config:

```bash
mkdir -p ~/.config/opencode/plugin
curl -sSL https://raw.githubusercontent.com/NishantJoshi00/sidekick/main/plugins/opencode/sidekick.ts \
  -o ~/.config/opencode/plugin/sidekick.ts
```

opencode loads it at startup. If `sidekick` is not on `PATH`, the plugin no-ops rather than blocking opencode.

For a per-project install, use `<project>/.opencode/plugin/sidekick.ts`. Run `sidekick doctor` to confirm. See [`plugins/opencode/`](plugins/opencode/) for details.

</details>

<details>
<summary><b>Use with pi</b></summary>

pi uses a TypeScript extension system instead of CLI hooks. After installing the `sidekick` binary, drop the extension into your global pi config:

```bash
mkdir -p ~/.pi/agent/extensions
curl -sSL https://raw.githubusercontent.com/NishantJoshi00/sidekick/main/plugins/pi/sidekick.ts \
  -o ~/.pi/agent/extensions/sidekick.ts
```

pi loads it at startup. If `sidekick` is not on `PATH`, the extension no-ops rather than blocking pi.

For a per-project install, use `<project>/.pi/extensions/sidekick.ts`. Run `sidekick doctor` to confirm. See [`plugins/pi/`](plugins/pi/) for details.

</details>

## Usage

Just use `nvim`. The shell alias routes through sidekick so the hook can find your editor.

```bash
nvim src/main.rs
```

If you do not want the alias, run `sidekick neovim <args>` directly. You won't notice anything different until Claude Code, opencode, or pi tries to overwrite unsaved work, at which point sidekick blocks it and tells the agent why.

## Commands

| Command | What it does |
|---------|--------------|
| `sidekick neovim <args>` | Launches Neovim with a per-directory socket the hook can find. Aliased as `nvim`. |
| `sidekick hook` | Reads hook JSON on stdin and decides allow/deny, refreshes buffers, and returns visual-selection context. You don't run this directly — Claude Code does, and the opencode/pi bridges pipe to it. |
| `sidekick doctor [--fix] [--no-color]` | Checks your install: binary on PATH, nvim on PATH, AI harnesses present, hooks/plugins registered, alias active, sockets open in this directory, last hook decision. `--fix` offers consent-gated repairs where possible. |
| `sidekick demo` | Plays the demo cast inline in a ratatui frame. Useful for showing a coworker. |
| `sidekick stats [--range week\|month\|year\|all] [--no-color]` | Local activity dashboard built from append-only JSONL events: launches, allows, blocks, refreshes, and top files. Nothing leaves your machine. |

## How it works

1. `sidekick neovim` launches `nvim --listen /tmp/<blake3(cwd)>-<pid>.sock`. The socket path is deterministic per canonical working directory and unique per process, so the hook can find every Neovim instance opened from the same project.
2. Claude Code calls `sidekick hook` before any `Edit | Write | MultiEdit`. The opencode and pi bridges do the equivalent for their `edit` and `write` tools. The hook globs `/tmp/<blake3(cwd)>-*.sock`, connects to reachable instances over msgpack-rpc with a short timeout, and asks whether the target is active with unsaved changes. If yes, the edit is denied; otherwise it is allowed. If no Neovim socket is found, sidekick degrades to allow.
3. After an edit lands, the hook tells every reachable Neovim instance with the file open to reload it. Cursor positions and visible windows are preserved.
4. On prompt submission, if Neovim has a live visual selection or recent visual marks, sidekick returns fenced context blocks like `[Selected from path:start-end]`. Claude Code receives them as additional context; opencode and pi append them to the submitted prompt text.
5. Decisions, refreshes, Neovim launches, and stats views are appended locally to `sidekick/events.jsonl` under your OS data directory. Writes are best-effort and analytics never block the hook path.

No daemons, no background service. Just one CLI, Neovim RPC sockets in `/tmp`, and optional per-tool bridge files.

## Requirements

Neovim with RPC + Lua support, a Unix-like system, and at least one supported AI harness: Claude Code, opencode, or pi. Rust/Cargo is required for `cargo install` or building from source; the install script also needs `python3` or `python` to merge Claude Code settings.

## What's next

Sidekick is Phase 1 — Neovim plus Claude Code, opencode, and pi. The longer arc is a small protocol any editor can expose and any AI tool can query before writing: *is this file being edited by a human right now?* [PHILOSOPHY.md](PHILOSOPHY.md) has the roadmap (Helix, Zed, VS Code; Aider, Goose, Continue) and the extension points.

## Contributing

Issues and PRs welcome. If you want to add a new editor or AI tool, the `Action` trait in `src/action.rs` is the contract — [PHILOSOPHY.md](PHILOSOPHY.md) covers the architecture.
