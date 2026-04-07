/// Sanitize a partition name into a valid Turso database name.
///
/// Turso database names allow only lowercase letters, digits, and dashes,
/// with a maximum length of 64 characters. The result is `{prefix}-{sanitized}`.
pub(crate) fn sanitize_database_name(partition_name: &str, prefix: &str) -> String {
    if partition_name.is_empty() {
        return prefix.to_string();
    }

    let raw = format!("{prefix}-{partition_name}");
    let mut result = String::with_capacity(64);
    let mut prev_dash = false;

    for c in raw.chars() {
        let out = match c {
            'a'..='z' | '0'..='9' => {
                prev_dash = false;
                c
            }
            'A'..='Z' => {
                prev_dash = false;
                c.to_ascii_lowercase()
            }
            _ => {
                if prev_dash || result.is_empty() {
                    continue;
                }
                prev_dash = true;
                '-'
            }
        };
        result.push(out);
        if result.len() >= 64 {
            break;
        }
    }

    // Strip trailing dash
    while result.ends_with('-') {
        result.pop();
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_lowercase_name_gets_prefixed() {
        assert_eq!(sanitize_database_name("orders", "myapp"), "myapp-orders");
    }

    #[test]
    fn uppercase_is_lowercased() {
        assert_eq!(sanitize_database_name("Orders", "myapp"), "myapp-orders");
    }

    #[test]
    fn special_chars_become_dashes() {
        assert_eq!(
            sanitize_database_name("tenant:acme/us-east", "myapp"),
            "myapp-tenant-acme-us-east"
        );
    }

    #[test]
    fn underscores_become_dashes() {
        assert_eq!(sanitize_database_name("my_db", "app"), "app-my-db");
    }

    #[test]
    fn consecutive_dashes_are_collapsed() {
        assert_eq!(sanitize_database_name("a--b", "myapp"), "myapp-a-b");
    }

    #[test]
    fn trailing_special_chars_are_stripped() {
        assert_eq!(sanitize_database_name("trail:", "myapp"), "myapp-trail");
    }

    #[test]
    fn long_names_are_truncated_to_64_chars() {
        let long_name = "a".repeat(100);
        let result = sanitize_database_name(&long_name, "myapp");
        assert!(result.len() <= 64);
        assert!(result.starts_with("myapp-"));
    }

    #[test]
    fn empty_partition_name_returns_prefix_only() {
        assert_eq!(sanitize_database_name("", "myapp"), "myapp");
    }

    #[test]
    fn mixed_case_complex_name() {
        assert_eq!(
            sanitize_database_name("Tenant:ACME/US_East", "ev"),
            "ev-tenant-acme-us-east"
        );
    }
}
