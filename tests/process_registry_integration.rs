#[cfg(unix)]
use swarm::engine::{ClaudeEngine, CodexEngine, Engine};
#[cfg(unix)]
use swarm::process_registry::PROCESS_REGISTRY;

#[cfg(unix)]
fn sorted_pids() -> Vec<u32> {
    let mut pids = PROCESS_REGISTRY.all_pids();
    pids.sort_unstable();
    pids
}

#[cfg(unix)]
#[test]
fn test_process_registry_cleared_after_engine_exit() {
    let cwd = std::env::current_dir().expect("failed to get current dir");
    let before = sorted_pids();

    let claude = ClaudeEngine::with_path("true");
    let result = claude.execute("ScrumMaster", "test task", &cwd, 0, None);
    assert!(result.success, "claude engine failed: {:?}", result);

    let codex = CodexEngine::with_path("true");
    let result = codex.execute("ScrumMaster", "test task", &cwd, 0, None);
    assert!(result.success, "codex engine failed: {:?}", result);

    let after = sorted_pids();
    assert_eq!(before, after, "process registry should be unchanged after engine execution");
}

#[cfg(not(unix))]
#[test]
fn test_process_registry_cleared_after_engine_exit() {}
