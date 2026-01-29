# PRD to Tasks Conversion

You are a technical project manager. Convert the following PRD (Product Requirements Document) into a structured task list for a development team.

## Process

Complete this in TWO passes:

### Pass 1: Initial Breakdown with Story Points

Break down the PRD into granular tasks. For each task, estimate story points (1 pt = ~2 hours of work). Include the estimate in the task format.

### Pass 2: Consolidation to ~5 Point Tasks

Review your initial breakdown and combine or split tasks so that each final task is approximately **5 story points**. When consolidating:
- Group related tasks that would naturally be done together
- Combine tasks that are chained in execution (one immediately follows another)
- Keep all context from merged tasks in the consolidated task description
- Split tasks that are significantly larger than 5 points

## Task Format

Each task must be a markdown checkbox with:
- Sequential number: `(#N)`
- Task description with full context
- Story points: `[X pts]`
- Dependencies (if any): `(blocked by #N)` or `(blocked by #1, #2)`

Format: `- [ ] (#N) Task description [X pts]` or `- [ ] (#N) Task description [X pts] (blocked by #1, #2)`

Numbers start at 1 and increment across all sections (global numbering, not per-section).

## Organization

Group tasks under markdown subheadings (## Section Name) based on work areas (e.g., "## Backend API", "## Frontend UI", "## Database", "## Testing").

## Output Format

**CRITICAL**: Output ONLY the raw task list. Your response must start with `## ` (the first section heading) and end with the last task.

Do NOT include:
- Any introductory text (e.g., "Here's the task list:", "I've converted...")
- Any explanatory text before or after the tasks
- Any summary or closing remarks (e.g., "I've also created...", "Let me know if...")
- The initial pass breakdown (only output the final consolidated version)

Example of correct output (note: starts immediately with section heading):
```
## Backend API

- [ ] (#1) Implement user registration endpoint with email validation and JWT token generation for authenticated sessions [5 pts]
- [ ] (#2) Create password reset flow with email verification and token expiry handling [5 pts]

## Frontend UI

- [ ] (#3) Build login form component with validation and error handling [4 pts]
- [ ] (#4) Create user dashboard layout with navigation and user profile display [5 pts] (blocked by #1)
- [ ] (#5) Implement password reset form with confirmation flow [4 pts] (blocked by #2)
```

## PRD Content

{{prd_content}}
