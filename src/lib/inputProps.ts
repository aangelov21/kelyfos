/**
 * Turn off every browser writing aid on a field.
 *
 * Nothing typed into this app is prose: host names, usernames, shell commands,
 * variable names, tags, hex colours. Autocomplete offers stale values the app
 * never stored, and autocorrect/autocapitalise silently rewrite a command or a
 * case-sensitive path into something that no longer works — a class of bug the
 * user cannot see happening. Spread onto text-ish inputs; toggles, colour
 * swatches and range sliders have nothing to correct and are left alone.
 */
export const noAutoAssist = {
  autoComplete: "off",
  autoCorrect: "off",
  autoCapitalize: "off",
  spellCheck: false,
} as const;
