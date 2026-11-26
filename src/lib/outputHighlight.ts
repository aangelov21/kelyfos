/**
 * Termius-style colouring of plain terminal output: an IP address or a URL in
 * otherwise unformatted text is far easier to pick out coloured than not.
 *
 * This works by rewriting the *stream*, not by drawing over the grid. xterm
 * renders glyphs from the attributes it was handed, and its decoration API can
 * only put a background behind a cell range — there is no way to recolour text
 * already in the buffer. So tokens are wrapped in SGR sequences on the way in.
 *
 * Three rules keep that from fighting anything:
 *
 * - Only text is rewritten. Escape sequences are copied through byte for byte,
 *   which is what keeps OSC 133 marks, title changes, cursor motion and
 *   anything else the app is saying intact.
 * - Only text whose foreground is currently *default* is touched. Whatever a
 *   program coloured itself is left exactly as it sent it — and that also makes
 *   the sequence closing a token exactly `39m`, with no need to reconstruct the
 *   state it interrupted, since the only state it can interrupt is the default.
 * - Nothing is rewritten while the alternate screen is up. A full-screen
 *   program owns its colours; recolouring inside vim or htop would be
 *   surprising and useless. The mode is tracked from the stream rather than
 *   read off the terminal, because the chunk that switches screens must already
 *   be treated as switched by the time its own text is reached.
 *
 * Colours are ANSI indices, not hex, so they come from whatever palette the
 * active theme sets and stay legible when the user changes it.
 *
 * Chunk boundaries are deliberately not stitched: a token split across two PTY
 * reads is simply not coloured. Buffering the tail to catch that would mean
 * holding output back, and a prompt that arrives late because we were waiting
 * for the rest of a word is far worse than an occasional plain address.
 */

const URL_COLOR = "\x1b[36m";
const IP_COLOR = "\x1b[35m";
/** Default foreground. Correct as a closer only because we colour nothing else. */
const DEFAULT_FG = "\x1b[39m";

/**
 * A URL, or four dot-separated groups of up to three digits. The URL arm stops
 * before trailing punctuation so that a sentence's full stop or a wrapping
 * bracket is not swallowed into the link. The address arm is loose on purpose —
 * the octet range is checked in `colorize`, where a rejection is one comparison
 * rather than another regex branch.
 *
 * Bytes above 0x7f end a URL as surely as a space does. They are the
 * continuation bytes of a multi-byte character, and a match that ran into one
 * could end between them — putting our closing sequence *inside* a character
 * and turning it into mojibake. No real URL needs them.
 */
