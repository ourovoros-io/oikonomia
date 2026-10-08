/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'
import type { UiPrefs } from '../lib/api'

vi.mock('../lib/tauri', () => ({
  vaultBackup: vi.fn(),
  vaultRestore: vi.fn(),
  vaultPickBackup: vi.fn(),
  vaultChangePassword: vi.fn(),
}))

vi.mock('../lib/api', () => ({
  api: {
    getLockTimeout: vi.fn(),
    donationAddresses: vi.fn(),
    entityListArchived: vi.fn(),
    resolveLocale: vi.fn(),
    getLocale: vi.fn(),
    setLocale: vi.fn(),
    getUiPrefs: vi.fn(),
    resetUiPrefs: vi.fn(),
  },
}))

import { api } from '../lib/api'
import { I18nProvider } from '../lib/I18nProvider'
import { SettingsPage } from './SettingsPage'
import { getLocale, resetI18nForTests } from '../lib/i18n'

const NOTICE =
  'The preferences file is damaged, so your language and Quick Add choices cannot be saved. ' +
  'Nothing in your vault is affected. You can reset the preferences in Settings.'

const readable: UiPrefs = {
  locale: 'en',
  last_entity_id: null,
  last_accounts_by_entity_kind: {},
  unreadable: false,
}
const unreadable: UiPrefs = { ...readable, unreadable: true }

/** What Rust answers a save with while the preferences file is damaged. */
const REFUSAL = {
  code: 'prefs_unreadable',
  message: 'replace a preferences file that does not decode: expected value at line 1 column 1',
  params: { operation: 'replace a preferences file that does not decode' },
}

afterEach(() => {
  cleanup()
  resetI18nForTests()
})

beforeEach(() => {
  vi.mocked(api.getLockTimeout).mockReset().mockResolvedValue(900)
  vi.mocked(api.donationAddresses).mockReset().mockResolvedValue([])
  vi.mocked(api.entityListArchived).mockReset().mockResolvedValue([])
  vi.mocked(api.resolveLocale).mockReset().mockResolvedValue('en')
  vi.mocked(api.getLocale).mockReset().mockResolvedValue('en')
  vi.mocked(api.setLocale).mockReset().mockResolvedValue(undefined)
  vi.mocked(api.getUiPrefs).mockReset().mockResolvedValue(readable)
  vi.mocked(api.resetUiPrefs).mockReset().mockResolvedValue(readable)
})

function renderSettings() {
  render(
    <I18nProvider>
      <SettingsPage entities={[]} onEntitiesChange={async () => {}} onSelectEntity={() => {}} />
    </I18nProvider>,
  )
}

/** The reset button, which is on screen only inside the notice. */
function resetButton() {
  return screen.queryByRole('button', { name: 'Reset preferences' })
}

