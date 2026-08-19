/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'
import type { Account, Entity, PostedEntryView, UiPrefs } from '../lib/api'

vi.mock('../lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/api')>()
  return {
    ...actual,
    api: {
      ...actual.api,
      entityList: vi.fn(),
      accountList: vi.fn(),
      getUiPrefs: vi.fn(),
      entryPostSimple: vi.fn(),
      entrySetHidden: vi.fn(),
      rememberQuickAdd: vi.fn(),
    },
  }
})

import { api } from '../lib/api'
import { resetI18nForTests } from '../lib/i18n'
import { QuickAddPage } from './QuickAddPage'

const entity: Entity = {
  id: 'e1',
  name: 'Personal',
  base_currency: 'EUR',
  fiscal_year_start_month: 1,
  chart_template: 'personal',
}

const accounts: Account[] = [
  {
    id: 'w1',
    entity_id: 'e1',
    code: '1000',
    name: 'Checking',
    account_type: 'asset',
    parent_id: null,
    is_active: true,
    is_system: false,
    sort_order: 0,
  },
  {
    id: 'exp1',
    entity_id: 'e1',
    code: '5000',
    name: 'Meals & dining',
    account_type: 'expense',
    parent_id: null,
    is_active: true,
    is_system: false,
    sort_order: 1,
  },
]

const posted: PostedEntryView = {
  entry: {
    id: 'j1',
    entity_id: 'e1',
    entry_date: '2026-08-12',
    description: 'ATM cash',
    reference: null,
    status: 'posted',
    hidden: false,
  },
  lines: [],
  is_voided: false,
}

const prefs: UiPrefs = {
  theme: 'dark',
  last_entity_id: 'e1',
  last_accounts_by_entity_kind: {},
}

afterEach(() => {
  cleanup()
  resetI18nForTests()
})

beforeEach(() => {
  vi.mocked(api.entityList).mockReset().mockResolvedValue([entity])
  vi.mocked(api.accountList).mockReset().mockResolvedValue(accounts)
  vi.mocked(api.getUiPrefs).mockReset().mockResolvedValue(prefs)
  vi.mocked(api.entryPostSimple).mockReset().mockResolvedValue(posted)
  vi.mocked(api.entrySetHidden).mockReset().mockResolvedValue({
    ...posted,
    entry: { ...posted.entry, hidden: true },
  })
  vi.mocked(api.rememberQuickAdd).mockReset().mockResolvedValue(undefined)
})

const ROLL_MS = 230

async function afterRoll() {
  await new Promise((resolve) => setTimeout(resolve, ROLL_MS + 40))
}

async function reachSaveStep() {
  render(<QuickAddPage onPosted={() => {}} />)
  await waitFor(() => {
    expect(screen.getByRole('radio', { name: 'Expense' })).toBeTruthy()
  })
  await userEvent.click(screen.getByRole('radio', { name: 'Expense' }))
  await waitFor(() => {
    expect(screen.getByLabelText('Amount (EUR)')).toBeTruthy()
  })
  await afterRoll()
  await userEvent.type(screen.getByLabelText('Amount (EUR)'), '12.50')
  await userEvent.click(screen.getByRole('button', { name: 'Next' }))
  await waitFor(() => {
    expect(screen.getByLabelText('Category')).toBeTruthy()
  })
  await afterRoll()
  await userEvent.click(screen.getByRole('button', { name: 'Next' }))
  await waitFor(() => {
    expect(screen.getByRole('button', { name: 'Save' })).toBeTruthy()
  })
}

describe('QuickAddPage hidden paint', () => {
  test('Save with Hide set posts then calls entry_set_hidden', async () => {
    await reachSaveStep()
    const hide = screen.getByRole('checkbox', { name: /hide/i })
    expect(hide).not.toBeChecked()
    expect(screen.getByText('Hidden from export')).toBeTruthy()
    expect(screen.queryByRole('radio', { name: /hide/i })).toBeNull()
    expect(screen.queryByRole('checkbox', { name: /include hidden/i })).toBeNull()
    await userEvent.click(hide)
    await userEvent.click(screen.getByRole('button', { name: 'Save' }))
    await waitFor(() => {
      expect(api.entryPostSimple).toHaveBeenCalledTimes(1)
    })
    expect(api.entrySetHidden).toHaveBeenCalledWith('j1', true)
  })

  test('Save without Hide does not call entry_set_hidden', async () => {
    await reachSaveStep()
    await userEvent.click(screen.getByRole('button', { name: 'Save' }))
    await waitFor(() => {
      expect(api.entryPostSimple).toHaveBeenCalledTimes(1)
    })
    expect(api.entrySetHidden).not.toHaveBeenCalled()
  })
})
