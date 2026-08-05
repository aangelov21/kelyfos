//! Incremental scanner for OSC 133 prompt marks in the PTY byte stream.
//!
//! The marks arrive mid-output and can be split across reads, so the scanner
//! carries partial state between chunks rather than searching each buffer on
//! its own. xterm.js discards OSC sequences it does not handle, so the bytes
//! are left in the stream and never reach the user's eyes.

/// A sequence longer than this is not one of ours; abandoning it keeps a stray
/// ESC ] 133 ; in ordinary output from swallowing the rest of the stream.
const MAX_PAYLOAD: usize = 64;

const PREFIX: &[u8] = b"\x1b]133;";

#[derive(Debug, PartialEq)]
pub enum Mark {
    /// A prompt was drawn: the shell is waiting for input.
    Prompt,
    /// A command started running.
    CommandStart,
    /// A command finished, with its exit status when the shell reported one.
    CommandEnd(Option<i32>),
}

#[derive(Default)]
pub struct MarkScanner {
    /// How much of PREFIX has matched so far.
    prefix_pos: usize,
    /// Payload bytes collected after a complete prefix; None when not in one.
    payload: Option<Vec<u8>>,
    /// Last byte seen, to recognise the two-byte ST terminator (ESC \).
    prev: u8,
}

impl MarkScanner {
    /// Feed a chunk of PTY output; returns the marks it completed.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<Mark> {
        let mut out = Vec::new();
        for &b in chunk {
            if let Some(payload) = self.payload.as_mut() {
                // BEL, or ESC \ — both terminate an OSC sequence.
                if b == 0x07 || (b == b'\\' && self.prev == 0x1b) {
                    if payload.last() == Some(&0x1b) {
                        payload.pop();
                    }
                    if let Some(mark) = parse(payload) {
                        out.push(mark);
                    }
                    self.payload = None;
                } else if payload.len() >= MAX_PAYLOAD {
                    self.payload = None;
                } else {
                    payload.push(b);
                }
            } else if b == PREFIX[self.prefix_pos] {
                self.prefix_pos += 1;
                if self.prefix_pos == PREFIX.len() {
                    self.prefix_pos = 0;
                    self.payload = Some(Vec::new());
                }
            } else {
                // A mismatch may itself start a fresh prefix (ESC ESC ] …).
                self.prefix_pos = usize::from(b == PREFIX[0]);
            }
            self.prev = b;
        }
        out
    }
}

fn parse(payload: &[u8]) -> Option<Mark> {
    let text = std::str::from_utf8(payload).ok()?;
    let mut parts = text.split(';');
    match parts.next()? {
        "A" => Some(Mark::Prompt),
        "C" => Some(Mark::CommandStart),
        "D" => {
            // The status is optional: zsh always sends one, other shells may
            // report only that the command ended.
            let code = parts.next().and_then(|c| c.trim().parse::<i32>().ok());
            Some(Mark::CommandEnd(code))
        }
        // B (input start) and anything else carry nothing we act on.
        _ => None,
    }
}

/// Whether an exit status should light up the error dot.
///
/// 130 and 143 are SIGINT and SIGTERM — the user pressing Ctrl-C is not a
/// failure. Everything else non-zero counts, including the 255 a PHP fatal
/// produces and the 1 that a fruitless `grep` produces; the dot clears as soon
/// as the tab is looked at, which is what keeps the false positives cheap.
pub fn is_failure(code: i32) -> bool {
    !matches!(code, 0 | 130 | 143)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_marks_from_one_chunk() {
        let mut s = MarkScanner::default();
        let marks = s.feed(b"\x1b]133;A\x07hello\x1b]133;C\x07");
        assert_eq!(marks, vec![Mark::Prompt, Mark::CommandStart]);
    }

    #[test]
    fn reads_the_exit_status() {
        let mut s = MarkScanner::default();
        assert_eq!(s.feed(b"\x1b]133;D;255\x07"), vec![Mark::CommandEnd(Some(255))]);
    }

    #[test]
    fn accepts_a_missing_exit_status() {
        let mut s = MarkScanner::default();
        assert_eq!(s.feed(b"\x1b]133;D\x07"), vec![Mark::CommandEnd(None)]);
    }

    #[test]
    fn accepts_the_st_terminator() {
        let mut s = MarkScanner::default();
        assert_eq!(s.feed(b"\x1b]133;C\x1b\\"), vec![Mark::CommandStart]);
    }

    /// A read boundary can fall anywhere, including inside the prefix itself.
    #[test]
    fn survives_a_split_across_chunks() {
        let mut s = MarkScanner::default();
        assert!(s.feed(b"\x1b]13").is_empty());
        assert!(s.feed(b"3;D;1").is_empty());
        assert_eq!(s.feed(b"27\x07"), vec![Mark::CommandEnd(Some(127))]);
    }

    #[test]
    fn ignores_ordinary_output() {
        let mut s = MarkScanner::default();
        assert!(s.feed(b"\x1b[31mred\x1b[0m normal ]133; text").is_empty());
    }

    /// An unterminated sequence must not swallow everything that follows.
    #[test]
    fn abandons_an_overlong_payload() {
        let mut s = MarkScanner::default();
        let mut junk = b"\x1b]133;".to_vec();
        junk.extend(std::iter::repeat_n(b'x', MAX_PAYLOAD + 10));
        assert!(s.feed(&junk).is_empty());
        assert_eq!(s.feed(b"\x1b]133;A\x07"), vec![Mark::Prompt]);
    }

    #[test]
    fn user_interrupts_are_not_failures() {
        assert!(!is_failure(0));
        assert!(!is_failure(130));
        assert!(!is_failure(143));
        assert!(is_failure(1));
        assert!(is_failure(255));
    }
}
