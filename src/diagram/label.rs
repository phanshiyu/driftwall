//! The edge label words that restrict which import sites an edge allows.

use super::error::DiagramError;

const TEST_WORD: &str = "test";
const TYPE_WORD: &str = "type";

/// The words `type` and `test` of an edge label, as `(type, test)`.
///
/// A label is a comma-separated list. It is meaningful only when every word, trimmed, is `type` or
/// `test`; any other label is display only and gives `(false, false)`, so `type, helper` and `Test`
/// are plain labels.
///
/// # Errors
/// Returns [`DiagramError::RepeatedLabelWord`] when a meaningful label names a word twice, such as
/// `type, type`: a typo must not stay a silent no-op.
pub(super) fn restrictions(label: &str, line: u32) -> Result<(bool, bool), DiagramError> {
    let words: Vec<&str> = label.split(',').map(str::trim).collect();
    if !words
        .iter()
        .all(|word| matches!(*word, TYPE_WORD | TEST_WORD))
    {
        return Ok((false, false));
    }
    let mut seen = (false, false);
    for word in words {
        let slot = if word == TYPE_WORD {
            &mut seen.0
        } else {
            &mut seen.1
        };
        if *slot {
            return Err(DiagramError::RepeatedLabelWord {
                line,
                word: word.to_owned(),
            });
        }
        *slot = true;
    }
    Ok(seen)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_words_are_read_in_either_order_with_any_spacing() {
        assert_eq!(restrictions("test", 1), Ok((false, true)));
        assert_eq!(restrictions(" type ", 1), Ok((true, false)));
        assert_eq!(restrictions("type, test", 1), Ok((true, true)));
        assert_eq!(restrictions("test,type", 1), Ok((true, true)));
    }

    #[test]
    fn every_other_label_is_plain() {
        for plain in [
            "",
            "Test",
            "tests",
            "type, helper",
            "types",
            "type test",
            "type,",
            "x",
        ] {
            assert_eq!(restrictions(plain, 1), Ok((false, false)), "{plain}");
        }
    }

    #[test]
    fn a_repeated_word_is_an_error_naming_it() {
        assert_eq!(
            restrictions("type, type", 4),
            Err(DiagramError::RepeatedLabelWord {
                line: 4,
                word: "type".to_owned()
            })
        );
        assert!(restrictions("test,test", 1).is_err());
    }
}
