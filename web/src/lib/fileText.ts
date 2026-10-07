/**
 * Text from the user's own file, shown back inside a sentence: the cell that
 * is not a date, the header a mapping names.
 *
 * It is always rendered as text. React escapes what it renders, and no error
 * path sets inner HTML, so a cell that looks like markup is shown as typed.
 * What this module adds is a bound on the length, so one long cell cannot
 * push the rest of the sentence out of view.
 */

/** The parameters of an error or a row problem that hold text from a file. */
export const FILE_TEXT_PARAMS: ReadonlySet<string> = new Set(['value', 'column'])

/** The most characters of a file's text a sentence shows before it is cut. */
export const FILE_TEXT_LIMIT = 60

/**
 * `text` on one line and at most `FILE_TEXT_LIMIT` characters long, with an
 * ellipsis in place of what was cut. Line breaks and other runs of white
 * space become one space. Characters are counted as code points, so a cut
 * never splits a surrogate pair.
 */
export function shortenFileText(text: string): string {
  const characters = Array.from(text.replace(/\s+/g, ' ').trim())

  if (characters.length <= FILE_TEXT_LIMIT) return characters.join('')

  return `${characters.slice(0, FILE_TEXT_LIMIT).join('').trimEnd()}…`
}

/** `params` with every value that came from a file shortened for display. */
export function withShortFileText(params: Record<string, string>): Record<string, string> {
  const shown: Record<string, string> = {}

  for (const [name, value] of Object.entries(params)) {
    shown[name] = FILE_TEXT_PARAMS.has(name) ? shortenFileText(value) : value
  }

  return shown
}
