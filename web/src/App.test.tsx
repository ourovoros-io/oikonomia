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
    getTheme: vi.fn(async () => 'dark'),
    setTheme: vi.fn(),
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
  vi.mocked(appInfo).mockReset().mockResolvedValue({ name: 'Oikonomia', version: '0.1.0-dev' })
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
})
