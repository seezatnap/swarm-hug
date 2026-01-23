# Tasks

- [x] Add `set-email` command that creates `.swarm-hug/email.txt` with provided email
- [x] Update agent prompt to include Co-Authored-By line using email from `.swarm-hug/email.txt`
- [x] Ensure agents are unassigned after each sprint completes

## Add --with-prd flag to team init
- [x] Add `--with-prd` CLI flag and `prd_file_arg` field to CliArgs in config.rs
- [x] Create `prd_to_tasks.md` prompt template for PRD conversion
- [x] Add `prd_to_tasks` prompt to prompt.rs embedded prompts
- [x] Add PRD conversion function to planning.rs
- [x] Update `cmd_team_init` in main.rs to use PRD when --with-prd is provided
