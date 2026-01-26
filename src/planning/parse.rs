/// Find the byte position of the matching closing brace.
pub(super) fn find_matching_brace(s: &str) -> Option<usize> {
    let mut depth = 0;
    for (byte_pos, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(byte_pos);
                }
            }
            _ => {}
        }
    }
    None
}

/// Find the nearest valid character boundary at or before the given byte index.
pub(super) fn floor_char_boundary(s: &str, index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    let mut i = index;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Find the nearest valid character boundary at or after the given byte index.
pub(super) fn ceil_char_boundary(s: &str, index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    let mut i = index;
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// Parse a number from the start of a string.
pub(super) fn parse_number_at(s: &str) -> Option<usize> {
    let num_str: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    num_str.parse().ok()
}

/// Parse assignments from a JSON string.
pub(super) fn parse_assignments_json(json: &str) -> Option<Vec<(usize, char)>> {
    // Simple manual JSON parsing since we don't want to add serde
    let mut assignments = Vec::new();

    // Find the assignments array
    let array_start = json.find('[')? + 1;
    let array_end = json.rfind(']')?;
    let array_content = &json[array_start..array_end];

    // Split by },{ to get individual assignment objects
    let objects: Vec<&str> = array_content.split("},{").collect();

    for obj in objects {
        let obj = obj.trim_matches(|c| c == '{' || c == '}' || c == ' ');

        // Extract agent - look for "agent":"X"
        let agent = if let Some(pos) = obj.find(r#""agent":"#) {
            let start = pos + 9; // skip "agent":"
            obj.chars().nth(start).filter(|c| c.is_ascii_uppercase())
        } else {
            None
        };

        // Extract line number - look for "line": followed by a number
        // The JSON can have "line":N or "line": N (whitespace collapsed)
        let line = if let Some(pos) = obj.find(r#""line":"#) {
            // Skip past "line": (7 chars)
            parse_number_at(&obj[pos + 7..])
        } else {
            None
        };

        if let (Some(a), Some(l)) = (agent, line) {
            assignments.push((l, a));
        }
    }

    if assignments.is_empty() {
        None
    } else {
        Some(assignments)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_matching_brace() {
        assert_eq!(find_matching_brace("{a}"), Some(2));
        assert_eq!(find_matching_brace("{{a}}"), Some(4));
        assert_eq!(find_matching_brace("{a:{b:c}}"), Some(8));
        assert_eq!(find_matching_brace("{"), None);
    }

    #[test]
    fn test_parse_number_at() {
        assert_eq!(parse_number_at("123abc"), Some(123));
        assert_eq!(parse_number_at("42"), Some(42));
        assert_eq!(parse_number_at("abc"), None);
        assert_eq!(parse_number_at(""), None);
    }

    #[test]
    fn test_find_matching_brace_with_utf8() {
        // Multi-byte characters: '→' is 3 bytes (E2 86 92), '日' etc are 3 bytes each
        // {→} = { (0) → (1-3) } (4) = closing brace at byte 4
        assert_eq!(find_matching_brace("{→}"), Some(4));
        // {日本語} = { (0) 日 (1-3) 本 (4-6) 語 (7-9) } (10) = closing brace at byte 10
        assert_eq!(find_matching_brace("{日本語}"), Some(10));
        // {a→b} = { (0) a (1) → (2-4) b (5) } (6) = closing brace at byte 6
        assert_eq!(find_matching_brace("{a→b}"), Some(6));
    }

    #[test]
    fn test_floor_char_boundary() {
        let s = "a→b"; // bytes: a(1) →(3) b(1) = 5 bytes total
        assert_eq!(floor_char_boundary(s, 0), 0); // 'a' boundary
        assert_eq!(floor_char_boundary(s, 1), 1); // '→' boundary
        assert_eq!(floor_char_boundary(s, 2), 1); // inside '→', floor to 1
        assert_eq!(floor_char_boundary(s, 3), 1); // inside '→', floor to 1
        assert_eq!(floor_char_boundary(s, 4), 4); // 'b' boundary
        assert_eq!(floor_char_boundary(s, 5), 5); // end
        assert_eq!(floor_char_boundary(s, 100), 5); // past end
    }

    #[test]
    fn test_ceil_char_boundary() {
        let s = "a→b"; // bytes: a(1) →(3) b(1) = 5 bytes total
        assert_eq!(ceil_char_boundary(s, 0), 0); // 'a' boundary
        assert_eq!(ceil_char_boundary(s, 1), 1); // '→' boundary
        assert_eq!(ceil_char_boundary(s, 2), 4); // inside '→', ceil to 4 ('b')
        assert_eq!(ceil_char_boundary(s, 3), 4); // inside '→', ceil to 4
        assert_eq!(ceil_char_boundary(s, 4), 4); // 'b' boundary
        assert_eq!(ceil_char_boundary(s, 5), 5); // end
        assert_eq!(ceil_char_boundary(s, 100), 5); // past end
    }
}
