/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { act, cleanup, render, screen, within } from '@testing-library/react'
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
      accountSetOpeningBalance: vi.fn(),
    },
  }
})

import type { Account, Entity } from '../lib/api'
import { api } from '../lib/api'
import { resetI18nForTests, setLocale, t } from '../lib/i18n'
import { formatMoney } from '../lib/money'
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

describe('AccountsPage notices', () => {
  test('a language switch re-words the notice for an added account', async () => {
    vi.mocked(api.accountList).mockResolvedValue([checking])
    vi.mocked(api.accountCreate).mockResolvedValue(checking)
    render(<AccountsPage entity={entity} />)
    await screen.findByText('Checking')

    await userEvent.click(screen.getByRole('button', { name: 'Add account' }))
    await userEvent.type(screen.getByLabelText('Code'), '1010')
    await userEvent.type(screen.getByLabelText('Name'), 'Savings')
    await userEvent.click(screen.getByRole('button', { name: 'Create account' }))
    const english = 'Account 1010 · Savings added.'
    expect(await screen.findByRole('status')).toHaveTextContent(english)

    act(() => setLocale('el'))

    const greek = t('accounts.form.created', { code: '1010', name: 'Savings' })
    expect(greek).not.toBe(english)
    expect(screen.getByRole('status')).toHaveTextContent(greek)
  })

  test('a language switch re-formats the amount in a saved-balance notice', async () => {
    const euro = { code: 'EUR', decimals: 2 }
    vi.mocked(api.accountList).mockResolvedValue([checking])
    vi.mocked(api.accountBalance).mockResolvedValue(0)
    // The page ignores the posted entry, as in AccountsPage.test.tsx.
    vi.mocked(api.accountSetOpeningBalance).mockResolvedValue({} as never)
    render(<AccountsPage entity={entity} />)
    await screen.findByText('Checking')

    await userEvent.click(screen.getByRole('button', { name: 'Set balance for Checking' }))
    const dialog = await screen.findByRole('dialog')
    const amount = within(dialog).getByRole('textbox', { name: /actual balance/i })
    await userEvent.type(amount, '1234.50{Enter}')
    const english = formatMoney(123450, euro, 'en')
    expect((await screen.findByRole('status')).textContent).toContain(english)

    act(() => setLocale('de'))

    const german = formatMoney(123450, euro, 'de')
    expect(german).not.toBe(english)
    expect(screen.getByRole('status').textContent).toContain(german)
  })
})
