//! UTS #39 confusable skeleton, behind the `security` feature.
//!
//! Maps visually similar codepoints (Cyrillic 'а', Latin 'a', Greek 'α',
//! …) to a common skeleton string. Use in security-sensitive matching:
//! domain names, usernames, filename collisions.

use alloc::string::String;
use unicode_security::GeneralSecurityProfile;

/// Map a string through the confusable skeleton.
///
/// Returns the skeleton form of `input` per UTS #39 §4.
pub(super) fn skeleton(input: &str) -> String {
    // The `unicode-security` crate exposes the skeleton via the
    // `confusable_detection::skeleton` iterator on `&str`. The result
    // is a `String`-collectable iterator of `char`.
    use unicode_security::confusable_detection::skeleton;
    skeleton(input).collect()
}

/// Inert helper that lets the `security`-feature module pull
/// [`GeneralSecurityProfile`] into scope when callers want stricter
/// validation. Currently unused but kept for forward-compatibility.
#[allow(dead_code)]
fn _identifier_status_witness(c: char) -> bool {
    c.identifier_allowed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cyrillic_a_collapses_to_latin_a() {
        // Cyrillic small letter a (U+0430) should skeletonize to ascii 'a'.
        let cyr = skeleton("а");
        let lat = skeleton("a");
        assert_eq!(cyr, lat);
    }

    #[test]
    fn paypal_homograph() {
        // "раураl" (Cyrillic а, у) versus "paypal".
        let s1 = skeleton("раураl");
        let s2 = skeleton("paypal");
        assert_eq!(s1, s2);
    }

    #[test]
    fn ascii_passthrough() {
        assert_eq!(skeleton("hello"), "hello");
    }
}
