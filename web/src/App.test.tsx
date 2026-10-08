/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen, waitFor, within } from '@testing-library/react'
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
    entityListArchived: vi.fn(),
    entityArchive: vi.fn(),
    entityCreate: vi.fn(),
    getLockTimeout: vi.fn(async () => 900),
    donationAddresses: vi.fn(async () => []),
    setLockTimeout: vi.fn(),
    getLocale: vi.fn(),
    setLocale: vi.fn(),
    getUiPrefs: vi.fn(),
  },
}))

vi.mock('./pages/DashboardPage', async () => {
  const { TopBar } = await import('./components/TopBar')
  return {
    DashboardPage: ({ onCreateBook }: { onCreateBook?: () => void }) => (
      <div>
        <TopBar title="Dashboard stub title" actions={<button type="button">Stub period</button>} />
        Dashboard stub
        {onCreateBook ? (
          <button type="button" onClick={onCreateBook}>
            Create a book
          </button>
        ) : null}
      </div>
    ),
  }
})
vi.mock('./pages/TransactionsPage', () => ({
  TransactionsPage: ({ newEntryIntent }: { newEntryIntent?: number }) => (
    <div>{`Transactions stub, new entry intent ${newEntryIntent ?? 0}`}</div>
  ),
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
  base_currency_decimals: 2,
  fiscal_year_start_month: 1,
  chart_template: 'personal',
}

const BACKUP_PATH = '/tmp/backup.oikonomia-backup'

afterEach(() => {
  cleanup()
  resetI18nForTests()
  vaultLockedHandlers.length = 0
  vi.restoreAllMocks()
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
  vi.mocked(api.entityListArchived).mockReset().mockResolvedValue([])
  vi.mocked(api.entityArchive).mockReset().mockResolvedValue(undefined)
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

describe('App shell', () => {
  test('a backend failure renders through the shared ErrorBanner (role=alert)', async () => {
    // The app-level error box used to be hand-copied markup with no
    // role/aria-live and a stale hex edge; it now reuses ErrorBanner like
    // every other error surface in the app.
    vi.mocked(api.entityList)
      .mockReset()
      .mockRejectedValue({ code: 'vault_locked', message: 'backend down' })
    render(<App />)
    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('The vault is locked.')
    })
    expect(screen.queryByText('backend down')).toBeNull()
  })

  test('an unknown backend failure shows the localized fallback, never the raw message', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    vi.mocked(api.entityList)
      .mockReset()
      .mockRejectedValue({ code: 'brand_new', message: 'sqlcipher: disk image is malformed' })
    render(<App />)
    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent(
        'Something went wrong while talking to the app. Restart Oikonomia and try again.',
      )
    })
    expect(screen.queryByText(/sqlcipher/)).toBeNull()
  })

  test('a failed book refresh after creating a book shows the backend-failure sentence', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    vi.mocked(api.entityCreate).mockReset().mockResolvedValue({ ...entity, id: 'e2', name: 'Work' })
    render(<App />)
    await userEvent.click(await screen.findByRole('button', { name: 'Settings' }))
    await userEvent.click(screen.getByRole('button', { name: /entities/i }))
    await userEvent.click(await screen.findByRole('button', { name: /new entity/i }))
    await userEvent.type(screen.getByLabelText('Name'), 'Work')

    // Created fine, but reloading the list fails with a code the UI has no copy for.
    vi.mocked(api.entityList).mockRejectedValue({ code: 'brand_new', message: 'sqlcipher: raw detail' })
    await userEvent.click(screen.getByRole('button', { name: /create entity/i }))

    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent(
        'Something went wrong while talking to the app. Restart Oikonomia and try again.',
      )
    })
    expect(screen.queryByText(/sqlcipher/)).toBeNull()
  })

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

    // The books list is a single choice, not a set of toggles, so the
    // selected book is marked with aria-current, not aria-pressed.
    const household = await screen.findByRole('button', { name: 'Household, EUR' })
    expect(household.getAttribute('aria-current')).toBeNull()
    expect(household).toHaveAttribute('title', 'Household, EUR')

    await userEvent.click(household)

    expect(household.getAttribute('aria-current')).toBe('true')
    expect(
      screen.getByRole('button', { name: 'Personal, EUR' }).getAttribute('aria-current'),
    ).toBeNull()
  })

  test('Lock is a button with its own word, and has no native tooltip to linger on the lock screen', async () => {
    render(<App />)
    const lock = await screen.findByRole('button', { name: 'Lock vault' })

    expect(lock).toHaveTextContent('Lock')
    expect(lock).not.toHaveAttribute('title')
  })

  test('mounts exactly one aurora, above the shell', async () => {
    render(<App />)
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Settings' })).toBeTruthy()
    })

    expect(document.querySelectorAll('.aurora')).toHaveLength(1)
  })

  test('gives each book its own categorical colour', async () => {
    vi.mocked(api.entityList).mockResolvedValue([entity, { ...entity, id: 'e2', name: 'Household' }])
    render(<App />)

    const personal = await screen.findByRole('button', { name: 'Personal, EUR' })
    const household = await screen.findByRole('button', { name: 'Household, EUR' })

    expect(personal.querySelector('span[style]')?.getAttribute('style')).toContain(
      'var(--viz-1)',
    )
    expect(household.querySelector('span[style]')?.getAttribute('style')).toContain(
      'var(--viz-2)',
    )
  })
})

