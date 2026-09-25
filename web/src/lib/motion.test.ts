/** @vitest-environment jsdom */

import { act, renderHook } from '@testing-library/react'
import { afterEach, describe, expect, test, vi } from 'vitest'

import { useMotionAllowed } from './motion'

function setVisibility(state: DocumentVisibilityState) {
  Object.defineProperty(document, 'visibilityState', { value: state, configurable: true })
}

function mockReducedMotion(matches: boolean) {
  Object.defineProperty(window, 'matchMedia', {
    configurable: true,
    value: vi.fn().mockReturnValue({
      matches,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    }),
  })
}

afterEach(() => {
  vi.restoreAllMocks()
  setVisibility('visible')
  Reflect.deleteProperty(window, 'matchMedia')
})

describe('useMotionAllowed', () => {
  test('allows motion in a visible, focused window with no preference', () => {
    vi.spyOn(document, 'hasFocus').mockReturnValue(true)

    const { result } = renderHook(() => useMotionAllowed())

    expect(result.current).toBe(true)
  })

  test('stops when the window loses focus', () => {
    const hasFocus = vi.spyOn(document, 'hasFocus').mockReturnValue(true)
    const { result } = renderHook(() => useMotionAllowed())

    act(() => {
      hasFocus.mockReturnValue(false)
      window.dispatchEvent(new Event('blur'))
    })

    expect(result.current).toBe(false)
  })

  test('stops when the page is hidden', () => {
    vi.spyOn(document, 'hasFocus').mockReturnValue(true)
    const { result } = renderHook(() => useMotionAllowed())

    act(() => {
      setVisibility('hidden')
      document.dispatchEvent(new Event('visibilitychange'))
    })

    expect(result.current).toBe(false)
  })

  test('never moves when the user asks for reduced motion', () => {
    vi.spyOn(document, 'hasFocus').mockReturnValue(true)
    mockReducedMotion(true)

    const { result } = renderHook(() => useMotionAllowed())

    expect(result.current).toBe(false)
  })

  test('survives a webview without matchMedia', () => {
    vi.spyOn(document, 'hasFocus').mockReturnValue(true)

    const { result } = renderHook(() => useMotionAllowed())

    expect(result.current).toBe(true)
  })
})
