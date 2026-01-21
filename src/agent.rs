//! Agent naming module with A-Z name/initial mappings.
//!
//! Provides deterministic agent names for assignment tracking.
//! The mapping is identical to ralph-bash-v2.

/// All agent initials in order (A-Z).
pub const INITIALS: [char; 26] = [
    'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M',
    'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z',
];

/// All agent names in order (Aaron through Zane).
pub const NAMES: [&str; 26] = [
    "Aaron", "Betty", "Carlos", "Diana", "Ethan", "Fiona", "George", "Hannah",
    "Ian", "Julia", "Kevin", "Laura", "Miguel", "Nadia", "Omar", "Priya",
    "Quinn", "Rosa", "Sam", "Tina", "Uma", "Victor", "Wendy", "Xavier",
    "Yara", "Zane",
];

/// Get agent name from initial.
///
/// # Examples
/// ```
/// use swarm::agent::name_from_initial;
/// assert_eq!(name_from_initial('A'), Some("Aaron"));
/// assert_eq!(name_from_initial('Z'), Some("Zane"));
/// assert_eq!(name_from_initial('1'), None);
/// ```
pub fn name_from_initial(initial: char) -> Option<&'static str> {
    let upper = initial.to_ascii_uppercase();
    if upper.is_ascii_uppercase() {
        let idx = (upper as u8 - b'A') as usize;
        Some(NAMES[idx])
    } else {
        None
    }
}

/// Get initial from agent name.
///
/// # Examples
/// ```
/// use swarm::agent::initial_from_name;
/// assert_eq!(initial_from_name("Aaron"), Some('A'));
/// assert_eq!(initial_from_name("Zane"), Some('Z'));
/// assert_eq!(initial_from_name("Unknown"), None);
/// ```
pub fn initial_from_name(name: &str) -> Option<char> {
    NAMES.iter().position(|&n| n == name).map(|idx| INITIALS[idx])
}

/// Get the first N agent names starting from A.
///
/// # Examples
/// ```
/// use swarm::agent::get_names;
/// assert_eq!(get_names(3), vec!["Aaron", "Betty", "Carlos"]);
/// assert_eq!(get_names(0), Vec::<&str>::new());
/// ```
pub fn get_names(count: usize) -> Vec<&'static str> {
    NAMES.iter().take(count).copied().collect()
}

/// Get the first N agent initials starting from A.
///
/// # Examples
/// ```
/// use swarm::agent::get_initials;
/// assert_eq!(get_initials(3), vec!['A', 'B', 'C']);
/// ```
pub fn get_initials(count: usize) -> Vec<char> {
    INITIALS.iter().take(count).copied().collect()
}

/// Check if a character is a valid agent initial.
pub fn is_valid_initial(initial: char) -> bool {
    initial.to_ascii_uppercase().is_ascii_uppercase()
}

/// Check if a string is a valid agent name.
pub fn is_valid_name(name: &str) -> bool {
    NAMES.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_from_initial_a() {
        assert_eq!(name_from_initial('A'), Some("Aaron"));
    }

    #[test]
    fn test_name_from_initial_b() {
        assert_eq!(name_from_initial('B'), Some("Betty"));
    }

    #[test]
    fn test_name_from_initial_z() {
        assert_eq!(name_from_initial('Z'), Some("Zane"));
    }

    #[test]
    fn test_name_from_initial_lowercase() {
        assert_eq!(name_from_initial('a'), Some("Aaron"));
        assert_eq!(name_from_initial('z'), Some("Zane"));
    }

    #[test]
    fn test_name_from_initial_invalid() {
        assert_eq!(name_from_initial('1'), None);
        assert_eq!(name_from_initial('!'), None);
    }

    #[test]
    fn test_initial_from_name_aaron() {
        assert_eq!(initial_from_name("Aaron"), Some('A'));
    }

    #[test]
    fn test_initial_from_name_betty() {
        assert_eq!(initial_from_name("Betty"), Some('B'));
    }

    #[test]
    fn test_initial_from_name_zane() {
        assert_eq!(initial_from_name("Zane"), Some('Z'));
    }

    #[test]
    fn test_initial_from_name_invalid() {
        assert_eq!(initial_from_name("Unknown"), None);
        assert_eq!(initial_from_name("aaron"), None); // case-sensitive
    }

    #[test]
    fn test_get_names_three() {
        let names = get_names(3);
        assert_eq!(names.len(), 3);
        assert_eq!(names[0], "Aaron");
        assert_eq!(names[1], "Betty");
        assert_eq!(names[2], "Carlos");
    }

    #[test]
    fn test_get_names_one() {
        let names = get_names(1);
        assert_eq!(names.len(), 1);
        assert_eq!(names[0], "Aaron");
    }

    #[test]
    fn test_get_names_all() {
        let names = get_names(26);
        assert_eq!(names.len(), 26);
        assert_eq!(names[0], "Aaron");
        assert_eq!(names[25], "Zane");
    }

    #[test]
    fn test_get_names_zero() {
        let names = get_names(0);
        assert!(names.is_empty());
    }

    #[test]
    fn test_get_names_over_26() {
        let names = get_names(100);
        assert_eq!(names.len(), 26);
    }

    #[test]
    fn test_get_initials_three() {
        let initials = get_initials(3);
        assert_eq!(initials, vec!['A', 'B', 'C']);
    }

    #[test]
    fn test_is_valid_initial_valid() {
        assert!(is_valid_initial('A'));
        assert!(is_valid_initial('Z'));
        assert!(is_valid_initial('a'));
        assert!(is_valid_initial('z'));
    }

    #[test]
    fn test_is_valid_initial_invalid() {
        assert!(!is_valid_initial('1'));
        assert!(!is_valid_initial('!'));
    }

    #[test]
    fn test_is_valid_name_valid() {
        assert!(is_valid_name("Aaron"));
        assert!(is_valid_name("Zane"));
    }

    #[test]
    fn test_is_valid_name_invalid() {
        assert!(!is_valid_name("Unknown"));
        assert!(!is_valid_name("aaron"));
    }

    #[test]
    fn test_all_names_unique() {
        let mut seen = std::collections::HashSet::new();
        for name in NAMES {
            assert!(seen.insert(name), "Duplicate name: {}", name);
        }
    }

    #[test]
    fn test_name_initial_roundtrip() {
        for (i, &name) in NAMES.iter().enumerate() {
            let initial = INITIALS[i];
            assert_eq!(name_from_initial(initial), Some(name));
            assert_eq!(initial_from_name(name), Some(initial));
        }
    }
}
