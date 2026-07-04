//! System clipboard integration.
//!
//! glassterm writes the clipboard via two independent paths so a copy works
//! whether or not the outer terminal honors OSC 52:
//!   * `pbcopy` (macOS), spawned on a detached thread so a slow/hung child
//!     never blocks the event loop, and
//!   * an OSC 52 sequence written straight to `/dev/tty`, which the host
//!     terminal (Ghostty) turns into a system clipboard write.
//!
//! Every failure is logged and swallowed; clipboard trouble must never take
//! down the terminal.

use std::{
    fs::OpenOptions,
    io::Write,
    process::{Command, Stdio},
    thread,
};

use tracing::{debug, warn};

/// Standard base64 alphabet (RFC 4648).
const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode bytes as standard (padded) base64.
pub(crate) fn base64_encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(BASE64_ALPHABET[((triple >> 18) & 0x3f) as usize] as char);
        out.push(BASE64_ALPHABET[((triple >> 12) & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            BASE64_ALPHABET[((triple >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            BASE64_ALPHABET[(triple & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Build an OSC 52 clipboard-set sequence for the given text, targeting the
/// `c` (clipboard) selection and terminated with ST (`ESC \`).
pub(crate) fn osc52_set_sequence(text: &str) -> Vec<u8> {
    let encoded = base64_encode(text.as_bytes());
    let mut seq = Vec::with_capacity(encoded.len() + 8);
    seq.extend_from_slice(b"\x1b]52;c;");
    seq.extend_from_slice(encoded.as_bytes());
    seq.extend_from_slice(b"\x1b\\");
    seq
}

/// Write `text` to the system clipboard via both `pbcopy` and outer OSC 52.
///
/// Empty (or already-empty) input is ignored by the caller; this function
/// assumes it has real content. Never blocks the event loop and never panics.
pub(crate) fn write_clipboard(text: &str) {
    write_pbcopy(text);
    write_osc52(text);
}

/// Spawn `pbcopy` on a detached thread and feed it the text on stdin.
fn write_pbcopy(text: &str) {
    let text = text.to_string();
    let spawn = thread::Builder::new()
        .name("glassterm-pbcopy".to_string())
        .spawn(move || {
            match Command::new("pbcopy")
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(mut child) => {
                    if let Some(mut stdin) = child.stdin.take() {
                        if let Err(error) = stdin.write_all(text.as_bytes()) {
                            warn!(%error, "failed to write to pbcopy stdin");
                        }
                    }
                    // Dropping stdin closes it; wait so we don't leave a zombie.
                    let _ = child.wait();
                }
                Err(error) => warn!(%error, "failed to spawn pbcopy"),
            }
        });
    if let Err(error) = spawn {
        warn!(%error, "failed to spawn pbcopy thread");
    }
}

/// Write an OSC 52 clipboard-set sequence to the controlling terminal.
fn write_osc52(text: &str) {
    let seq = osc52_set_sequence(text);
    match OpenOptions::new().write(true).open("/dev/tty") {
        Ok(mut tty) => {
            if let Err(error) = tty.write_all(&seq).and_then(|()| tty.flush()) {
                warn!(%error, "failed to write OSC 52 to /dev/tty");
            } else {
                debug!(bytes = seq.len(), "wrote OSC 52 clipboard sequence");
            }
        }
        Err(error) => warn!(%error, "failed to open /dev/tty for OSC 52"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_encodes_known_vectors() {
        // RFC 4648 test vectors.
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64_encodes_high_bytes() {
        assert_eq!(base64_encode(&[0xff, 0xff, 0xff]), "////");
        assert_eq!(base64_encode(&[0x00, 0x00, 0x00]), "AAAA");
        assert_eq!(base64_encode("héllo".as_bytes()), "aMOpbGxv");
    }

    #[test]
    fn osc52_sequence_wraps_base64_payload() {
        let seq = osc52_set_sequence("foobar");
        assert_eq!(seq, b"\x1b]52;c;Zm9vYmFy\x1b\\");
    }

    #[test]
    fn osc52_sequence_starts_and_ends_with_expected_delimiters() {
        let seq = osc52_set_sequence("hi");
        assert!(seq.starts_with(b"\x1b]52;c;"));
        assert!(seq.ends_with(b"\x1b\\"));
        assert_eq!(&seq[b"\x1b]52;c;".len()..seq.len() - 2], b"aGk=");
    }
}
