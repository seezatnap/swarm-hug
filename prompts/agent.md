You are agent {{agent_name}}. Complete the following task:

{{task_description}}

## Your environment
- You are in a git worktree directory
- Your current branch: `agent/{{agent_name_lower}}`
- The main repository is the parent of `.swarm-hug/`

## Team context (IMPORTANT - read before starting)
Your team's context files are located in the main repository at `{{team_dir}}/`:

1. **Read `$MAIN_REPO/{{team_dir}}/prompt.md` first** - Contains the goals and requirements for the current work
2. **Review `$MAIN_REPO/{{team_dir}}/specs.md`** - Detailed specifications that may help you understand your task
3. **Check `$MAIN_REPO/{{team_dir}}/tasks.md`** - See what tasks are assigned and in progress

To access these files, first get the main repo path:
```bash
MAIN_REPO=$(git worktree list | head -1 | awk '{print $1}')
cat "$MAIN_REPO/{{team_dir}}/prompt.md"
```

Understanding the broader context will help you complete your task correctly.

## Golden rules
- Do not assume the stack. Discover it from files and existing automation.
- Prefer existing scripts and tools already used by the repository.
- Do not add new dependencies unless required by your task.
- Do not commit secrets. Do not print tokens. Do not modify lockfiles unless necessary.
- Complete ONE task fully, then stop. Do not work on other tasks.
- **Test everything you add.** If you write code, write tests for it. Run build and test to confirm validity.

## First steps (orientation)
Before writing code, understand the codebase:

1. Inspect the root: `ls` and `git status`
2. Read obvious docs if present: README*, CONTRIBUTING*, docs/*, DEVELOPMENT*
3. Identify automation that defines "truth":
   - .github/workflows/*
   - Makefile / justfile / taskfile.yml
   - package.json scripts
   - Cargo.toml / go.mod

## Determine the validation commands
Find the repository's lint/test/build commands by checking (in order):
1. CI workflows (.github/workflows/)
2. Makefile / task runner (`make help`, `just --list`)
3. package.json scripts / tool config files

Common patterns:
- Node: `npm test`, `npm run lint`, `npm run build`
- Rust: `cargo test`, `cargo clippy`, `cargo build`
- Go: `go test ./...`, `go build ./...`

## Install dependencies (only if required)
- Node: use the lockfile's package manager (pnpm-lock.yaml → `pnpm install`, yarn.lock → `yarn install`, package-lock.json → `npm ci`)
- Other ecosystems: follow repository docs or workflow files.

## Validation gate (run before committing)
Before committing, run the repository's equivalent of:
- Build (must succeed)
- Lint
- Type check (if applicable)
- Tests (targeted to your changes when possible, but full test suite is preferred)

**If you added new functionality, you MUST add tests for it.** Untested code is incomplete code.

If something fails:
- Read the error carefully
- Fix the smallest change that makes the gate pass
- Re-run the failing command

## After completing your task - FOLLOW THESE STEPS EXACTLY

### Step 1: Validate your changes
Run the validation gate. Do not commit if tests or lint fail.

### Step 2: Commit your changes
Use **Conventional Commits** format: `type(scope): description`
- Types: `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`
- Example: `feat(auth): add login validation`

```bash
# First, check what you're about to commit - avoid committing artifacts!
git add -A
git diff --stat --cached

# Review the --stat output. Remove any files that shouldn't be versioned:
# - Build artifacts (dist/, build/, target/, *.o, *.pyc)
# - Dependencies (node_modules/, vendor/)
# - IDE files (.idea/, .vscode/ unless project config)
# - Secrets or credentials
# If you see unwanted files, use: git reset HEAD <file>

# Then commit with conventional commit format
git commit -m "type(scope): {{task_short}}{{co_author}}" --author="Agent {{agent_name}} <agent-{{agent_initial}}@swarm.local>"
```

### Step 3: Find the main repository path
```bash
MAIN_REPO=$(git worktree list | head -1 | awk '{print $1}')
```

### Step 4: Merge your branch into main (run from main repo)
```bash
GIT_AUTHOR_NAME="Agent {{agent_name}}" GIT_AUTHOR_EMAIL="agent-{{agent_initial}}@swarm.local" GIT_COMMITTER_NAME="Agent {{agent_name}}" GIT_COMMITTER_EMAIL="agent-{{agent_initial}}@swarm.local" git -C "$MAIN_REPO" merge agent/{{agent_name_lower}} --no-ff -m "Merge agent/{{agent_name_lower}}: {{task_short}}{{co_author}}"
```

### Step 5: Reset your branch to match main (prepares you for next task)
```bash
git reset --hard HEAD
git pull "$MAIN_REPO" main --rebase 2>/dev/null || git reset --hard $(git -C "$MAIN_REPO" rev-parse HEAD)
```

## If Step 4 (merge) has conflicts
1. Go to the main repo: `cd "$MAIN_REPO"`
2. Check which files have conflicts: `git status`
3. Open each conflicted file, understand the context, and resolve the conflict markers (`<<<<<<<`, `=======`, `>>>>>>>`)
4. Stage resolved files: `git add <file>`
5. Complete the merge: `GIT_AUTHOR_NAME="Agent {{agent_name}}" GIT_AUTHOR_EMAIL="agent-{{agent_initial}}@swarm.local" GIT_COMMITTER_NAME="Agent {{agent_name}}" GIT_COMMITTER_EMAIL="agent-{{agent_initial}}@swarm.local" git commit -m "Merge agent/{{agent_name_lower}}: {{task_short}} (resolved conflicts){{co_author}}"`
6. Return to your worktree and continue with Step 5

## Important
- Run ALL steps in order after completing your task
- Do not skip the validation gate
- Do not work on tasks not assigned to you
