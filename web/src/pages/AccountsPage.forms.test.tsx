/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, test, vi } from 'vitest'

vi.mock('../lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/api')>()
  return {
    ...actual,
    api: {
      ...actual.api,
      accountList: vi.fn(),
      accountCreate: vi.fn(),
      accountBalance: vi.fn(),
    },
  }
})

import type { Account, Entity } from '../lib/api'
import { api } from '../lib/api'
import { resetI18nForTests } from '../lib/i18n'
import { AccountsPage } from './AccountsPage'

afterEach(() => {
  cleanup()
  resetI18nForTests()
  vi.resetAllMocks()
})

const entity: Entity = {
  id: 'e1',
  name: 'Personal',
  base_currency: 'EUR',
  base_currency_decimals: 2,
  fiscal_year_start_month: 1,
  chart_template: 'personal',
}

const checking: Account = {
  id: 'w1',
  entity_id: 'e1',
  code: '1000',
  name: 'Checking',
  account_type: 'asset',
  is_active: true,
  is_system: false,
  sort_order: 0,
}

describe('AccountsPage form errors', () => {
  test('a refused account is explained inside the Add account card and cleared when it closes', async () => {
    vi.mocked(api.accountList).mockResolvedValue([checking])
    vi.mocked(api.accountCreate).mockRejectedValue({
      code: 'account_code_taken',
      message: 'code taken',
      params: { code: '1000' },
    })
    render(<AccountsPage entity={entity} />)
    await screen.findByText('Checking')

    await userEvent.click(screen.getByRole('button', { name: 'Add account' }))
    const form = screen.getByLabelText('Code').closest('form')
    await userEvent.type(screen.getByLabelText('Code'), '1000')
    await userEvent.type(screen.getByLabelText('Name'), 'Dup')
    await userEvent.click(screen.getByRole('button', { name: 'Create account' }))

    const alert = await screen.findByRole('alert')
    expect(form).toContainElement(alert)

    await userEvent.click(screen.getByRole('button', { name: 'Close' }))
    await userEvent.click(screen.getByRole('button', { name: 'Add account' }))
    expect(screen.queryByRole('alert')).toBeNull()
  })

  test('a bad balance is explained inside the Set balance dialog, which starts on the amount', async () => {
    vi.mocked(api.accountList).mockResolvedValue([checking])
    vi.mocked(api.accountBalance).mockResolvedValue(0)
    render(<AccountsPage entity={entity} />)
    await screen.findByText('Checking')

    await userEvent.click(screen.getByRole('button', { name: 'Set balance for Checking' }))
    const dialog = await screen.findByRole('dialog')
    const amount = within(dialog).getByRole('textbox', { name: /actual balance/i })
    expect(amount).toHaveFocus()

    await userEvent.type(amount, 'abc{Enter}')

    expect(within(dialog).getByRole('alert')).toHaveTextContent('Enter a valid amount')
  })

  test('Set balance is a labelled button, not an icon alone', async () => {
    vi.mocked(api.accountList).mockResolvedValue([checking])
    render(<AccountsPage entity={entity} />)
    await screen.findByText('Checking')

    expect(screen.getByRole('button', { name: 'Set balance for Checking' })).toHaveTextContent(
      'Set balance',
    )
  })
})
