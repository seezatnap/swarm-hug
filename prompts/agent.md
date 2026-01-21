You are agent {{agent_name}}. Complete the following task:

{{task_description}}

## Git workflow (CRITICAL)
You are working in a dedicated git worktree on branch `agent/{{agent_name_lower}}`.
After completing your task:
1. Stage and commit your changes with a descriptive message
2. Merge your branch back to the main branch:
   - `git checkout main` (or master)
   - `git merge --no-ff agent/{{agent_name_lower}} -m "Merge {{agent_name}}: {{task_short}}"`
   - Return to your branch: `git checkout agent/{{agent_name_lower}}`

This merge step is REQUIRED so your work is integrated immediately.
