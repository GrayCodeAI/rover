# Coding-agent integration

Four profiles currently exist: generic-headless, generic-pty, codex-exec, claude-print.
Use `rover agent list` for actual capabilities. Native profiles build CLI commands and
parse bounded JSONL events. They are not full App Server/ACP implementations and do
not claim portable/native conversation resumption. No live provider was used here.

Generic `argv` substitutes a standalone `{{objective}}` argument without a shell.
Explicit shell commands are possible only because the local operator authorizes them.
Codex/Claude adapter `write` options choose native documented behavior; they do not
replace host isolation or circumvent a provider's restrictions. Unknown terminal
results and truncated/malformed transcripts cannot silently become successful runs.

Provider text remains an agent claim. An adapter that says completion was reported
does not certify the candidate. Auto-verification uses the original approved plan.

MCP makes Rover usable inside supported agent hosts. It does not force tool invocation.
The server exposes no approval, signing or grant-creation tool. `integrate` can add
reviewed instructions to AGENTS.md/CLAUDE.md/GEMINI.md and undo them safely; instructions
are workflow assistance, not a security boundary.

Authentication needs explicitly approved local environment/home configuration. No
API key is included or acquired automatically. Treat account/subscription support and
CLI version compatibility as separate from parser fixture coverage.
