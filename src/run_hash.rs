//! Run hash generation for sprint run isolation.
//!
//! Generates unique 6-character alphanumeric hashes to identify sprint runs.
//! Each sprint run gets a unique hash, ensuring worktrees and branches from
//! different runs don't conflict.

use rand::Rng;

/// Character set for run hashes: lowercase letters and digits.
/// This ensures git branch name compatibility.
const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";

/// Length of generated run hashes.
const HASH_LEN: usize = 6;

/// Generates a 6-character alphanumeric hash unique to this run.
///
/// Uses lowercase letters and digits for git branch name compatibility.
/// With 36^6 ≈ 2.2 billion possibilities, collisions are extremely unlikely.
///
/// # Examples
/// ```
/// use swarm::run_hash::generate_run_hash;
///
/// let hash = generate_run_hash();
/// assert_eq!(hash.len(), 6);
/// assert!(hash.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
/// ```
pub fn generate_run_hash() -> String {
    let mut rng = rand::thread_rng();
    (0..HASH_LEN)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_hash_length() {
        let hash = generate_run_hash();
        assert_eq!(hash.len(), 6);
    }

    #[test]
    fn test_hash_uniqueness() {
        let hash1 = generate_run_hash();
        let hash2 = generate_run_hash();
        // With 36^6 possibilities, collision is astronomically unlikely
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_hash_uniqueness_bulk() {
        // Generate many hashes and verify no collisions
        let mut hashes = HashSet::new();
        for _ in 0..1000 {
            let hash = generate_run_hash();
            assert!(
                hashes.insert(hash.clone()),
                "Collision detected: {}",
                hash
            );
        }
        assert_eq!(hashes.len(), 1000);
    }

    #[test]
    fn test_hash_is_alphanumeric_lowercase() {
        let hash = generate_run_hash();
        assert!(hash.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
    }

    #[test]
    fn test_hash_contains_no_uppercase() {
        // Generate multiple hashes to increase confidence
        for _ in 0..100 {
            let hash = generate_run_hash();
            assert!(
                !hash.chars().any(|c| c.is_ascii_uppercase()),
                "Hash contains uppercase: {}",
                hash
            );
        }
    }

    #[test]
    fn test_hash_is_git_branch_safe() {
        // Git branch names cannot contain: space, ~, ^, :, ?, *, [, \, ..
        let invalid_chars = [' ', '~', '^', ':', '?', '*', '[', '\\', '.'];
        for _ in 0..100 {
            let hash = generate_run_hash();
            for c in &invalid_chars {
                assert!(
                    !hash.contains(*c),
                    "Hash contains invalid git character '{}': {}",
                    c,
                    hash
                );
            }
        }
    }

    #[test]
    fn test_hash_not_empty() {
        let hash = generate_run_hash();
        assert!(!hash.is_empty());
    }

    #[test]
    fn test_hash_uses_expected_charset() {
        // Generate many hashes and verify we see variety in characters
        let mut seen_chars = HashSet::new();
        for _ in 0..1000 {
            let hash = generate_run_hash();
            for c in hash.chars() {
                seen_chars.insert(c);
            }
        }
        // With 1000 hashes of 6 chars each, we should see most of the 36 chars
        // Using a conservative threshold
        assert!(
            seen_chars.len() >= 30,
            "Expected to see at least 30 different characters, got {}",
            seen_chars.len()
        );
    }
}
