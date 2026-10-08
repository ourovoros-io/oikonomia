/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, test } from 'vitest'

import { ErrorBanner, Notice, ToastStack } from './ui'

afterEach(() => {
  cleanup()
})

/** The rules of one selector, from the unlayered block that styles it. */
function rule(css: string, selector: string): string {
  const start = css.indexOf(`\n${selector} {`)
  expect(start, `${selector} rule`).toBeGreaterThan(-1)

  return css.slice(start, css.indexOf('}', start))
}

describe('ToastStack', () => {
  test('keeps errors assertive and confirmations polite', () => {
    render(
      <ToastStack>
        <ErrorBanner className="" message="Could not save." />
        <Notice message="Saved." onDismiss={() => undefined} />
      </ToastStack>,
    )

    expect(screen.getByRole('alert')).toHaveAttribute('aria-live', 'assertive')
    expect(screen.getByRole('status')).toHaveAttribute('aria-live', 'polite')
  })

  test('floats its banners in the stack and leaves no visible wrapper for a quiet one', () => {
    const { container } = render(
      <ToastStack>
        <ErrorBanner className="" message={null} />
        {null}
        <Notice message="Saved." onDismiss={() => undefined} />
      </ToastStack>,
    )

    const stack = container.querySelector('.oik-toast-stack')
    expect(stack).not.toBeNull()
    expect(stack).toContainElement(screen.getByRole('status'))

    // CSS hides an empty wrapper (.oik-toast:empty); here, only the Notice has content.
    const filled = [...container.querySelectorAll('.oik-toast')].filter((el) => !el.matches(':empty'))
    expect(filled).toHaveLength(1)
  })

  test('takes no room in the layout and respects reduced motion', () => {
    const css = readFileSync(join(process.cwd(), 'src', 'index.css'), 'utf8')

    const stack = rule(css, '.oik-toast-stack')
    expect(stack).toMatch(/position:\s*sticky/)
    expect(stack).toMatch(/height:\s*0/)
    expect(stack).toMatch(/top:\s*16px/)

    const reduced = css.slice(css.lastIndexOf('@media (prefers-reduced-motion: reduce)'))
    expect(reduced).toMatch(/\.oik-toast\s*\{\s*animation:\s*none/)
  })
})
