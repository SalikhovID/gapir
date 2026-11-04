//! Release version comparison for the update notice.

/// True when `latest_tag` (like `v0.2.0`) is newer than `current`. Malformed input is "no update".
pub fn is_newer(current: &str, latest_tag: &str) -> bool {
    match (parse(current), parse(latest_tag)) {
        (Some(current), Some(latest)) => latest > current,
        _ => false,
    }
}

fn parse(version: &str) -> Option<[u64; 3]> {
    let mut parts = version.strip_prefix('v').unwrap_or(version).split('.');
    let mut numbers = [0; 3];
    for number in &mut numbers {
        *number = parts.next()?.parse().ok()?;
    }
    parts.next().is_none().then_some(numbers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_patch_minor_major() {
        assert!(is_newer("0.1.0", "v0.1.1"));
        assert!(is_newer("0.1.9", "v0.2.0"));
        assert!(is_newer("0.9.9", "v1.0.0"));
    }

    #[test]
    fn numbers_compare_as_numbers_not_text() {
        assert!(is_newer("0.1.9", "v0.1.10"));
        assert!(!is_newer("0.1.10", "v0.1.9"));
    }

    #[test]
    fn same_or_older_is_not_newer() {
        assert!(!is_newer("0.2.0", "v0.2.0"));
        assert!(!is_newer("0.2.0", "v0.1.9"));
    }

    #[test]
    fn the_v_prefix_is_optional() {
        assert!(is_newer("0.1.0", "0.1.1"));
        assert!(is_newer("v0.1.0", "v0.1.1"));
    }

    #[test]
    fn malformed_versions_are_not_updates() {
        assert!(!is_newer("0.1.0", "model-rubaistt-v2-q5km"));
        assert!(!is_newer("0.1.0", ""));
        assert!(!is_newer("0.1.0", "v1.2"));
        assert!(!is_newer("garbage", "v1.0.0"));
    }
}
