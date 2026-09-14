import { existsSync, readFileSync } from 'node:fs'
import { describe, expect, test } from 'vitest'

// These invariants read CSS from disk: Vitest turns .css imports into empty
// strings, so importing the files would silently test nothing.
const indexCss = readFileSync(new URL('../src/index.css', import.meta.url), 'utf8')
const fontsCssUrl = new URL('../src/fonts.css', import.meta.url)
const fontsDir = new URL('../public/fonts/', import.meta.url)

function fontsCss(): string {
  return readFileSync(fontsCssUrl, 'utf8')
}

function stack(token: string): string[] {
  const match = new RegExp(`${token}:\\s*([^;]+);`).exec(indexCss)
  expect(match, `${token} is declared in index.css`).not.toBeNull()

  return (match?.[1] ?? '')
    .split(',')
    .map((family) => family.trim().replace(/"/g, ''))
}

describe('bundled typefaces', () => {
  test('every face fonts.css declares ships in public/fonts', () => {
    const files = [...fontsCss().matchAll(/url\("\/fonts\/([^"]+)"\)/g)].map((m) => m[1])

    expect(files.length).toBeGreaterThan(0)
    for (const file of files) {
      expect(existsSync(new URL(file, fontsDir)), file).toBe(true)
    }
  })

  test('Latin faces fall back to Greek-capable faces, in order', () => {
    // Barlow and IBM Plex Mono carry no Greek glyphs; the el locale needs them.
    const sans = stack('--font-sans')
    const mono = stack('--font-mono')

    expect(sans.indexOf('Barlow')).toBe(0)
    expect(sans.indexOf('Sofia Sans')).toBe(1)
    expect(mono.indexOf('IBM Plex Mono')).toBe(0)
    expect(mono.indexOf('JetBrains Mono')).toBe(1)
  })

  test('the Greek fallbacks really carry Greek', () => {
    const faces = fontsCss().split('@font-face')

    for (const family of ['Sofia Sans', 'JetBrains Mono']) {
      const greek = faces.some((face) => face.includes(`"${family}"`) && face.includes('U+0370-0377'))
      expect(greek, family).toBe(true)
    }
  })

  test('each bundled family ships its SIL OFL text', () => {
    const licences = ['OFL-Barlow.txt', 'OFL-IBMPlexMono.txt', 'OFL-SofiaSans.txt', 'OFL-JetBrainsMono.txt']

    for (const licence of licences) {
      expect(existsSync(new URL(licence, fontsDir)), licence).toBe(true)
    }
  })

  test('nothing is fetched from a font service at runtime', () => {
    expect(indexCss + fontsCss()).not.toMatch(/fonts\.(googleapis|gstatic)\.com|@fontsource/)
  })
})
