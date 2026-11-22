//! Joins an error and its causes into one line.

/// Joins `messages` (the error first, then each cause) with `: `.
///
/// A cause whose text the previous message already contains is dropped. The library errors
/// interpolate their `source` into their own message, so printing both would repeat it.
#[must_use]
pub fn join(messages: impl IntoIterator<Item = String>) -> String {
    let mut out = String::new();
    let mut previous: Option<String> = None;
    for message in messages {
        match &previous {
            None => out.clone_from(&message),
            Some(before) if before.contains(&message) => {}
            Some(_) => {
                out.push_str(": ");
                out.push_str(&message);
            }
        }
        previous = Some(message);
    }
    out
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
    fn a_cause_already_in_the_message_is_not_repeated() {
        assert_eq!(
            join_all(&["cannot read x: No such file", "No such file"]),
            "cannot read x: No such file"
        );
    }

    #[test]
    fn no_messages_gives_an_empty_line() {
        assert_eq!(join_all(&[]), "");
    }
}
