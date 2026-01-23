# PRD to Tasks Conversion

You are a technical project manager. Convert the following PRD (Product Requirements Document) into a structured task list for a development team.

## Requirements

1. **Task Size**: Each task should be approximately 3 story points (roughly half a day to a day of work for one developer).
2. **Task Format**: Each task must be a markdown checkbox item: `- [ ] Task description`
3. **Organization**: Group tasks under markdown subheadings (## Section Name) based on work areas (e.g., "## Backend API", "## Frontend UI", "## Database", "## Testing", "## Documentation").
4. **Specificity**: Tasks should be specific and actionable, not vague.
5. **Dependencies**: If a task depends on another, add "BLOCKED:" prefix (e.g., "- [ ] BLOCKED: Implement user dashboard (needs authentication)")

## Output Format

Output ONLY the task list in markdown format. Start directly with the first section heading. Do not include any preamble, explanation, or summary.

Example output format:
```
## Backend API

- [ ] Implement user registration endpoint with email validation
- [ ] Add JWT token generation for authenticated sessions
- [ ] Create password reset flow with email verification

## Frontend UI

- [ ] Build login form component with validation
- [ ] BLOCKED: Create user dashboard layout (needs authentication endpoints)
```

## PRD Content

{{prd_content}}
