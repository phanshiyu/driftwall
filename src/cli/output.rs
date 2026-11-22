//! The only code in driftwall that writes to the terminal.

use std::io::{self, Write};

/// Writes `text` to stdout exactly as given and flushes.
///
/// A closed pipe (`driftwall lint | head`) is not an error: the reader chose to stop, so the
/// write is abandoned quietly and the exit code stays what the command decided.
///
/// # Errors
/// Any write failure other than a closed pipe.
pub fn stdout(text: &str) -> io::Result<()> {
    quiet_on_closed_pipe(write_all(&mut io::stdout().lock(), text))
}

/// Writes `text` to stderr exactly as given.
///
/// Failures are dropped: there is nowhere left to report that stderr itself is unwritable.
pub fn stderr(text: &str) {
    // why: stderr is the last resort for reporting problems, so its own failure has no outlet.
    let _ = write_all(&mut io::stderr().lock(), text);
}

fn write_all(sink: &mut impl Write, text: &str) -> io::Result<()> {
    sink.write_all(text.as_bytes())?;
    sink.flush()
}

fn quiet_on_closed_pipe(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sink whose reader has gone away.
    struct Closed;

    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn write_all_passes_bytes_through_unchanged() {
        let mut sink = Vec::new();
        write_all(&mut sink, "a\nb\n").unwrap();
        assert_eq!(sink, b"a\nb\n");
    }

    #[test]
    fn a_closed_pipe_is_swallowed() {
        assert!(quiet_on_closed_pipe(write_all(&mut Closed, "x")).is_ok());
    }

    #[test]
    fn other_write_errors_are_kept() {
        let denied = Err(io::ErrorKind::PermissionDenied.into());
        assert!(quiet_on_closed_pipe(denied).is_err());
    }
}