describe('App top bar and Quick add', () => {
  test('a page that owns the top bar replaces the book title with its own', async () => {
    render(<App />)
    const banner = await screen.findByRole('banner')

    expect(await within(banner).findByRole('heading', { name: 'Dashboard stub title' })).toBeTruthy()
    expect(within(banner).getByRole('button', { name: 'Stub period' })).toBeTruthy()
    expect(within(banner).queryByText('Personal')).toBeNull()
  })

  test('a page that does not own it keeps the book title', async () => {
    render(<App />)
    await userEvent.click(await screen.findByRole('button', { name: 'Accounts' }))

    const banner = screen.getByRole('banner')
    expect(await within(banner).findByText('Personal')).toBeTruthy()
    expect(within(banner).queryByRole('heading', { name: 'Dashboard stub title' })).toBeNull()
  })

  test('Quick add opens New entry on Transactions', async () => {
    render(<App />)
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Quick add' })).toBeEnabled()
    })

    await userEvent.click(screen.getByRole('button', { name: 'Quick add' }))

    expect(await screen.findByText('Transactions stub, new entry intent 1')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Transactions' })).toHaveAttribute('aria-current', 'page')
  })

  test('Quick add waits for a book to add to', async () => {
    vi.mocked(api.entityList).mockReset().mockResolvedValue([])
    render(<App />)

    expect(await screen.findByRole('button', { name: 'Quick add' })).toBeDisabled()
  })
})

describe('App after archiving the current book', () => {
  const household: Entity = { ...entity, id: 'e2', name: 'Household' }

  /** Archives `name` from the Settings books list, through its confirmation. */
  async function archiveFromSettings(name: string) {
    await userEvent.click(await screen.findByRole('button', { name: 'Settings' }))
    await userEvent.click(screen.getByRole('button', { name: /^entities/i }))
    await userEvent.click(screen.getByRole('button', { name: `Archive ${name}` }))
    await userEvent.click(
      within(screen.getByRole('dialog', { name: 'Archive book?' })).getByRole('button', {
        name: 'Archive',
      }),
    )
  }

  test('the next active book becomes the current one, as after deleting it', async () => {
    vi.mocked(api.entityList)
      .mockReset()
      .mockResolvedValueOnce([entity, household])
      .mockResolvedValue([household])
    vi.mocked(api.entityListArchived).mockResolvedValueOnce([]).mockResolvedValue([entity])
    render(<App />)
    const personal = await screen.findByRole('button', { name: 'Personal, EUR' })
    expect(personal).toHaveAttribute('aria-current', 'true')

    await archiveFromSettings('Personal')

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Household, EUR' })).toHaveAttribute(
        'aria-current',
        'true',
      )
    })
    expect(api.entityArchive).toHaveBeenCalledWith('e1')
    // The archived book is gone from the switcher, so it cannot be opened;
    // it is only offered for restoring.
    expect(screen.queryByRole('button', { name: 'Personal, EUR' })).toBeNull()
    expect(await screen.findByRole('button', { name: 'Restore Personal' })).toBeEnabled()
    expect(screen.getByRole('button', { name: 'Settings' })).toHaveAttribute('aria-current', 'page')
    expect(screen.queryByRole('alert')).toBeNull()
  })

  test('with no active book left the app shows its no-book state', async () => {
    vi.mocked(api.entityList).mockReset().mockResolvedValueOnce([entity]).mockResolvedValue([])
    vi.mocked(api.entityListArchived).mockResolvedValueOnce([]).mockResolvedValue([entity])
    render(<App />)
    await screen.findByRole('button', { name: 'Personal, EUR' })

    await archiveFromSettings('Personal')

    expect(await screen.findByText('No entities yet')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Quick add' })).toBeDisabled()
    expect(await screen.findByRole('button', { name: 'Restore Personal' })).toBeEnabled()
    expect(screen.queryByRole('alert')).toBeNull()
  })
})
