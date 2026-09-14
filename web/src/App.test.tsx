/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'
import type { Entity } from './lib/api'

const vaultLockedHandlers: Array<() => void> = []

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (event: string, handler: () => void) => {
    if (event === 'vault-locked') vaultLockedHandlers.push(handler)
    return () => {}
  }),
}))

vi.mock('./lib/tauri', () => ({
  isTauri: () => true,
  vaultStatus: vi.fn(),
  appInfo: vi.fn(),
  vaultLock: vi.fn(),
  vaultTouch: vi.fn(),
  vaultBackup: vi.fn(),
  vaultRestore: vi.fn(),
  vaultPickBackup: vi.fn(),
  vaultChangePassword: vi.fn(),
  vaultInit: vi.fn(),
  vaultUnlock: vi.fn(),
  updateCheck: vi.fn(),
  updateInstall: vi.fn(),
}))

vi.mock('./lib/api', () => ({
  api: {
    entityList: vi.fn(),
    getLockTimeout: vi.fn(async () => 900),
    setLockTimeout: vi.fn(),
    getLocale: vi.fn(),
    setLocale: vi.fn(),
    getUiPrefs: vi.fn(),
    licenseStatus: vi.fn(async () => ({ state: 'trial', days_remaining: 12 })),
    licenseInstall: vi.fn(),
    eulaText: vi.fn(async () => ''),
  },
}))

vi.mock('./pages/DashboardPage', () => ({
  DashboardPage: ({ onCreateBook }: { onCreateBook?: () => void }) => (
    <div>
      Dashboard stub
      {onCreateBook ? (
        <button type="button" onClick={onCreateBook}>
          Create a book
        </button>
      ) : null}
    </div>
  ),
}))
vi.mock('./pages/TransactionsPage', () => ({
  TransactionsPage: () => null,
}))
vi.mock('./pages/DocumentsPage', () => ({
  DocumentsPage: () => null,
}))
vi.mock('./pages/AccountsPage', () => ({
  AccountsPage: () => null,
}))
vi.mock('./pages/ReportsPage', () => ({
  ReportsPage: () => null,
}))

import { listen } from '@tauri-apps/api/event'
import { appInfo, vaultPickBackup, vaultRestore, vaultStatus, vaultTouch } from './lib/tauri'
import { api } from './lib/api'
import { resetI18nForTests } from './lib/i18n'
import App from './App'

const entity: Entity = {
  id: 'e1',
  name: 'Personal',
  base_currency: 'EUR',
  fiscal_year_start_month: 1,
  chart_template: 'personal',
}

const BACKUP_PATH = '/tmp/backup.oikonomia-backup'

afterEach(() => {
  cleanup()
  resetI18nForTests()
  vaultLockedHandlers.length = 0
})

beforeEach(() => {
  vi.mocked(vaultStatus).mockReset().mockResolvedValue('unlocked')
  vi.mocked(appInfo).mockReset().mockResolvedValue({
    name: 'Oikonomia',
    version: '0.1.0-dev',
    support_email: 'info@ourovoros.io',
  })
  vi.mocked(vaultTouch).mockReset().mockResolvedValue(undefined)
  vi.mocked(vaultPickBackup).mockReset().mockResolvedValue(BACKUP_PATH)
  vi.mocked(vaultRestore).mockReset().mockResolvedValue(BACKUP_PATH)
  vi.mocked(api.entityList).mockReset().mockResolvedValue([entity])
  vi.mocked(listen).mockClear()
})

describe('App restore-while-unlocked', () => {
  test('Settings restore then vault-locked leaves Settings and shows Unlock', async () => {
    render(<App />)
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Settings' })).toBeTruthy()
    })
    await waitFor(() => {
      expect(listen).toHaveBeenCalledWith('vault-locked', expect.any(Function))
    })

    await userEvent.click(screen.getByRole('button', { name: 'Settings' }))
    await userEvent.click(screen.getByRole('button', { name: /vault backup/i }))
    await userEvent.click(screen.getByRole('button', { name: /restore from backup/i }))
    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: 'Replace local vault?' })).toBeTruthy()
    })
    await userEvent.click(screen.getByRole('button', { name: 'Replace vault' }))
    await waitFor(() => {
      expect(vaultRestore).toHaveBeenCalledWith({ path: BACKUP_PATH, replace: true })
    })

    expect(screen.getByRole('heading', { name: 'Settings' })).toBeTruthy()
    for (const handler of vaultLockedHandlers) handler()

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Welcome back' })).toBeTruthy()
    })
    expect(screen.getByRole('button', { name: 'Unlock' })).toBeTruthy()
    expect(screen.queryByRole('heading', { name: 'Settings' })).toBeNull()
  })
})

