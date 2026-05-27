# Codex — manual setup

Codex uses plugin-bundled hooks. Wiring it up by hand means three edits in two files.

## 1. Install the plugin into your personal Codex marketplace

Copy [`plugins/codex/`](../../plugins/codex/) into `~/plugins/sidekick/`:

```bash
mkdir -p ~/plugins/sidekick
cp -R plugins/codex/* ~/plugins/sidekick/
```

The directory must contain at least `hooks.json` from this repo. That file registers `sidekick hook` for `PreToolUse`, `PostToolUse`, and `UserPromptSubmit` with a matcher that covers Codex's edit tools (`Edit|Write|MultiEdit|apply_patch|edit|write|multi_edit`).

## 2. Register the plugin in `~/.agents/plugins/marketplace.json`

Add an entry pointing at the directory you just created. If the file does not exist, create it:

```json
{
  "marketplaces": {
    "personal": {
      "plugins": {
        "sidekick": {
          "path": "~/plugins/sidekick"
        }
      }
    }
  }
}
```

## 3. Enable plugin hooks in `~/.codex/config.toml`

```toml
[features]
plugin_hooks = true

[plugins."sidekick@personal"]
enabled = true
```

## 4. Trust the hook

Open Codex; it may prompt you to review and trust the new hook before it runs. Approve once.

## Verify

```bash
sidekick doctor
```

Look for the `Codex plugin installed` row.

## Why this is annoying by hand

Three writes across two files, plus a Codex-side trust prompt. `sidekick init` does all three and shows you the diff before each. Use it unless you have a specific reason not to.
