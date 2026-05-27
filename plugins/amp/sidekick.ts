// sidekick Amp plugin
//
// Bridges the Amp coding agent's plugin events to the `sidekick hook`
// binary so Amp respects the same Neovim integration that protects
// Claude Code, opencode, and pi users:
//   - tool.call    → block edits to files with unsaved Neovim changes
//   - tool.result  → refresh buffers after an edit lands
//
// Amp emits no UserPromptSubmit-equivalent event, so visual-selection
// injection is not supported in this harness.
//
// Install: drop this file into ~/.config/amp/plugins/sidekick.ts
//          (per-project: <project>/.amp/plugins/sidekick.ts)
//
// Requires `sidekick` on PATH. If it isn't, the plugin silently no-ops
// rather than blocking Amp.

import type { PluginAPI } from "@ampcode/plugin"
import { spawn } from "node:child_process"
import { isAbsolute, join } from "node:path"

type ToolName = "Edit" | "Write"

// Amp's built-in file-edit tools per `amp tools list`. Both map to
// Claude-shape tool names — sidekick treats Edit and MultiEdit
// identically, so create_file collapses to Write.
const TOOL_MAP: Record<string, ToolName> = {
  edit_file: "Edit",
  create_file: "Write",
}

type ToolEnvelope = {
  session_id: string
  transcript_path: string
  cwd: string
  hook_event_name: "PreToolUse" | "PostToolUse"
  tool_name: ToolName
  tool_input: { file_path: string }
}

type HookResponse = {
  hookSpecificOutput?: {
    permissionDecision?: "allow" | "deny" | "ask"
    permissionDecisionReason?: string
    additionalContext?: string
  }
}

// A hook must never hang Amp. If `sidekick hook` doesn't answer within
// this window (e.g. its RPC to Neovim stalls), we kill it and move on.
const HOOK_TIMEOUT_MS = 3000

// Amp's edit_file/create_file input schemas aren't publicly documented,
// so probe the common keys.
function pickFilePath(input: unknown): string | null {
  if (!input || typeof input !== "object") return null
  const obj = input as Record<string, unknown>
  for (const key of ["file_path", "filePath", "path"]) {
    const candidate = obj[key]
    if (typeof candidate === "string" && candidate.length > 0) {
      return candidate
    }
  }
  return null
}

function callSidekick(envelope: ToolEnvelope, cwd: string): Promise<HookResponse | null> {
  return new Promise((resolve) => {
    let proc
    try {
      proc = spawn("sidekick", ["hook"], { stdio: ["pipe", "pipe", "ignore"], cwd })
    } catch {
      resolve(null)
      return
    }

    let settled = false
    const finish = (value: HookResponse | null) => {
      if (settled) return
      settled = true
      clearTimeout(timer)
      resolve(value)
    }

    const timer = setTimeout(() => {
      proc.kill("SIGKILL")
      finish(null)
    }, HOOK_TIMEOUT_MS)

    let stdout = ""
    proc.stdout.on("data", (chunk: Buffer) => {
      stdout += chunk.toString()
    })
    proc.on("error", () => finish(null))
    proc.on("close", () => {
      const body = stdout.trim()
      if (!body) {
        finish({})
        return
      }
      try {
        finish(JSON.parse(body) as HookResponse)
      } catch {
        finish(null)
      }
    })

    // A killed child can EPIPE on stdin; swallow it rather than crash.
    proc.stdin.on("error", () => {})
    proc.stdin.write(JSON.stringify(envelope))
    proc.stdin.end()
  })
}

// Amp's plugin runtime sandboxes process.cwd() to the plugin install
// dir (and ctx.$ runs in that same sandbox), so the bridge has no way
// to retrieve the user's actual cwd from Amp's API. Sidekick handles
// this on its own side: when its cwd-based socket lookup misses, it
// walks up from the file path in the envelope until it finds a
// matching Neovim socket. The bridge just spawns sidekick and lets it
// locate the editor.
export default function (amp: PluginAPI) {
  amp.on("tool.call", async (event, _ctx) => {
    const toolName = TOOL_MAP[event.tool]
    if (!toolName) return { action: "allow" }

    const raw = pickFilePath(event.input)
    if (!raw) return { action: "allow" }
    const cwd = process.cwd()
    const filePath = isAbsolute(raw) ? raw : join(cwd, raw)

    const response = await callSidekick(
      {
        session_id: event.thread?.id ?? "",
        transcript_path: "",
        cwd,
        hook_event_name: "PreToolUse",
        tool_name: toolName,
        tool_input: { file_path: filePath },
      },
      cwd,
    )

    if (response?.hookSpecificOutput?.permissionDecision === "deny") {
      return {
        action: "reject-and-continue",
        message:
          response.hookSpecificOutput.permissionDecisionReason ??
          "sidekick: file has unsaved changes in Neovim",
      }
    }

    return { action: "allow" }
  })

  amp.on("tool.result", async (event, _ctx) => {
    if (event.status !== "done") return
    const toolName = TOOL_MAP[event.tool]
    if (!toolName) return

    const raw = pickFilePath(event.input)
    if (!raw) return
    const cwd = process.cwd()
    const filePath = isAbsolute(raw) ? raw : join(cwd, raw)

    await callSidekick(
      {
        session_id: event.thread?.id ?? "",
        transcript_path: "",
        cwd,
        hook_event_name: "PostToolUse",
        tool_name: toolName,
        tool_input: { file_path: filePath },
      },
      cwd,
    )
  })
}