const TOKEN =
  /((?:https?|ftps?|ssh|sftp|git|file):\/\/[^\s'"<>`|\\\x80-\xff]*[^\s'"<>`|\\\x80-\xff.,:;!?)\]}])|\b\d{1,3}(?:\.\d{1,3}){3}\b/g;

function isIpv4(text: string): boolean {
  return text.split(".").every((octet) => Number(octet) <= 255);
}

function colorize(text: string): string {
  return text.replace(TOKEN, (match, url: string | undefined) => {
    if (url !== undefined) return `${URL_COLOR}${match}${DEFAULT_FG}`;
    // A version like 1.2.3.4 matches the same shape as an address; only the
    // octet range separates them, and nothing else here can tell them apart.
    return isIpv4(match) ? `${IP_COLOR}${match}${DEFAULT_FG}` : match;
  });
}

/**
 * Whether the foreground is still the default after an SGR sequence, given what
 * it was before. Only the foreground matters, so bold, background and underline
 * parameters fall through untouched.
 *
 * The extended-colour forms have to be stepped over rather than scanned, or
 * their arguments read as parameters of their own: the `0` in a truecolor
 * `38;2;0;39;100` is not a reset, and the `39` is not a return to default.
 */
function foregroundIsDefault(params: string, wasDefault: boolean): boolean {
  // `ESC[m` means `ESC[0m`.
  if (params === "") return true;
  const parts = params.split(";");
  let isDefault = wasDefault;
  for (let i = 0; i < parts.length; i++) {
    const part = parts[i];
    // Colon-delimited extended colour, which carries its arguments inside the
    // one parameter (`38:2::r:g:b`) and so needs no stepping over.
    if (/^(38|48|58):/.test(part)) {
      if (part.startsWith("38")) isDefault = false;
      continue;
    }
    const n = Number(part);
    if (Number.isNaN(n)) continue;
    if (n === 38 || n === 48 || n === 58) {
      if (n === 38) isDefault = false;
      // 5 takes one argument (palette index), 2 takes three (r, g, b).
      if (parts[i + 1] === "5") i += 2;
      else if (parts[i + 1] === "2") i += 4;
      continue;
    }
    if (n === 0 || n === 39) isDefault = true;
    else if ((n >= 30 && n <= 37) || (n >= 90 && n <= 97)) isDefault = false;
  }
  return isDefault;
}

/** Where the scanner is in an escape sequence between one chunk and the next. */
type Phase = "text" | "esc" | "csi" | "string";

/**
 * A stateful transform over one terminal's output. The state is the escape
 * parser's, which has to survive a chunk ending mid-sequence — no bytes are
 * ever held back, only the knowledge of where we were.
 *
 * Operates on a latin1 string (one character per byte, as `bytesToLatin1` in
 * XtermView produces) so that a multi-byte UTF-8 character split across chunks
 * stays intact: the patterns are ASCII-only, so no continuation byte can ever
 * be matched, and converting back reproduces the original bytes exactly.
 */
export function createOutputHighlighter(): (
  chunk: string,
  enabled: boolean
) => string {
  let phase: Phase = "text";
  let params = "";
  // A string sequence ends at BEL or at ST, which is itself an escape.
  let sawEscInString = false;
  let defaultFg = true;
  let altScreen = false;

  return (chunk, enabled) => {
    let out = "";
    let runStart = 0;

    // Text is colourable; everything else is copied as it arrived.
    const flush = (end: number, isText: boolean) => {
      if (end <= runStart) return;
      const run = chunk.slice(runStart, end);
      out += isText && enabled && defaultFg && !altScreen ? colorize(run) : run;
      runStart = end;
    };

    for (let i = 0; i < chunk.length; i++) {
      const code = chunk.charCodeAt(i);
      switch (phase) {
        case "text":
          if (code === 0x1b) {
            flush(i, true);
            phase = "esc";
          }
          break;

        case "esc":
          if (code === 0x1b) break; // a fresh escape restarts the sequence
          if (code === 0x5b /* [ */) {
            phase = "csi";
            params = "";
          } else if (
            code === 0x5d /* ] OSC */ ||
            code === 0x50 /* P DCS */ ||
            code === 0x5e /* ^ PM  */ ||
            code === 0x5f /* _ APC */
          ) {
            phase = "string";
            sawEscInString = false;
          } else if (code < 0x20 || code > 0x2f) {
            // Anything outside the intermediate range ends the sequence; an
            // intermediate (`ESC ( B`) means another byte is still to come.
            flush(i + 1, false);
            phase = "text";
          }
          break;

        case "csi":
          if (code >= 0x40 && code <= 0x7e) {
            if (code === 0x6d /* m */) {
              defaultFg = foregroundIsDefault(params, defaultFg);
            } else if (
              (code === 0x68 /* h */ || code === 0x6c /* l */) &&
              /^\?(1049|1047|47)$/.test(params)
            ) {
              altScreen = code === 0x68;
            }
            flush(i + 1, false);
            phase = "text";
            // Cap the parameter buffer: a stream that never sends a final byte
            // would otherwise grow it without limit.
          } else if (params.length < 64) {
            params += chunk[i];
          }
          break;

        case "string":
          if (code === 0x07 || (sawEscInString && code === 0x5c /* \ */)) {
            flush(i + 1, false);
            phase = "text";
          } else {
            sawEscInString = code === 0x1b;
          }
          break;
      }
    }

    flush(chunk.length, phase === "text");
    return out;
  };
}
