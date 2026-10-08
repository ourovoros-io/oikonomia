/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'
import type { Account, AccountDefaults, Entity, UiPrefs } from '../lib/api'

vi.mock('../lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/api')>()
  return {
    ...actual,
    api: {
      ...actual.api,
      entityList: vi.fn(),
      accountList: vi.fn(),
      accountDefaults: vi.fn(),
      getUiPrefs: vi.fn(),
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
  base_currency_decimals: 2,
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
    is_active: true,
    is_system: false,
    sort_order: 0,
  },
]

const prefs: UiPrefs = { last_entity_id: 'e1', last_accounts_by_entity_kind: {} }

const defaults: AccountDefaults = {
  category: null,
  payment: 'w1',
  deposit: 'w1',
  income: null,
  bill_category: null,
  bills_payable: null,
  transfer_source: 'w1',
  transfer_destination: 'w1',
}

afterEach(() => {
  cleanup()
  resetI18nForTests()
})

beforeEach(() => {
  vi.mocked(api.entityList).mockReset().mockResolvedValue([entity])
  vi.mocked(api.accountList).mockReset().mockResolvedValue(accounts)
  vi.mocked(api.accountDefaults).mockReset().mockResolvedValue(defaults)
  vi.mocked(api.getUiPrefs).mockReset().mockResolvedValue(prefs)
})

async function openAmountStep() {
  render(<QuickAddPage onPosted={() => {}} />)
  await userEvent.click(await screen.findByRole('radio', { name: 'Expense' }))
  return screen.findByLabelText('Amount (EUR)')
}

describe('QuickAddPage amount errors', () => {
  test('an unreadable amount keeps its message until the value changes', async () => {
    const amount = await openAmountStep()

    await userEvent.type(amount, 'abc{Enter}')

    expect(await screen.findByRole('alert')).toHaveTextContent('Invalid amount')
    expect(amount).toHaveAttribute('aria-invalid', 'true')

    await new Promise((resolve) => setTimeout(resolve, 400))
    expect(screen.getByRole('alert')).toBeInTheDocument()

    await userEvent.type(amount, '1')
    await waitFor(() => expect(screen.queryByRole('alert')).toBeNull())
  })

  test('zero says the amount must be greater than zero, not that it is unreadable', async () => {
    const amount = await openAmountStep()

    await userEvent.type(amount, '0{Enter}')

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'The amount must be greater than zero.',
    )
  })
})
