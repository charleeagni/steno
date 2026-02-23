# AGENTS.md

## Prime Directive
Highest priority: readability. Do not extend functionality, integrate new interfaces, or update unrelated modules.

## Plane Workflow (Optional)
Applies ONLY when the user provides a Plane work-item ID or asks to work on a Plane ticket.

- Default project: `f36d36f0-f2f3-4c0b-a47f-8cd2eac8f027` (Coding).
- States: Backlog (needs refinement), Todo (requirements clear), HLD (optional high-level design), LLD (implementation harness per lld_generation.md), Split (break work per split.md), In Progress (execute sub-tasks), Done (all implemented and validated).
- Work-item shortcut: If an ID is provided, fetch details, list sub-tasks, check statuses, then proceed to the HLD/LLD check step; coordinate with the user.
- Procedure (only if following Plane workflow):
  1. Explore local repo first (`ls`, `rg`, open AGENTS.md) to capture constraints.
  2. List Coding modules via Plane; show `ID — Name`. If the user mentioned a partially matching module, select it; otherwise prompt with the list.
  3. Fetch Backlog and Todo tasks for the module (with descriptions). Show grouped lists and ask which task to focus on.
  4. Inspect task description for HLD/LLD; summarize if present. If missing, ask for requirements, propose HLD/LLD options, and confirm.
  5. After HLD is agreed, move to HLD. After LLD is agreed, move to LLD. After user approval to implement, move to In Progress.
  6. Implement only agreed scope. Move to Done after user validates.
- Navigation: If module/task info seems stale, rerun Plane commands. Keep outputs concise and only reference in-session Plane data.
- HLD/LLD persistence: Save to `spec/<module-name>/<Work-Item-name>/LLD.md` or `spec/<module-name>/<Work-Item-name>/HLD.md`. If the folder doesn't exist, create it.

## Scope Control
- If the user asks for an interface, function, class, or file, generate code **only for that unit**.
- Do not touch consumers, pipelines, watchers, repositories, or API routes unless the user directly specifies them.
- Never infer “next steps” or perform proactive integration.
- If creating layers of abstractions or redirections, ask for confirmation first. Don't impliment intermediate abstractions or redirections unless explicitly asked in the plan file or prompt.

## Modification Rules
- Keep changes local to the specified path or file.
- No cross-module edits (e.g., changing API handlers when only a repository method was requested).
- No automated refactors.
- No updating pipeline logic, watchers, lifecycle hooks, or database models unless explicitly directed.

## Default Framework Behavior
- Start with framework defaults (Django models, DRF generics, serializer save flows, etc.); only override hooks/methods when the user or spec explicitly says so.
- Do not introduce helper utilities or custom services to replace built-in behavior unless the request demands extra logic; confirm before deviating.
- If a requirement implies custom handling (e.g., overriding `post()`, `perform_create()`, or model timestamps), pause and validate that non-default behavior is truly needed.
- Prefer built-in ordering/pagination/timestamp fields unless the user states otherwise; rely on `auto_now`, DRF pagination, serializer defaults, etc., by default.
- Document in the response whenever you diverge from defaults so it’s clear why.

## Clarification Requirement
If a request is ambiguous, try to infer the correct file or function based on context and standard conventions. If still unsure or risk is high, ask for clarification.

## Output Discipline
- Add clear comments that describe code concisely; keep to a maximum of 10 words per comment.
- For newly generated code, include brief inline comments (separate line + blank line) to match the example spacing style.
- Provide only the code asked for.
- Produce minimal diffs.
- Avoid adding abstractions, helpers, new dependencies, or reorganizing existing code.
- Do not include code blocks in Low Level Design documents.

## Documentation Standards
- **Docstrings**: Multi-line format with detailed description of purpose and behavior.
- **Parameters**: `:param name: description` for all parameters, explaining type and purpose.
- **Returns**: `:return: description` describing return value and type.
- **Exceptions**: `:raises ExceptionType: condition` for documented exceptions.
- **Inline comments**: On separate lines before logical blocks, max 10 words, with blank line after.
- **Comment focus**: Explain what, why, and flow; prioritize decision points and non-obvious logic.
- **Class docs**: Expand to explain role, responsibilities, and relationships; use bullet points for key characteristics.

## Service Management
Use `./start_services.sh` to manage the application's services and interact with the running state of the application. Always prefer the non-interactive direct mode listed below:

- **Check Status**: `./start_services.sh status`
- **Start All**: `./start_services.sh run-all`
- **Stop All**: `./start_services.sh kill-all`
- **Restart All**: `./start_services.sh restart-all`
- **Targeted Action**: `./start_services.sh [start|stop|restart] [service_name|all]`

*Available service names: `server`, `transcriber`, `frontend`, `temporal`, `mcp-server`.*

## Environment
- Always activate the `.venv` before running Python commands or tests.
