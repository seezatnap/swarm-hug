You are agent {{agent_name}}. Complete the following task:

{{task_description}}

## Git workflow (CRITICAL)
You are working in a dedicated git worktree on branch `agent/{{agent_name_lower}}`.

After completing your task:
1. Stage all your changes: `git add -A`
2. Commit with your agent name:
   ```bash
   git commit -m "{{task_short}}" --author="Agent {{agent_name}} <agent-{{agent_initial}}@swarm.local>"
   ```
3. Find the main repo and merge your branch:
   ```bash
   MAIN_REPO=$(git rev-parse --path-format=absolute --git-common-dir | sed 's|/.git$||')
   git -C "$MAIN_REPO" merge --no-ff agent/{{agent_name_lower}} -m "Merge Agent {{agent_name}}: {{task_short}}"
   ```
4. Reset your branch to match master for the next task:
   ```bash
   git fetch origin master:master 2>/dev/null || git fetch origin main:main 2>/dev/null
   git reset --hard master 2>/dev/null || git reset --hard main
   ```

All steps are REQUIRED so your work is integrated and you're ready for the next task.
