# Specifications: worktrees

// FIRST WE CREATE A FEATURE WORKTREE
georgepantazis@Georges-MacBook-Air git-tester (main) % git worktree add -b work/feature ./worktrees/wt-feature main
Preparing worktree (new branch 'work/feature')
HEAD is now at c2ec34b init

// NEXT WE CREATE A WORKTREE TO WORK ON A TASK WITHIN THE FEATURE
georgepantazis@Georges-MacBook-Air git-tester (main) % git worktree add -b work/feature-part ./worktrees/wt-feature-part
 work/feature
Preparing worktree (new branch 'work/feature-part')
HEAD is now at c2ec34b init

// WE NOW HAVE MAIN PLUS OUR TWO WORKTREES
georgepantazis@Georges-MacBook-Air git-tester (main) % git branch
* main
+ work/feature
+ work/feature-part

// EXAMPLE COMMIT WITHIN THE TASK WORKTREE
georgepantazis@Georges-MacBook-Air git-tester (main) % cd worktrees/wt-feature-part
georgepantazis@Georges-MacBook-Air wt-feature-part (work/feature-part) % touch foo.md
georgepantazis@Georges-MacBook-Air wt-feature-part (work/feature-part) % git add .
georgepantazis@Georges-MacBook-Air wt-feature-part (work/feature-part) % git commit -m "add foo"
[work/feature-part 128e984] add foo
 1 file changed, 0 insertions(+), 0 deletions(-)
 create mode 100644 foo.md
georgepantazis@Georges-MacBook-Air wt-feature-part (work/feature-part) % git lg
* 128e984 - (HEAD -> work/feature-part) add foo (1 second ago) <seezatnap>
* c2ec34b - (work/feature, main) init (4 minutes ago) <seezatnap>

// ONCE THE TASK IS COMPLETE WE CAN MERGE IT INTO THE FEATURE WORKTREE
georgepantazis@Georges-MacBook-Air wt-feature-part (work/feature-part) % cd ../wt-feature
georgepantazis@Georges-MacBook-Air wt-feature (work/feature) % git merge --no-ff work/feature-part
Merge made by the 'ort' strategy.
 foo.md | 0
 1 file changed, 0 insertions(+), 0 deletions(-)
 create mode 100644 foo.md
georgepantazis@Georges-MacBook-Air wt-feature (work/feature) % git lg
*   d5179b7 - (HEAD -> work/feature) Merge branch 'work/feature-part' into work/feature (3 seconds ago) <seezatnap>
|\
| * 128e984 - (work/feature-part) add foo (59 seconds ago) <seezatnap>
|/
* c2ec34b - (main) init (5 minutes ago) <seezatnap>

// LET'S DESTROY THE TASK WORKTREE. BACK TO ROOT REPO.
georgepantazis@Georges-MacBook-Air wt-feature (work/feature) % cd ../../
georgepantazis@Georges-MacBook-Air git-tester (main) % git lg
* c2ec34b - (HEAD -> main) init (6 minutes ago) <seezatnap>
georgepantazis@Georges-MacBook-Air git-tester (main) % git worktree remove ./worktrees/wt-feature-part
georgepantazis@Georges-MacBook-Air git-tester (main) % git branch
* main
+ work/feature
  work/feature-part
// WE MUST -D FORCE DELETE IT SINCE IT WASN'T MERGED INTO MAIN YET, BUT THE WORK IS IN THE FEATURE WORKTREE
georgepantazis@Georges-MacBook-Air git-tester (main) % git branch -D work/feature-part
Deleted branch work/feature-part (was 128e984).

// NOW LET'S SAY THE FEATURE IS COMPLETE. WE'LL MERGE IT INTO MAIN NOW.
georgepantazis@Georges-MacBook-Air git-tester (main) % git merge --no-ff work/feature
Merge made by the 'ort' strategy.
 foo.md | 0
 1 file changed, 0 insertions(+), 0 deletions(-)
 create mode 100644 foo.md
georgepantazis@Georges-MacBook-Air git-tester (main) % git lg
*   ece49f8 - (HEAD -> main) Merge branch 'work/feature' (3 seconds ago) <seezatnap>
|\
| * d5179b7 - (work/feature) Merge branch 'work/feature-part' into work/feature (3 minutes ago) <seezatnap>
|/|
| * 128e984 - add foo (4 minutes ago) <seezatnap>
|/
* c2ec34b - init (8 minutes ago) <seezatnap>

// FINALLY, LET'S REMOVE THE FEATURE WORKTREE
georgepantazis@Georges-MacBook-Air git-tester (main) % git worktree remove ./worktrees/wt-feature
georgepantazis@Georges-MacBook-Air git-tester (main) % git branch
* main
  work/feature
georgepantazis@Georges-MacBook-Air git-tester (main) % git branch -d work/feature
Deleted branch work/feature (was d5179b7).


---------------

The above is an example of how i want to orchestrate work in Swarm Hug. 
Per sprint, I want to create a feature branch under ./swarm-hug/project-name/worktrees i.e. for the greenfield project, greenfield-sprint-1
We'll then sprint plan. Each agent should get a worktree forked form the feature branch as shown above for "tasks" worktrees. i.e. a worktree called agent-aaron
After they complete a task, it is merged into greenfield-sprint-1 and their worktree is deleted. if they have another task, it is recreated fresh from greenfield-sprint-1
when all sprint work is complete, the feature branch is merged in main/master (we should determine which automatically) -- main/master would be the "target branch"
the feature/sprint branch is then deleted.
We should support customizing the target branch with --target-branch, which is where the feature branch would have forked from originally AND where it will be merged back to on completion.

Merging should be done by the agents into the feature branch, as it is now.
We must add a "merge agent" prompt + execution that's responsible for merging the feature branch into the main branch and resolving any conflicts. The agent should be instructed that they should not destroy any uptream, just focus on getting their code + tests out of conflict.
Sprint planning, postmortem, and sprint close commits should all be done within the feature/sprint branch.
