Look at SPECS.md, make an exhaustive TASKS.md. Then (or if it's already there) take the most important task and do it.

Use ../ralph-bash-v2 as a reference but note that that's the older, fragile project and you should just use it as a reference.

After each turn make sure the README.md is updated and accurate, accessible, and friendly.

Do up to 3 related tasks at once.

If you encounter files larger than 1000 LOC, make tasks to break them apart.

DON'T FORGET TO CHECK IN YOUR WORK


REFACTOR REQUEST:

I want all of the config an artificats form swarm-hug to live in `.swarm-hug`. inside you'll have a number of teams folders i.e.

.swarm-hug/authentication
.swarm-hug/payments

each "team" in that folder has its own specs, prompt, tasks. any arifacts (loop folder, worktree folders) should live INSIDE here. this way multiple teams can initialize and work on the main repo simultaneously without running into each other

.swarm-hug/assignments.yaml/toml/whatever is best should assign the canonical alpahabetal agent names (aaron, betty, carlos etc) to each team automatically -- i.e. aaron can't be assigned or working on two different teams