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

/** Parses `rgb(r, g, b)` / `rgba(r, g, b, a)`; a missing alpha means opaque. */
function rgba(value: string): [number, number, number, number] {
  const match = /rgba?\(\s*([\d.]+)\s*,\s*([\d.]+)\s*,\s*([\d.]+)\s*(?:,\s*([\d.]+)\s*)?\)/.exec(value)
  expect(match, `${value} parses as rgb()/rgba()`).not.toBeNull()
  const [, r, g, b, a] = match ?? []
  return [Number(r), Number(g), Number(b), a === undefined ? 1 : Number(a)]
}

/** Paints a translucent foreground over an opaque background, channel by channel. */
function compositeOver(
  fg: [number, number, number, number],
  bg: [number, number, number],
): [number, number, number] {
  const [r, g, b, a] = fg
  return [a * r + (1 - a) * bg[0], a * g + (1 - a) * bg[1], a * b + (1 - a) * bg[2]]
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

    // --color-danger-text sits on the danger-soft plate (error banners), not
    // bare glass, so its background is that plate composited over the
    // brightest measured glass, not the glass itself.
    const plate = compositeOver(rgba(token('--color-danger-soft')), BRIGHTEST_GLASS)
    expect(
      contrast(rgb(token('--color-danger-text')), plate),
      '--color-danger-text over the danger-soft plate',
    ).toBeGreaterThanOrEqual(4.5)
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

  test('the left-padding reset opts out for adorned controls', () => {
    // An icon-adorned control (e.g. UnlockScreen's password field) keeps its
    // own left padding so the icon does not sit over the typed text; every
    // other field-box reset (height, border, radius, background, shadow)
    // still applies to it.
    const resetRule = css.match(/\.field-box:has\(\.ui-control\)\s*\.ui-control:not\(\[data-adorned\]\)\s*\{([^}]+)\}/)
    expect(resetRule, 'adorned-aware padding reset exists').not.toBeNull()
    expect(resetRule?.[1] ?? '').toContain('padding-left: 0')

    const sharedRule = css.match(/\.field-box:has\(\.ui-control\)\s*\.ui-control\s*\{([^}]+)\}/)
    expect(sharedRule, 'shared control reset exists').not.toBeNull()
    expect(sharedRule?.[1] ?? '').not.toContain('padding-left')
  })

  test('a focused invalid field keeps both the invalid ring and the focus glow', () => {
    // The plain invalid rule and the plain focus-within rule have equal
    // specificity, so whichever comes later in the stylesheet always wins;
    // a focused invalid field needs a rule combining both, not one replacing
    // the other.
    const rule = css.match(
      /\.field-box:has\(\.ui-control\[aria-invalid="true"\]\):focus-within\s*\{([^}]+)\}/,
    )
    expect(rule, 'focused-invalid combined rule exists').not.toBeNull()

    const body = rule?.[1] ?? ''
    expect(body).toContain('inset 0 0 0 1px var(--color-danger)')
    expect(body).toContain('0 0 0 4px rgba(255, 130, 149, 0.14)')
    expect(body).toContain('0 0 24px rgba(55, 213, 255, 0.18)')
  })

  test('focus rings draw inset inside glass panes and dialogs, which clip overflow', () => {
    // The global :focus-visible ring sits 2px outside an element; a pane with
    // overflow-hidden (CollapsibleSection's header button, Panel) or
    // overflow-y-auto (the books ul) cuts that ring off entirely.
    const rule = css.match(
      /\.glass-pane :focus-visible,\s*\.glass-dialog :focus-visible\s*\{([^}]+)\}/,
    )
    expect(rule, 'inset focus-ring rule exists').not.toBeNull()
    expect(rule?.[1] ?? '').toContain('outline-offset: -2px')
  })

  test('the light stops are the spec gradient, shared with the canvas', () => {
    // web/src/lib/cashFlowLight.ts paints with these same six values; its own
    // test pins them there.
    expect(['--color-money-in-a', '--color-money-in-b', '--color-money-in-c'].map((n) => token(n))).toEqual([
      '#27bf93',
      '#1ba39a',
      '#1e8db0',
    ])
    expect(['--color-money-out-a', '--color-money-out-b', '--color-money-out-c'].map((n) => token(n))).toEqual([
      '#f07a45',
      '#e8603f',
      '#d64a5a',
    ])
  })

  test('the label on a chosen entry type clears 4.5:1 across its whole gradient', () => {
    const onIn = rgb(token('--color-on-money-in'))
    const onOut = rgb(token('--color-on-money-out'))

    for (const stop of ['--color-money-in-a', '--color-money-in-b', '--color-money-in-c']) {
      expect(contrast(onIn, rgb(token(stop))), stop).toBeGreaterThanOrEqual(4.5)
    }
    for (const stop of ['--color-money-out-a', '--color-money-out-b', '--color-money-out-c']) {
      expect(contrast(onOut, rgb(token(stop))), stop).toBeGreaterThanOrEqual(4.5)
    }
  })

  test('every stop of the net figure clears 3:1, the large-text threshold', () => {
    for (const name of ['net-figure-in', 'net-figure-out']) {
      const rule = css.match(new RegExp(`\\.${name}\\s*\\{([^}]+)\\}`))
      expect(rule, `${name} rule exists`).not.toBeNull()

      const stops = (rule?.[1] ?? '').match(/#[0-9a-f]{6}/gi) ?? []
      expect(stops.length, `${name} has gradient stops`).toBeGreaterThanOrEqual(2)
      for (const stop of stops) {
        expect(contrast(rgb(stop.toLowerCase()), BRIGHTEST_GLASS), `${name} ${stop}`).toBeGreaterThanOrEqual(3)
      }
    }
  })

  test('money pills keep label and value legible on their plates', () => {
    const inPlate = compositeOver(rgba(token('--color-money-in-soft')), BRIGHTEST_GLASS)
    const outPlate = compositeOver(rgba(token('--color-money-out-soft')), BRIGHTEST_GLASS)
    const neutralPlate = compositeOver(rgba(token('--color-plate-neutral')), BRIGHTEST_GLASS)
    const label = rgb(token('--color-fg-secondary'))

    expect(contrast(label, inPlate), 'label on in').toBeGreaterThanOrEqual(4.5)
    expect(contrast(label, outPlate), 'label on out').toBeGreaterThanOrEqual(4.5)
    expect(contrast(label, neutralPlate), 'label on neutral').toBeGreaterThanOrEqual(4.5)
    expect(contrast(rgb(token('--color-money-in-text')), inPlate), 'in value').toBeGreaterThanOrEqual(4.5)
    expect(contrast(rgb(token('--color-money-out-text')), outPlate), 'out value').toBeGreaterThanOrEqual(4.5)
    expect(contrast(rgb(token('--color-fg')), neutralPlate), 'neutral value').toBeGreaterThanOrEqual(4.5)
  })

  test('the net figure glows in its own Ledger colour, never the brand light', () => {
    for (const [name, expectedRgb] of [
      ['net-figure-in', [27, 163, 154]],
      ['net-figure-out', [232, 96, 63]],
    ] as const) {
      const rule = css.match(new RegExp(`\\.${name}\\s*\\{([^}]+)\\}`))
      expect(rule, `${name} rule exists`).not.toBeNull()

      const body = rule?.[1] ?? ''
      const shadowMatch = /drop-shadow\(\s*[^)]*\s*rgba\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*,\s*[\d.]+\s*\)/.exec(
        body,
      )
      expect(shadowMatch, `${name} has drop-shadow rgba`).not.toBeNull()

      const [, r, g, b] = shadowMatch ?? []
      expect([Number(r), Number(g), Number(b)], `${name} glow color`).toEqual(expectedRgb)
    }
  })
})
