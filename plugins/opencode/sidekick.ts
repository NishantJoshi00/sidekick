// sidekick opencode plugin
//
// Bridges opencode's plugin events to the `sidekick hook` binary so opencode
// respects the same Neovim integration that protects Claude Code users:
//   - tool.execute.before  → block edits to dirty buffers
//   - tool.execute.after   → refresh buffers after an edit lands
//   - chat.message         → inject the current Neovim visual selection
//
// Covers the edit/write tools and the apply_patch tool that opencode
// substitutes for them on GPT models (a single apply_patch call can touch
// several files at once).
//
// Install: drop this file into ~/.config/opencode/plugin/sidekick.ts
//          (per-project: <project>/.opencode/plugin/sidekick.ts)
//
// Requires `sidekick` on PATH. If it isn't, the plugin silently no-ops
// rather than blocking opencode.

import type { Plugin } from "@opencode-ai/plugin"
import { spawn } from "node:child_process"
import { isAbsolute, join } from "node:path"

type ToolName = "Edit" | "Write"

const TOOL_MAP: Record<string, ToolName> = {
  edit: "Edit",
  write: "Write",
}

// opencode does model-conditional tool substitution: for GPT models it swaps
// edit/write for a single `apply_patch` tool whose only arg is a multi-file
// V4A patch. Affected files live in these marker lines, project-root-relative.
const PATCH_FILE_MARKERS = [
  "*** Add File:",
  "*** Update File:",
  "*** Delete File:",
  "*** Move to:",
]

type ToolEnvelope = {
  session_id: string
  transcript_path: string
  cwd: string
  hook_event_name: "PreToolUse" | "PostToolUse"
  tool_name: ToolName
  tool_input: { file_path: string }
}

type PromptEnvelope = {
  session_id: string
  transcript_path: string
  cwd: string
  hook_event_name: "UserPromptSubmit"
  prompt: string
}

type HookEnvelope = ToolEnvelope | PromptEnvelope

type HookResponse = {
  hookSpecificOutput?: {
    permissionDecision?: "allow" | "deny" | "ask"
    permissionDecisionReason?: string
    additionalContext?: string
  }
}

// A hook must never hang opencode. If `sidekick hook` doesn't answer within
// this window (e.g. its RPC to Neovim stalls), we kill it and move on.
const HOOK_TIMEOUT_MS = 3000

function pickFilePath(args: unknown): string | null {
  if (!args || typeof args !== "object") return null
  const candidate = (args as Record<string, unknown>).filePath
  return typeof candidate === "string" && candidate.length > 0 ? candidate : null
}

// Scan an apply_patch `patchText` blob for every file it touches.
function pickPatchPaths(args: unknown): string[] {
  if (!args || typeof args !== "object") return []
  const patch = (args as Record<string, unknown>).patchText
  if (typeof patch !== "string") return []

  const paths: string[] = []
  for (const line of patch.split("\n")) {
    const trimmed = line.trimStart()
    for (const marker of PATCH_FILE_MARKERS) {
      if (trimmed.startsWith(marker)) {
        const candidate = trimmed.slice(marker.length).trim()
        if (candidate) paths.push(candidate)
        break
      }
    }
  }
  return paths
}

// Resolve a tool call into the absolute file paths it will touch. Returns
// null if this tool isn't one sidekick cares about. A single apply_patch
// call can touch several files; edit/write touch exactly one.
function resolveCall(
  tool: string,
  args: unknown,
  cwd: string,
): { toolName: ToolName; filePaths: string[] } | null {
  const abs = (p: string) => (isAbsolute(p) ? p : join(cwd, p))

  if (tool === "apply_patch") {
    return { toolName: "Edit", filePaths: pickPatchPaths(args).map(abs) }
  }

  const toolName = TOOL_MAP[tool]
  if (!toolName) return null

  const raw = pickFilePath(args)
  return { toolName, filePaths: raw ? [abs(raw)] : [] }
}

function callSidekick(envelope: HookEnvelope, cwd: string): Promise<HookResponse | null> {
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

    // Hard ceiling: if sidekick stalls, kill it so opencode never blocks.
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

export const SidekickPlugin: Plugin = async ({ directory }) => {
  const cwd = directory ?? process.cwd()

  // opencode's tool.execute.after doesn't carry tool args, so we stash the
  // resolved file paths under the call id when the call is allowed through,
  // then look them up afterward to drive the buffer refresh.
  const pendingByCallID = new Map<string, { toolName: ToolName; filePaths: string[] }>()

  return {
    "tool.execute.before": async (input, output) => {
      const call = resolveCall(input.tool, output.args, cwd)
      if (!call || call.filePaths.length === 0) return

      // Check every file the call touches; deny the whole call if any one
      // has unsaved changes in Neovim.
      for (const filePath of call.filePaths) {
        const response = await callSidekick(
          {
            session_id: input.sessionID,
            transcript_path: "",
            cwd,
            hook_event_name: "PreToolUse",
            tool_name: call.toolName,
            tool_input: { file_path: filePath },
          },
          cwd,
        )

        if (response?.hookSpecificOutput?.permissionDecision === "deny") {
          throw new Error(
            response.hookSpecificOutput.permissionDecisionReason ??
              "sidekick: file has unsaved changes in Neovim",
          )
        }
      }

      pendingByCallID.set(input.callID, call)
    },

    "tool.execute.after": async (input) => {
      const pending = pendingByCallID.get(input.callID)
      if (!pending) return
      pendingByCallID.delete(input.callID)

      // Refresh every Neovim buffer the call modified.
      for (const filePath of pending.filePaths) {
        await callSidekick(
          {
            session_id: input.sessionID,
            transcript_path: "",
            cwd,
            hook_event_name: "PostToolUse",
            tool_name: pending.toolName,
            tool_input: { file_path: filePath },
          },
          cwd,
        )
      }
    },

    "chat.message": async (input, output) => {
      const response = await callSidekick(
        {
          session_id: input.sessionID,
          transcript_path: "",
          cwd,
          hook_event_name: "UserPromptSubmit",
          prompt: "",
        },
        cwd,
      )

      const context = response?.hookSpecificOutput?.additionalContext
      if (!context) return

      // opencode validates message parts against a strict schema (id,
      // sessionID and messageID are all required). Rather than synthesise a
      // new part, append the selection to the user's existing text part.
      const textPart = output.parts.find((p: any) => p.type === "text") as any
      if (textPart && typeof textPart.text === "string") {
        textPart.text = `${textPart.text}\n\n${context}`
      }
    },
  }
}

export default SidekickPlugin