describe('App create-book intent', () => {
  test('clicking the dashboard empty-state CTA opens Settings and the new-entity form', async () => {
    vi.mocked(api.entityList).mockReset().mockResolvedValue([])
    render(<App />)
    const cta = await screen.findByRole('button', { name: 'Create a book' })
    await userEvent.click(cta)

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Settings' })).toBeTruthy()
    })
    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: 'New entity' })).toBeTruthy()
    })
  })

  test('closing the dialog then navigating away and back to Settings does not reopen it', async () => {
    vi.mocked(api.entityList).mockReset().mockResolvedValue([])
    render(<App />)
    const cta = await screen.findByRole('button', { name: 'Create a book' })
    await userEvent.click(cta)
    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: 'New entity' })).toBeTruthy()
    })

    await userEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    await waitFor(() => {
      expect(screen.queryByRole('dialog', { name: 'New entity' })).toBeNull()
    })

    await userEvent.click(screen.getByRole('button', { name: 'Dashboard' }))
    await waitFor(() => {
      expect(screen.getByText('Dashboard stub')).toBeTruthy()
    })

    await userEvent.click(screen.getByRole('button', { name: 'Settings' }))
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Settings' })).toBeTruthy()
    })
    expect(screen.queryByRole('dialog', { name: 'New entity' })).toBeNull()
  })

  test('a second CTA click still reopens the dialog', async () => {
    vi.mocked(api.entityList).mockReset().mockResolvedValue([])
    render(<App />)
    const cta1 = await screen.findByRole('button', { name: 'Create a book' })
    await userEvent.click(cta1)
    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: 'New entity' })).toBeTruthy()
    })
    await userEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    await waitFor(() => {
      expect(screen.queryByRole('dialog', { name: 'New entity' })).toBeNull()
    })

    await userEvent.click(screen.getByRole('button', { name: 'Dashboard' }))
    await waitFor(() => {
      expect(screen.getByText('Dashboard stub')).toBeTruthy()
    })
    const cta2 = await screen.findByRole('button', { name: 'Create a book' })
    await userEvent.click(cta2)

    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: 'New entity' })).toBeTruthy()
    })
  })
})

describe('App trial banner survives Settings visits', () => {
  test('navigating to Settings does not blank an already-visible trial banner', async () => {
    // App's own fetch (on unlock) resolves with an expiring trial. Settings'
    // own fetch (on mount, when the user navigates there) then fails — this
    // must not blank the banner App already has.
    vi.mocked(api.licenseStatus).mockReset()
    vi.mocked(api.licenseStatus).mockResolvedValueOnce({ state: 'trial', days_remaining: 3 })
    vi.mocked(api.licenseStatus).mockRejectedValueOnce(new Error('network'))

    render(<App />)

    await waitFor(() => {
      expect(screen.getByRole('status')).toHaveTextContent('3 days left in your trial.')
    })

    await userEvent.click(screen.getByRole('button', { name: 'Settings' }))
    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Settings' })).toBeTruthy()
    })

    // Settings' own licenseStatus() call has now rejected (second mocked
    // call). The banner must still show App's originally fetched status.
    expect(screen.getByRole('status')).toHaveTextContent('3 days left in your trial.')
  })
})

describe('App shell', () => {
  test('is dark-only: offers no light or dark mode switch', async () => {
    render(<App />)
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Settings' })).toBeTruthy()
    })

    expect(screen.queryByRole('button', { name: /light mode|dark mode/i })).toBeNull()
  })

  test('switches books from the sidebar list', async () => {
    vi.mocked(api.entityList).mockResolvedValue([entity, { ...entity, id: 'e2', name: 'Household' }])
    render(<App />)

    const household = await screen.findByRole('button', { name: 'Household, EUR' })
    expect(household.getAttribute('aria-pressed')).toBe('false')

    await userEvent.click(household)

    expect(household.getAttribute('aria-pressed')).toBe('true')
    expect(screen.getByRole('button', { name: 'Personal, EUR' }).getAttribute('aria-pressed')).toBe(
      'false',
    )
  })

  test('gives each book its own categorical colour', async () => {
    vi.mocked(api.entityList).mockResolvedValue([entity, { ...entity, id: 'e2', name: 'Household' }])
    render(<App />)

    const personal = await screen.findByRole('button', { name: 'Personal, EUR' })
    const household = await screen.findByRole('button', { name: 'Household, EUR' })

    expect(personal.querySelector('span[aria-hidden]')?.getAttribute('style')).toContain(
      'var(--viz-1)',
    )
    expect(household.querySelector('span[aria-hidden]')?.getAttribute('style')).toContain(
      'var(--viz-2)',
    )
  })
})
