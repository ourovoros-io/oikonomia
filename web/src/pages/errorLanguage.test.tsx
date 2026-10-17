/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { act, cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'

vi.mock('../lib/tauri', () => ({
  vaultBackup: vi.fn(),
  vaultRestore: vi.fn(),
  vaultPickBackup: vi.fn(),
  vaultChangePassword: vi.fn(),
}))

vi.mock('../lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/api')>()
  return {
    ...actual,
    api: {
      ...actual.api,
      getLockTimeout: vi.fn(async () => 900),
      donationAddresses: vi.fn(async () => []),
      getUiPrefs: vi.fn(async () => ({ locale: 'en', unreadable: false })),
      entityListArchived: vi.fn(),
      documentList: vi.fn(),
      recurringList: vi.fn(),
      accountList: vi.fn(async () => []),
      accountDefaults: vi.fn(),
    },
  }
})

import { RenameDialog } from '../components/RenameDialog'
import { ErrorBanner } from '../components/ui'
import { api, type Entity } from '../lib/api'
import { resetI18nForTests, setLocale, t } from '../lib/i18n'
import { copyError } from '../lib/uiError'
import { DocumentsPage } from './DocumentsPage'
import { RecurringPage } from './RecurringPage'
import { SettingsPage } from './SettingsPage'

const entity: Entity = {
  id: 'e1',
  name: 'Personal',
  base_currency: 'EUR',
  base_currency_decimals: 2,
  fiscal_year_start_month: 1,
  chart_template: 'personal',
}

/** Switches the language the way the Settings pill does, outside any handler. */
function switchToGreek() {
  act(() => setLocale('el'))
}

beforeEach(() => {
  vi.spyOn(console, 'warn').mockImplementation(() => undefined)
})

afterEach(() => {
  cleanup()
  resetI18nForTests()
  vi.restoreAllMocks()
})

describe('a language switch re-words an error banner that is up', () => {
  test('on Settings, a page error from a failed command', async () => {
    vi.mocked(api.entityListArchived).mockRejectedValue({ code: 'io', message: 'EACCES' })
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={async () => {}}
        onEntityAdded={async () => {}}
        onSelectEntity={() => {}}
      />,
    )
    const english = 'Could not load the archived books.'
    expect(await screen.findByRole('alert')).toHaveTextContent(english)

    switchToGreek()

    const greek = t('settings.entities.archived.loadError')
    expect(greek).not.toBe(english)
    expect(screen.getByRole('alert')).toHaveTextContent(greek)
  })

  test('on Documents, the error from loading the list', async () => {
    vi.mocked(api.documentList).mockRejectedValue({ code: 'vault_locked', message: 'locked' })
    render(<DocumentsPage entity={entity} />)
    const english = 'The vault is locked.'
    expect(await screen.findByRole('alert')).toHaveTextContent(english)

    switchToGreek()

    const greek = t('error.vaultLocked')
    expect(greek).not.toBe(english)
    expect(screen.getByRole('alert')).toHaveTextContent(greek)
  })

  test('in a dialog, a refused save with the value it names', async () => {
    const onSave = vi.fn().mockRejectedValue({
      code: 'name_taken',
      message: 'name taken',
      params: { name: 'Home' },
    })
    const user = userEvent.setup()
    render(
      <RenameDialog
        current="Personal"
        title="Rename book"
        label="Name"
        onSave={onSave}
        onClose={() => {}}
      />,
    )
    await user.clear(screen.getByLabelText('Name'))
    await user.type(screen.getByLabelText('Name'), 'Home')
    await user.click(screen.getByRole('button', { name: 'Rename' }))
    const english = t('error.nameTaken', { name: 'Home' })
    expect(await screen.findByRole('alert')).toHaveTextContent(english)

    switchToGreek()

    const greek = t('error.nameTaken', { name: 'Home' })
    expect(greek).not.toBe(english)
    expect(greek).toContain('Home')
    expect(screen.getByRole('alert')).toHaveTextContent(greek)
  })

  test('on Recurring, a page error routed through the dialog error hook', async () => {
    vi.mocked(api.recurringList).mockRejectedValue({ code: 'vault_locked', message: 'locked' })
    render(<RecurringPage entity={entity} onBack={() => {}} />)
    const english = 'The vault is locked.'
    expect(await screen.findByRole('alert')).toHaveTextContent(english)

    switchToGreek()

    const greek = t('error.vaultLocked')
    expect(greek).not.toBe(english)
    expect(screen.getByRole('alert')).toHaveTextContent(greek)
  })
})

describe('a language switch and the banner position', () => {
  afterEach(() => {
    // jsdom has no scrollIntoView; each test installs its own spy.
    Reflect.deleteProperty(Element.prototype, 'scrollIntoView')
  })

  test('re-wording a banner does not scroll it into view again', () => {
    const scroll = vi.fn()
    Element.prototype.scrollIntoView = scroll
    const error = copyError('date.invalid')
    render(<ErrorBanner error={error} />)
    expect(scroll).toHaveBeenCalledTimes(1)

    switchToGreek()

    expect(screen.getByRole('alert')).toHaveTextContent(t('date.invalid'))
    expect(scroll).toHaveBeenCalledTimes(1)
  })

  test('a new error after a language switch is still scrolled into view', () => {
    const scroll = vi.fn()
    Element.prototype.scrollIntoView = scroll
    const { rerender } = render(<ErrorBanner error={copyError('date.invalid')} />)
    switchToGreek()

    rerender(<ErrorBanner error={copyError('tx.invalidAmount')} />)

    expect(scroll).toHaveBeenCalledTimes(2)
  })
})
