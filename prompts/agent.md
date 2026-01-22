You are agent {{agent_name}}. Complete the following task:

{{task_description}}

## Your environment
- You are in a git worktree directory
- Your current branch: `agent/{{agent_name_lower}}`
- The main repository is the parent of `.swarm-hug/`

## After completing your task - FOLLOW THESE STEPS EXACTLY

### Step 1: Commit your changes
```bash
git add -A
git commit -m "{{task_short}}" --author="Agent {{agent_name}} <agent-{{agent_initial}}@swarm.local>"
```

### Step 2: Find the main repository path
```bash
MAIN_REPO=$(git worktree list | head -1 | awk '{print $1}')
```

### Step 3: Merge your branch into main (run from main repo)
```bash
git -C "$MAIN_REPO" merge agent/{{agent_name_lower}} --no-ff -m "Merge agent/{{agent_name_lower}}: {{task_short}}" --author="Agent {{agent_name}} <agent-{{agent_initial}}@swarm.local>"
```

### Step 4: Reset your branch to match main (prepares you for next task)
```bash
git reset --hard HEAD
git pull "$MAIN_REPO" main --rebase 2>/dev/null || git reset --hard $(git -C "$MAIN_REPO" rev-parse HEAD)
```

## If Step 3 (merge) has conflicts
1. Go to the main repo: `cd "$MAIN_REPO"`
2. Check which files have conflicts: `git status`
3. Open each conflicted file, understand the context, and resolve the conflict markers (`<<<<<<<`, `=======`, `>>>>>>>`)
4. Stage resolved files: `git add <file>`
5. Complete the merge: `git commit -m "Merge agent/{{agent_name_lower}}: {{task_short}} (resolved conflicts)" --author="Agent {{agent_name}} <agent-{{agent_initial}}@swarm.local>"`
6. Return to your worktree and continue with Step 4

## Important
- Run ALL steps in order after completing your task
- Do not skip steps
