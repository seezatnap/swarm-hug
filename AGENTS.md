This is the operational guide for working in an unfamiliar repository.
Goal: discover how to build, test, lint, and run validations, then use those gates every iteration.

## Related files
- **PROMPT.md**: Contains the goals and requirements for the current work. Read this first to understand what needs to be accomplished.
- **SPECS.md**: Detailed specifications derived from PROMPT.md. If PROMPT.md changes or contains requirements not yet captured in SPECS.md, update SPECS.md accordingly.
- **TASKS.md**: A checklist for tracking progress toward the goals in PROMPT.md. Update this as you complete tasks.

## Task management (CRITICAL)
**Always write TODOs to TASKS.md. Never manage tasks in internal state.**

- TASKS.md is the single source of truth for all task tracking.
- **Create entries in TASKS.md for ALL requirements in PROMPT.md.** Break down the prompt into discrete tasks and add them all upfront—but only complete one task per session.
- When planning work, write the task breakdown to TASKS.md immediately.
- When starting a task, update its status in TASKS.md.
- When completing a task, mark it done in TASKS.md.
- When discovering new tasks, add them to TASKS.md.
- When updating TASKS.md, remove completed tasks that are no longer relevant to PROMPT.md. Keep TASKS.md focused on current goals.
- Do not rely on memory or internal state to track progress—if it's not in TASKS.md, it doesn't exist.

Use a simple checkbox format:
```markdown
- [ ] Pending task
- [x] Completed task
```

**ONE TASK PER SESSION**: Complete only the first unchecked task in TASKS.md, then stop. Leave subsequent tasks for another run. Do not attempt multiple tasks in a single session.

## Golden rules
- Do not assume the stack. Discover it from files and existing automation.
- Prefer existing scripts and tools already used by the repository.
- Do not add new dependencies unless required by the current task.
- Do not commit secrets. Do not print tokens. Do not modify lockfiles unless necessary.

## First 5 minutes (orientation)
1) Read PROMPT.md to understand the goals for this session.
2) Review SPECS.md and compare against PROMPT.md. If any requirements in PROMPT.md are missing from SPECS.md, update SPECS.md before proceeding.
3) Review TASKS.md to see current progress and remaining work.
4) Inspect the root:
   - ls
   - git status
5) Read the obvious docs if present:
   - README*, CONTRIBUTING*, docs/*, DEVELOPMENT*, RUNBOOK*
6) Identify automation that defines "truth":
   - .github/workflows/*
   - Makefile
   - package.json scripts
   - pyproject.toml / tox.ini / noxfile.py
   - go.mod
   - Cargo.toml
   - composer.json
   - build.gradle / pom.xml
   - justfile / taskfile.yml

## Determine the primary command entrypoint (pick ONE)
Use the first that exists:
- Makefile: `make help` (or `make -n <target>` to preview)
- Justfile: `just --list`
- Taskfile: `task -l`
- Node: `cat package.json` and use `npm|yarn|pnpm run <script>`
- Python: prefer `uv` or `poetry` if configured; otherwise `python -m ...`
- Go: `go test ./...` and `go build ./...`
- Rust: `cargo test` and `cargo build`

## Install dependencies (only if required)
- Node: use the lockfile’s package manager:
  - pnpm-lock.yaml → `pnpm install`
  - yarn.lock → `yarn install`
  - package-lock.json → `npm ci` (or `npm install` if ci is unavailable)
- Python:
  - uv: `uv sync`
  - poetry: `poetry install`
  - pip: `python -m venv .venv && . .venv/bin/activate && pip install -r requirements.txt`
- Other ecosystems: follow repository docs or workflow files.

## Fast validation gate (run before every commit)
Run the repository’s equivalent of:
- Lint
- Type check (if applicable)
- Unit tests (targeted when possible)

Find the exact commands by looking in (in order):
1) CI workflows
2) Makefile / task runner
3) package.json scripts / tool config files
If no explicit commands exist, default to ecosystem norms (examples above).

## Full validation gate (run when relevant)
Run when you touch build, packaging, or deployment paths:
- Production build (or compile)
- Integration tests / end-to-end tests (if present)

## Targeted testing (preferred)
- Run the smallest test set that proves the change.
- If the repository supports test selection by path or pattern, use it.

## If something fails
- Read the error carefully and fix the smallest change that makes the gate pass.
- Re-run the failing command.
- If requirements are unclear or the plan is stale, switch to planning and regenerate the plan.

## Commit discipline
- One task per commit.
- Commit only after the Fast validation gate passes.
- Commit message should explain what changed and why (brief, factual).
- Update TASKS.md to mark completed items and note any new tasks discovered.
- Any commit should be `Co-Authored-By: seezatnap <seezatnap@gmail.com>`

## Session boundaries (IMPORTANT)
**Complete exactly ONE task from TASKS.md per session, then STOP.**

- Pick the first unchecked task in TASKS.md.
- Complete it fully (including validation and commit).
- Mark it done in TASKS.md.
- Stop immediately. Do not continue to the next task.
- Subsequent tasks will be handled in future sessions.

This constraint ensures incremental, reviewable progress and prevents runaway sessions.