describe('SettingsPage damaged preferences', () => {
  test('readable preferences show no notice and no reset button', async () => {
    renderSettings()

    await waitFor(() => expect(api.getUiPrefs).toHaveBeenCalled())
    await vi.mocked(api.getUiPrefs).mock.results[0].value

    expect(screen.queryByText(NOTICE)).toBeNull()
    expect(resetButton()).toBeNull()
    expect(api.resetUiPrefs).not.toHaveBeenCalled()
  })

  test('unreadable preferences are announced quietly with a reset button', async () => {
    vi.mocked(api.getUiPrefs).mockResolvedValue(unreadable)
    renderSettings()

    const notice = (await screen.findByText(NOTICE)).closest('[role="status"]')

    expect(notice).not.toBeNull()
    expect(notice).toContainElement(resetButton())
    // Quiet: a status, not an alert, and nothing was saved or reset to find out.
    expect(screen.queryByRole('alert')).toBeNull()
    expect(api.setLocale).not.toHaveBeenCalled()
    expect(api.resetUiPrefs).not.toHaveBeenCalled()
  })

  test('a reset removes the notice and says where the damaged file went', async () => {
    vi.mocked(api.getUiPrefs).mockResolvedValue(unreadable)
    renderSettings()
    await screen.findByText(NOTICE)

    await userEvent.click(screen.getByRole('button', { name: 'Reset preferences' }))

    await waitFor(() => expect(screen.queryByText(NOTICE)).toBeNull())
    expect(api.resetUiPrefs).toHaveBeenCalledTimes(1)
    expect(resetButton()).toBeNull()
    expect(screen.getByRole('status')).toHaveTextContent(
      'Preferences reset. The damaged file was kept beside the vault as ui-prefs.damaged.json.',
    )
    expect(screen.queryByRole('alert')).toBeNull()
  })

  test('the reset button works from the keyboard', async () => {
    vi.mocked(api.getUiPrefs).mockResolvedValue(unreadable)
    renderSettings()
    await screen.findByText(NOTICE)

    screen.getByRole('button', { name: 'Reset preferences' }).focus()
    await userEvent.keyboard('{Enter}')

    await waitFor(() => expect(screen.queryByText(NOTICE)).toBeNull())
    expect(api.resetUiPrefs).toHaveBeenCalledTimes(1)
  })

  test('a reset that fails keeps the notice and shows the screen sentence, never the raw text', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    vi.mocked(api.getUiPrefs).mockResolvedValue(unreadable)
    vi.mocked(api.resetUiPrefs).mockRejectedValue({
      code: 'io',
      message: 'move the damaged preferences file aside: Permission denied (os error 13)',
      params: { operation: 'move the damaged preferences file aside' },
    })
    renderSettings()
    await screen.findByText(NOTICE)

    await userEvent.click(screen.getByRole('button', { name: 'Reset preferences' }))

    expect(await screen.findByRole('alert')).toHaveTextContent('Could not reset the preferences.')
    expect(screen.getByText(NOTICE)).toBeInTheDocument()
    expect(resetButton()).toBeEnabled()
    expect(screen.queryByText(/Permission denied/)).toBeNull()
  })

  test('a reset that leaves the file unreadable keeps the notice', async () => {
    vi.mocked(api.getUiPrefs).mockResolvedValue(unreadable)
    vi.mocked(api.resetUiPrefs).mockResolvedValue(unreadable)
    renderSettings()
    await screen.findByText(NOTICE)

    await userEvent.click(screen.getByRole('button', { name: 'Reset preferences' }))

    await waitFor(() => expect(api.resetUiPrefs).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(resetButton()).toBeEnabled())
    expect(screen.getByText(NOTICE)).toBeInTheDocument()
    expect(screen.queryByText(/Preferences reset/)).toBeNull()
  })

  test('a refused language change brings the notice up, and after a reset the language saves', async () => {
    // The file is damaged after the page has loaded, so only the failed
    // save can prompt the page to ask again.
    vi.mocked(api.getUiPrefs).mockResolvedValueOnce(readable).mockResolvedValue(unreadable)
    vi.mocked(api.setLocale).mockRejectedValueOnce(REFUSAL)
    renderSettings()
    await waitFor(() => expect(api.resolveLocale).toHaveBeenCalled())
    await userEvent.click(screen.getByRole('button', { name: /language/i }))

    await userEvent.click(screen.getByRole('radio', { name: 'Deutsch' }))

    expect(await screen.findByText(NOTICE)).toBeInTheDocument()
    expect(getLocale()).toBe('en')
    expect(screen.getByRole('alert')).toHaveTextContent('Could not change the language.')

    await userEvent.click(screen.getByRole('button', { name: 'Reset preferences' }))
    await waitFor(() => expect(screen.queryByText(NOTICE)).toBeNull())

    await userEvent.click(screen.getByRole('radio', { name: 'Deutsch' }))

    await waitFor(() => expect(getLocale()).toBe('de'))
    expect(api.setLocale).toHaveBeenCalledTimes(2)
    await waitFor(() => expect(screen.queryByRole('alert')).toBeNull())
    expect(screen.getByRole('radio', { name: 'Deutsch' })).toHaveAttribute('aria-checked', 'true')
  })
})
