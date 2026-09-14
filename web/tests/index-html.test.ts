import { readFileSync } from 'node:fs'
import { describe, expect, test } from 'vitest'

// Read from disk: index.html is never imported by the app bundle under test.
const html = readFileSync(new URL('../index.html', import.meta.url), 'utf8')

describe('index.html', () => {
  test('is dark-only: no leftover .dark class and a single-value color-scheme', () => {
    // The app dropped the theme toggle entirely; a stray class="dark" or a
    // "dark light" color-scheme both imply a light mode still exists.
    expect(html).toMatch(/<html lang="en">/)
    expect(html).not.toMatch(/<html[^>]*class="dark"/)
    expect(html).toMatch(/<meta name="color-scheme" content="dark" \/>/)
  })
})
