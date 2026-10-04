import { afterEach, describe, expect, test, vi } from 'vitest'
import { createHandler } from './handler'

afterEach(() => vi.restoreAllMocks())

describe('marketing IPC handler', () => {
  test('answers the unlock and locale path', () => {
    const h = createHandler('el')

    expect(h('vault_status')).toBe('unlocked')
    expect(h('settings_get_locale')).toBe('el')
    expect(h('settings_resolve_locale', { systemLanguages: ['fr-FR'] })).toBe('el')
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

  test('answers the default accounts the entry forms ask for', () => {
    const h = createHandler('en')
    const defaults = h('account_defaults', { entityId: 'demo-entity' }) as Record<string, string>
    const ids = new Set((h('account_list') as { id: string }[]).map((a) => a.id))

    expect(Object.keys(defaults).length).toBe(8)
    for (const id of Object.values(defaults)) expect(ids.has(id)).toBe(true)
  })

  test('an unknown command logs and throws, so the capture fails loudly', () => {
    const log = vi.spyOn(console, 'error').mockImplementation(() => {})
    const h = createHandler('en')

    expect(() => h('entry_void', { id: 'x' })).toThrow('marketing fixture has no answer for entry_void')
    expect(log).toHaveBeenCalledWith('[marketing] missing command', 'entry_void')
  })

  test('a renamed required arg logs and throws, so the capture fails loudly', () => {
    const log = vi.spyOn(console, 'error').mockImplementation(() => {})
    const h = createHandler('en')

    expect(() =>
      h('dashboard_summary_cmd', {
        entityId: 'demo-entity',
        from: '2026-09-01',
        to: '2026-09-30',
        assets_as_of: '2026-09-24',
      }),
    ).toThrow('marketing fixture: dashboard_summary_cmd missing arg assetsAsOf')
    expect(log).toHaveBeenCalledWith('[marketing] missing arg', 'dashboard_summary_cmd', 'assetsAsOf')
  })

  test('a non-string optional arg logs and throws, so the capture fails loudly', () => {
    const log = vi.spyOn(console, 'error').mockImplementation(() => {})
    const h = createHandler('en')

    expect(() => h('entry_list', { accountId: 42 })).toThrow('marketing fixture: entry_list missing arg accountId')
    expect(log).toHaveBeenCalledWith('[marketing] missing arg', 'entry_list', 'accountId')
  })
})
