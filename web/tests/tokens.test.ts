import { readFileSync } from 'node:fs'
import { describe, expect, test } from 'vitest'

// Read from disk: Vitest turns .css imports into empty strings.
const css = readFileSync(new URL('../src/index.css', import.meta.url), 'utf8')

/**
 * The lightest backdrop pixel any pane text can sit on: the 99th percentile
 * under every pane, measured across five points of the aurora's drift at a
 * pane fill of 0.5 (see the spec's "Measured, not assumed" note). If the fill
 * or the aurora changes, re-measure before touching this number.
 */
const BRIGHTEST_GLASS: [number, number, number] = [8, 66, 48]

function token(name: string): string {
  const match = new RegExp(`${name}:\\s*([^;]+);`).exec(css)
  expect(match, `${name} is declared`).not.toBeNull()

  return (match?.[1] ?? '').trim().toLowerCase()
}

function rgb(hex: string): [number, number, number] {
  return [1, 3, 5].map((i) => Number.parseInt(hex.slice(i, i + 2), 16)) as [number, number, number]
}

function luminance([r, g, b]: [number, number, number]): number {
  const linear = (channel: number) => {
    const c = channel / 255
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4
  }

  return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

function contrast(a: [number, number, number], b: [number, number, number]): number {
  const [light, dark] = [luminance(a), luminance(b)].sort((x, y) => y - x)
  return (light + 0.05) / (dark + 0.05)
}

describe('Aurora glass tokens', () => {
  test('money wears the validated Ledger pair', () => {
    // Validated with the dataviz six checks against #11141b: deutan dE 13.8.
    // Changing either side means re-running the validator.
    expect(token('--color-money-in')).toBe('#1ba39a')
    expect(token('--color-money-out')).toBe('#e8603f')
  })

  test('chrome wears the brand light', () => {
    expect(token('--color-accent')).toBe('#2ee6a6')
    expect(token('--color-accent-b')).toBe('#37d5ff')
    expect(token('--color-accent-c')).toBe('#7a8cff')
  })

  test('the glass fill is the one the contrast was measured at', () => {
    expect(token('--glass-fill')).toBe('rgba(16, 18, 24, 0.5)')
  })

  test('text on glass clears 4.5:1 over the brightest measured backdrop', () => {
    const textTokens = [
      '--color-fg',
      '--color-fg-secondary',
      '--color-muted',
      '--color-money-in-text',
      '--color-money-out-text',
      '--color-danger',
    ]

    for (const name of textTokens) {
      expect(contrast(rgb(token(name)), BRIGHTEST_GLASS), name).toBeGreaterThanOrEqual(4.5)
    }
  })

  test('the dim tier is for icons: it clears 3:1 and nothing more is promised', () => {
    expect(contrast(rgb(token('--color-dim')), BRIGHTEST_GLASS)).toBeGreaterThanOrEqual(3)
  })

  test('labels on filled buttons clear 4.5:1', () => {
    const onAccent = rgb(token('--color-on-accent'))
    const white: [number, number, number] = [255, 255, 255]

    expect(contrast(onAccent, rgb(token('--color-accent')))).toBeGreaterThanOrEqual(4.5)
    expect(contrast(onAccent, rgb(token('--color-accent-b')))).toBeGreaterThanOrEqual(4.5)
    expect(contrast(white, rgb(token('--color-danger-fill-a')))).toBeGreaterThanOrEqual(4.5)
    expect(contrast(white, rgb(token('--color-danger-fill-b')))).toBeGreaterThanOrEqual(4.5)
  })

  test('the Reports categorical palette is unchanged', () => {
    const expected = ['#3987e5', '#d95926', '#199e70', '#c98500', '#d55181', '#008300', '#9085e9', '#e66767']

    expected.forEach((hex, index) => {
      expect(token(`--viz-${index + 1}`)).toBe(hex)
    })
  })

  test('the stylesheet is dark-only', () => {
    expect(css).not.toMatch(/prefers-color-scheme|html:not\(\.dark\)|@custom-variant dark/)
  })

  test('the invalid field ring uses the text-safe danger colour', () => {
    const invalidRule = css.match(/\.field-box:has\(\.ui-control\[aria-invalid="true"\]\)\s*\{([^}]+)\}/)
    expect(invalidRule, 'invalid field rule exists').not.toBeNull()

    const ruleBody = invalidRule?.[1] ?? ''
    expect(ruleBody).toContain('inset 0 0 0 1px var(--color-danger)')
    expect(ruleBody).not.toContain('rgba(255, 77, 103')
  })
})
