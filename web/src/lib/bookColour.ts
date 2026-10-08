const BOOK_COLOUR_SLOTS = 8

/**
 * A book's identity colour, one of the validated categorical palette slots,
 * derived from the book's id so it does not move when another book is added
 * or the list is reordered. Two books can share a slot; the name beside the
 * dot is what tells them apart.
 */
export function bookDotColour(bookId: string): string {
  let hash = 0
  for (const char of bookId) hash = (hash * 31 + char.charCodeAt(0)) >>> 0
  return `var(--viz-${(hash % BOOK_COLOUR_SLOTS) + 1})`
}
