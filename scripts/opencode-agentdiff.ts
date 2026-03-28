/**
 * agentdiff plugin for OpenCode
 *
 * Managed by `agentdiff configure`.
 */
import type { Plugin } from "@opencode-ai/plugin"
import { dirname } from "path"

const CAPTURE_SCRIPT = "__AGENTDIFF_CAPTURE_OPENCODE__"

// OpenCode tool names that write/modify files.
// OpenCode uses short lowercase names ("edit", "write") for its built-in tools.
const FILE_EDIT_TOOLS = new Set(["edit", "write", "patch", "multiedit", "replace", "create"])

type PendingEdit = {
  filePath: string
  repoDir: string
  sessionID: string
  tool: string
  args: Record<string, unknown>
}

/** Resolve the file path from tool args — OpenCode uses "file" or "path" or "filePath". */
function resolveFilePath(args: Record<string, unknown>): string | undefined {
  return (
    (args.file as string | undefined) ??
    (args.filePath as string | undefined) ??
    (args.file_path as string | undefined) ??
    (args.path as string | undefined) ??
    (args.filename as string | undefined)
  )
}

/** Spawn python3 with JSON payload on stdin. Uses Bun.spawnSync when available,
 *  falls back to the Bun $ shell. */
async function runCapture($: any, payload: object): Promise<void> {
  const json = JSON.stringify(payload)

  // Prefer Bun.spawnSync (avoids shell pipe quoting issues).
  if (typeof Bun !== "undefined" && Bun.spawnSync) {
    Bun.spawnSync(["python3", CAPTURE_SCRIPT], {
      stdin: new TextEncoder().encode(json),
      stdout: "ignore",
      stderr: "ignore",
    })
    return
  }

  // Fallback: write payload to a temp file and pass it via stdin redirect.
  const tmp = `/tmp/agentdiff-oc-${Date.now()}.json`
  try {
    await Bun.write(tmp, json)
    await $`python3 ${CAPTURE_SCRIPT} < ${tmp}`.quiet()
  } finally {
    try { await $`rm -f ${tmp}`.quiet() } catch { /* ignore */ }
  }
}

export const AgentDiffPlugin: Plugin = async (ctx) => {
  const { $ } = ctx
  const pendingEdits = new Map<string, PendingEdit>()

  const findGitRepo = async (filePath: string): Promise<string | null> => {
    try {
      const dir = dirname(filePath)
      const result = await $`git -C ${dir} rev-parse --show-toplevel`.quiet()
      const repoRoot = result.stdout.toString().trim()
      return repoRoot || null
    } catch {
      return null
    }
  }

  return {
    "tool.execute.before": async (input, output) => {
      const tool = String(input.tool || "").toLowerCase()
      if (!FILE_EDIT_TOOLS.has(tool)) {
        return
      }

      const args = (output.args ?? {}) as Record<string, unknown>
      const filePath = resolveFilePath(args)
      if (!filePath) {
        return
      }

      const repoDir = await findGitRepo(filePath)
      if (!repoDir) {
        return
      }

      pendingEdits.set(input.callID, {
        filePath,
        repoDir,
        sessionID: input.sessionID,
        tool,
        args,
      })
    },

    "tool.execute.after": async (input, _output) => {
      const editInfo = pendingEdits.get(input.callID)
      pendingEdits.delete(input.callID)
      if (!editInfo) {
        return
      }

      const { filePath, repoDir, sessionID, tool, args } = editInfo

      // Resolve old/new strings from various possible field names OpenCode may use.
      const oldString = String(
        args.old_string ?? args.oldString ?? args.old ?? args.search ?? "",
      )
      const newString = String(
        args.new_string ?? args.newString ?? args.new ?? args.replace ?? args.content ?? "",
      )

      const payload = {
        hook_event_name: "PostToolUse",
        session_id: sessionID,
        model: String((input as any).modelID ?? (input as any).model ?? "opencode"),
        cwd: repoDir,
        tool_name: tool,
        tool_input: {
          filePath,
          old_string: oldString,
          new_string: newString,
          content: String(args.content ?? ""),
        },
      }

      try {
        await runCapture($, payload)
      } catch (error) {
        console.error("[agentdiff] OpenCode capture failed:", String(error))
      }
    },
  }
}

export default AgentDiffPlugin
