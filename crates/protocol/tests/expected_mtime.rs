//! `expectedMtime` to the whole ms core compares.

use peektrace_protocol::expected_mtime;

#[test]
fn only_whole_ms_can_match() {
    assert_eq!(expected_mtime(1_700_000_000_123.0), 1_700_000_000_123);
    assert_eq!(expected_mtime(1.5), i64::MIN);
    assert_eq!(expected_mtime(1e300), i64::MIN);
    assert_eq!(expected_mtime(f64::NAN), i64::MIN);
}
