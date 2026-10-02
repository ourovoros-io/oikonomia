/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'

vi.mock('../lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/api')>()
  return {
    ...actual,
    api: {
      ...actual.api,
      dashboardSummary: vi.fn(),
      cashFlowSeries: vi.fn(),
      entryList: vi.fn(),
      accountList: vi.fn(),
      accountDefaults: vi.fn(),
      documentList: vi.fn(),
      recurringList: vi.fn(),
    },
  }
})

// jsdom has no 2D canvas; the light's painting is tested on its own.
vi.mock('../components/CashFlowPulse', () => ({
  CashFlowPulse: ({ label }: { label: string }) => <div role="img" aria-label={label} />,
}))

import type { AccountDefaults, Entity } from '../lib/api'
import { api } from '../lib/api'
import { resetI18nForTests } from '../lib/i18n'
import { DashboardPage } from './DashboardPage'
import { DocumentsPage } from './DocumentsPage'
import { RecurringPage } from './RecurringPage'

const RAW = 'sqlcipher: disk image is malformed'

const entity: Entity = {
  id: 'e1',
  name: 'Personal',
  base_currency: 'EUR',
  fiscal_year_start_month: 1,
  chart_template: 'personal',
}

beforeEach(() => {
  vi.spyOn(console, 'warn').mockImplementation(() => undefined)
  vi.mocked(api.dashboardSummary).mockReset().mockResolvedValue(undefined as never)
  vi.mocked(api.cashFlowSeries).mockReset().mockResolvedValue(undefined as never)
  vi.mocked(api.entryList).mockReset().mockResolvedValue([])
  vi.mocked(api.accountList).mockReset().mockResolvedValue([])
  vi.mocked(api.accountDefaults).mockReset().mockResolvedValue(DEFAULTS)
  vi.mocked(api.documentList).mockReset().mockResolvedValue([])
  vi.mocked(api.recurringList).mockReset().mockResolvedValue([])
})

const DEFAULTS: AccountDefaults = {
  category: null,
  payment: null,
  deposit: null,
  income: null,
  bill_category: null,
  bills_payable: null,
  transfer_source: null,
  transfer_destination: null,
}

afterEach(() => {
  cleanup()
  resetI18nForTests()
  vi.restoreAllMocks()
})

describe('a failed page load', () => {
  test('Dashboard shows the localized copy for the code, not the raw message', async () => {
    vi.mocked(api.dashboardSummary).mockRejectedValue({ code: 'vault_locked', message: RAW })

    render(<DashboardPage entity={entity} />)

    expect(await screen.findByText('The vault is locked.')).toBeInTheDocument()
    expect(screen.queryByText(RAW)).toBeNull()
  })

  test('Dashboard shows the generic copy for an unknown code, not the raw message', async () => {
    vi.mocked(api.dashboardSummary).mockRejectedValue({ code: 'brand_new', message: RAW })

    render(<DashboardPage entity={entity} />)

    expect(await screen.findByText('Something went wrong.')).toBeInTheDocument()
    expect(screen.queryByText(RAW)).toBeNull()
  })

  test('Documents shows the generic copy for an unknown code, not the raw message', async () => {
    vi.mocked(api.documentList).mockRejectedValue({ code: 'brand_new', message: RAW })

    render(<DocumentsPage entity={entity} />)

    expect(await screen.findByText('Something went wrong.')).toBeInTheDocument()
    expect(screen.queryByText(RAW)).toBeNull()
  })

  test('Recurring shows the generic copy for an unknown code, not the raw message', async () => {
    vi.mocked(api.recurringList).mockRejectedValue({ code: 'brand_new', message: RAW })

    render(<RecurringPage entity={entity} onBack={vi.fn()} />)

    expect(await screen.findByText('Something went wrong.')).toBeInTheDocument()
    expect(screen.queryByText(RAW)).toBeNull()
  })
})
