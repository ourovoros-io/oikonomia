// @vitest-environment jsdom
import { describe, expect, test } from 'vitest'
import { tabStops, trapTab, wrapTarget } from './focusTrap'

function panel(): { root: HTMLElement; a: HTMLElement; b: HTMLElement; c: HTMLElement } {
  document.body.innerHTML = `
    <div id="root">
      <button id="a">a</button>
      <input id="b" />
      <button id="gone" disabled>off</button>
      <button id="c">c</button>
    </div>`
  const get = (id: string) => document.getElementById(id) as HTMLElement
  return { root: get('root'), a: get('a'), b: get('b'), c: get('c') }
}

describe('focus trap', () => {
  test('lists the enabled controls only', () => {
    const { root, a, b, c } = panel()
    expect(tabStops(root)).toEqual([a, b, c])
  })

  test('Tab on the last control wraps to the first', () => {
    const { root, a, c } = panel()
    expect(wrapTarget(tabStops(root), c, false)).toBe(a)
  })

  test('Shift+Tab on the first control wraps to the last', () => {
    const { root, a, c } = panel()
    expect(wrapTarget(tabStops(root), a, true)).toBe(c)
  })

  test('a Tab in the middle is left to the browser', () => {
    const { root, b } = panel()
    expect(wrapTarget(tabStops(root), b, false)).toBeNull()
    expect(wrapTarget(tabStops(root), b, true)).toBeNull()
  })

  test('focus on the body is pulled back inside', () => {
    const { root, a, c } = panel()
    expect(wrapTarget(tabStops(root), document.body, false)).toBe(a)
    expect(wrapTarget(tabStops(root), null, true)).toBe(c)
  })

  test('an empty panel has nowhere to wrap to', () => {
    expect(wrapTarget([], null, false)).toBeNull()
  })

  test('trapTab moves focus and cancels the browser move', () => {
    const { root, a, c } = panel()
    c.focus()
    const event = new KeyboardEvent('keydown', { key: 'Tab', cancelable: true })

    trapTab(root, event)

    expect(document.activeElement).toBe(a)
    expect(event.defaultPrevented).toBe(true)
  })

  test('trapTab ignores other keys', () => {
    const { root, c } = panel()
    c.focus()
    const event = new KeyboardEvent('keydown', { key: 'Enter', cancelable: true })

    trapTab(root, event)

    expect(document.activeElement).toBe(c)
    expect(event.defaultPrevented).toBe(false)
  })
})
