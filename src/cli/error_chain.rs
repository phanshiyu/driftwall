//! Joins an error and its causes into one line.

/// Joins `messages` (the error first, then each cause) with `: `.
///
/// Every library error words only its own context and leaves the cause to `source()`, so the
/// chain is printed as it is, with nothing dropped.
#[must_use]
pub fn join(messages: impl IntoIterator<Item = String>) -> String {
    messages.into_iter().collect::<Vec<_>>().join(": ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn join_all(messages: &[&str]) -> String {
        join(messages.iter().map(|m| (*m).to_owned()))
    }

    #[test]
    fn single_message_is_unchanged() {
        assert_eq!(join_all(&["boom"]), "boom");
    }

    #[test]
    fn causes_follow_the_error_in_order() {
        assert_eq!(join_all(&["a", "b", "c"]), "a: b: c");
    }

    #[test]
    fn identical_messages_are_kept() {
        assert_eq!(join_all(&["a", "a"]), "a: a");
    }

    #[test]
    fn a_cause_contained_in_the_message_is_still_printed() {
        assert_eq!(
            join_all(&["cannot read x: No such file", "No such file"]),
            "cannot read x: No such file: No such file"
        );
    }

    #[test]
    fn no_messages_gives_an_empty_line() {
        assert_eq!(join_all(&[]), "");
    }
}
