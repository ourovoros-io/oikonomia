import { afterEach, describe, expect, test, vi } from 'vitest'
import { createHandler } from './handler'

afterEach(() => vi.restoreAllMocks())

describe('marketing IPC handler', () => {
  test('answers the unlock and locale path', () => {
    const h = createHandler('el')

    expect(h('vault_status')).toBe('unlocked')
    expect(h('settings_get_locale')).toBe('el')
    expect(h('license_status')).toEqual({ state: 'licensed', licensed_until: '2027-09-24' })
  })

  test('answers dashboard reads with consistent data', () => {
    const h = createHandler('en')
    const summary = h('dashboard_summary_cmd', {
      entityId: 'demo-entity',
      from: '2026-09-01',
      to: '2026-09-30',
      assetsAsOf: '2026-09-24',
    }) as { income: number }

    expect(summary.income).toBeGreaterThan(0)
    expect((h('entity_list') as unknown[]).length).toBe(1)
  })

  test('an unknown command logs and throws, so the capture fails loudly', () => {
    const log = vi.spyOn(console, 'error').mockImplementation(() => {})
    const h = createHandler('en')

    expect(() => h('entry_void', { id: 'x' })).toThrow('marketing fixture has no answer for entry_void')
    expect(log).toHaveBeenCalledWith('[marketing] missing command', 'entry_void')
  })
})
