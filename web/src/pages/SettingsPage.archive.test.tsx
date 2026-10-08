/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'
import type { Entity } from '../lib/api'

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
    getUiPrefs: vi.fn(),
    entityListArchived: vi.fn(),
    entityArchive: vi.fn(),
    entityUnarchive: vi.fn(),
    entityDelete: vi.fn(),
  },
}))

import { api } from '../lib/api'
import { SettingsPage } from './SettingsPage'
import { resetI18nForTests } from '../lib/i18n'

function book(id: string, name: string): Entity {
  return {
    id,
    name,
    base_currency: 'EUR',
    base_currency_decimals: 2,
    fiscal_year_start_month: 1,
    chart_template: 'personal',
  }
}

const home = book('e1', 'Home')
const shop = book('e2', 'Shop')
const oldShop = book('e3', 'Old shop')

afterEach(() => {
  cleanup()
  resetI18nForTests()
  vi.restoreAllMocks()
})

beforeEach(() => {
  vi.mocked(api.getLockTimeout).mockReset().mockResolvedValue(900)
  vi.mocked(api.donationAddresses).mockReset().mockResolvedValue([])
  vi.mocked(api.getUiPrefs).mockReset().mockResolvedValue({
    locale: 'en',
    last_entity_id: null,
    last_accounts_by_entity_kind: {},
    unreadable: false,
  })
  vi.mocked(api.entityListArchived).mockReset().mockResolvedValue([])
  vi.mocked(api.entityArchive).mockReset().mockResolvedValue(undefined)
  vi.mocked(api.entityUnarchive).mockReset().mockResolvedValue(undefined)
  vi.mocked(api.entityDelete).mockReset().mockResolvedValue(undefined)
})

/** Renders Settings over `entities` with the books section open. */
async function openBooks(entities: Entity[], onEntitiesChange = vi.fn(async () => {})) {
  render(
    <SettingsPage
      entities={entities}
      onEntitiesChange={onEntitiesChange}
      onSelectEntity={() => {}}
    />,
  )
  await userEvent.click(screen.getByRole('button', { name: /^books/i }))
  return onEntitiesChange
}

/** The region that lists the archived books; absent when there are none. */
function archivedRegion() {
  return screen.queryByRole('region', { name: 'Archived' })
}

describe('SettingsPage archiving a book', () => {
  test('every book row offers Archive beside Open and Delete', async () => {
    await openBooks([home, shop])

    // Named by aria-label; a native title would linger over the dialog it opens.
    expect(screen.getByRole('button', { name: 'Archive Home' })).not.toHaveAttribute('title')
    expect(screen.getByRole('button', { name: 'Archive Shop' })).toBeEnabled()
    expect(screen.getByRole('button', { name: 'Delete Home' })).toBeEnabled()
  })

  test('Archive asks first, saying the book becomes read-only and can be restored', async () => {
    await openBooks([home, shop])

    await userEvent.click(screen.getByRole('button', { name: 'Archive Home' }))

    const dialog = screen.getByRole('dialog', { name: 'Archive book?' })
    expect(dialog).toHaveAttribute('aria-modal', 'true')
    expect(dialog).toHaveTextContent(
      '“Home” becomes read-only and leaves the list of books. Nothing is deleted, and you can restore it here at any time.',
    )
    // Not the destructive look of Delete: nothing is lost.
    expect(within(dialog).getByRole('button', { name: 'Archive' })).toHaveAttribute(
      'data-variant',
      'primary',
    )
    expect(api.entityArchive).not.toHaveBeenCalled()
  })

  test('cancelling archives nothing and hands focus back to the button', async () => {
    const onEntitiesChange = await openBooks([home, shop])
    const trigger = screen.getByRole('button', { name: 'Archive Home' })
    await userEvent.click(trigger)

    await userEvent.click(screen.getByRole('button', { name: 'Cancel' }))

    expect(screen.queryByRole('dialog')).toBeNull()
    expect(api.entityArchive).not.toHaveBeenCalled()
    expect(onEntitiesChange).not.toHaveBeenCalled()
    expect(trigger).toHaveFocus()
  })

  test('Escape closes the confirmation without archiving', async () => {
    await openBooks([home])
    const trigger = screen.getByRole('button', { name: 'Archive Home' })
    trigger.focus()
    await userEvent.keyboard('{Enter}')
    expect(screen.getByRole('dialog', { name: 'Archive book?' })).toBeInTheDocument()

    await userEvent.keyboard('{Escape}')

    expect(screen.queryByRole('dialog')).toBeNull()
    expect(api.entityArchive).not.toHaveBeenCalled()
    expect(trigger).toHaveFocus()
  })

  test('confirming archives the book, reloads the books and lists it as archived', async () => {
    vi.mocked(api.entityListArchived).mockResolvedValueOnce([]).mockResolvedValue([home])
    const onEntitiesChange = await openBooks([home, shop])
    expect(archivedRegion()).toBeNull()

    await userEvent.click(screen.getByRole('button', { name: 'Archive Home' }))
    await userEvent.click(
      within(screen.getByRole('dialog')).getByRole('button', { name: 'Archive' }),
    )

    const region = await screen.findByRole('region', { name: 'Archived' })
    expect(api.entityArchive).toHaveBeenCalledTimes(1)
    expect(api.entityArchive).toHaveBeenCalledWith('e1')
    expect(onEntitiesChange).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole('dialog')).toBeNull()
    expect(within(region).getByText('Home')).toBeInTheDocument()
    expect(within(region).getByRole('button', { name: 'Restore Home' })).toBeEnabled()
    expect(screen.queryByRole('alert')).toBeNull()
  })

  test('a failed archive closes the dialog and shows the screen sentence, never the raw text', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    vi.mocked(api.entityArchive).mockRejectedValue({
      code: 'database',
      message: 'archive entity: disk I/O error',
      params: { operation: 'archive entity' },
    })
    const onEntitiesChange = await openBooks([home])

    await userEvent.click(screen.getByRole('button', { name: 'Archive Home' }))
    await userEvent.click(
      within(screen.getByRole('dialog')).getByRole('button', { name: 'Archive' }),
    )

    expect(await screen.findByRole('alert')).toHaveTextContent('Could not archive the book.')
    expect(screen.queryByRole('dialog')).toBeNull()
    expect(screen.queryByText(/disk I\/O/)).toBeNull()
    expect(onEntitiesChange).not.toHaveBeenCalled()
  })
})

describe('SettingsPage archived books', () => {
  test('are listed under their own heading, in the order Rust sends, and only restorable', async () => {
    vi.mocked(api.entityListArchived).mockResolvedValue([oldShop, home])
    await openBooks([shop])

    const region = await screen.findByRole('region', { name: 'Archived' })

    expect(region).toHaveTextContent(
      'Archived books are kept as they are and cannot be opened or changed. Restore one to use it again.',
    )
    const rows = within(region).getAllByRole('listitem')
    expect(rows.map((row) => within(row).getByRole('button').getAttribute('aria-label'))).toEqual([
      'Restore Old shop',
      'Restore Home',
    ])
    // An archived book is never opened, so there is no write to disable.
    for (const row of rows) {
      expect(within(row).getAllByRole('button')).toHaveLength(1)
      expect(within(row).getByRole('button')).toHaveTextContent('Restore')
    }
    expect(screen.queryByRole('button', { name: 'Archive Home' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Delete Home' })).toBeNull()
  })

  test('are not listed, heading included, when there are none', async () => {
    await openBooks([shop])
    await waitFor(() => expect(api.entityListArchived).toHaveBeenCalledTimes(1))
    await vi.mocked(api.entityListArchived).mock.results[0].value

    expect(archivedRegion()).toBeNull()
    expect(screen.queryByText('Archived')).toBeNull()
  })

  test('are shown when no active book is left', async () => {
    vi.mocked(api.entityListArchived).mockResolvedValue([home])
    await openBooks([])

    expect(await screen.findByRole('button', { name: 'Restore Home' })).toBeEnabled()
    expect(screen.getByText('Use the New book button above to create your first book.')).toBeInTheDocument()
  })

  test('are not asked for when there is no vault', async () => {
    render(
      <SettingsPage
        entities={[]}
        vaultPresent={false}
        onEntitiesChange={async () => {}}
        onSelectEntity={() => {}}
      />,
    )
    await waitFor(() => expect(api.getUiPrefs).toHaveBeenCalled())

    expect(api.entityListArchived).not.toHaveBeenCalled()
    expect(screen.queryByRole('alert')).toBeNull()
  })

  test('a list that cannot be loaded says so', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    vi.mocked(api.entityListArchived).mockRejectedValue({
      code: 'database',
      message: 'list archived entities: disk I/O error',
      params: { operation: 'list archived entities' },
    })
    await openBooks([shop])

    expect(await screen.findByRole('alert')).toHaveTextContent('Could not load the archived books.')
    expect(screen.queryByText(/disk I\/O/)).toBeNull()
  })

  test('Restore un-archives the book and reloads both lists', async () => {
    vi.mocked(api.entityListArchived).mockResolvedValueOnce([home]).mockResolvedValue([])
    const onEntitiesChange = await openBooks([shop])

    await userEvent.click(await screen.findByRole('button', { name: 'Restore Home' }))

    await waitFor(() => expect(archivedRegion()).toBeNull())
    expect(api.entityUnarchive).toHaveBeenCalledTimes(1)
    expect(api.entityUnarchive).toHaveBeenCalledWith('e1')
    expect(onEntitiesChange).toHaveBeenCalledTimes(1)
    expect(api.entityListArchived).toHaveBeenCalledTimes(2)
    expect(screen.queryByRole('alert')).toBeNull()
  })

  test('Restore works from the keyboard', async () => {
    vi.mocked(api.entityListArchived).mockResolvedValue([home])
    await openBooks([shop])

    ;(await screen.findByRole('button', { name: 'Restore Home' })).focus()
    await userEvent.keyboard('{Enter}')

    await waitFor(() => expect(api.entityUnarchive).toHaveBeenCalledWith('e1'))
  })

  test('a restore whose name was taken says which name and what to do, and the book stays archived', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    vi.mocked(api.entityListArchived).mockResolvedValue([home])
    vi.mocked(api.entityUnarchive).mockRejectedValue({
      code: 'name_taken',
      message: 'the name "Home" is already in use',
      params: { name: 'Home' },
    })
    const onEntitiesChange = await openBooks([book('e9', 'home')])

    await userEvent.click(await screen.findByRole('button', { name: 'Restore Home' }))

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Another book is now named “Home”. Archive or delete that book before restoring this one.',
    )
    // Not the general sentence, which says to choose a name: an archived
    // book cannot be renamed.
    expect(screen.queryByText(/Choose a different name/)).toBeNull()
    expect(screen.queryByText(/is already in use$/)).toBeNull()
    expect(onEntitiesChange).not.toHaveBeenCalled()
    expect(await screen.findByRole('button', { name: 'Restore Home' })).toBeEnabled()
    expect(warn).not.toHaveBeenCalled()
  })

  test('any other failed restore shows the screen sentence, never the raw text', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    vi.mocked(api.entityListArchived).mockResolvedValue([home])
    vi.mocked(api.entityUnarchive).mockRejectedValue({
      code: 'database',
      message: 'un-archive entity: disk I/O error',
      params: { operation: 'un-archive entity' },
    })
    await openBooks([shop])

    await userEvent.click(await screen.findByRole('button', { name: 'Restore Home' }))

    expect(await screen.findByRole('alert')).toHaveTextContent('Could not restore the book.')
    expect(screen.queryByText(/disk I\/O/)).toBeNull()
  })

  test('a book restored elsewhere in the meantime is reported and dropped from the list', async () => {
    vi.mocked(api.entityListArchived).mockResolvedValueOnce([home]).mockResolvedValue([])
    vi.mocked(api.entityUnarchive).mockRejectedValue({
      code: 'not_found',
      message: 'entity not found',
      params: { resource: 'entity' },
    })
    await openBooks([shop])

    await userEvent.click(await screen.findByRole('button', { name: 'Restore Home' }))

    expect(await screen.findByRole('alert')).toHaveTextContent('That item could not be found.')
    await waitFor(() => expect(archivedRegion()).toBeNull())
  })

  test('while one restore runs, the other Restore buttons wait', async () => {
    let finish: () => void = () => {}
    vi.mocked(api.entityListArchived).mockResolvedValue([home, oldShop])
    vi.mocked(api.entityUnarchive).mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve
        }),
    )
    await openBooks([shop])

    await userEvent.click(await screen.findByRole('button', { name: 'Restore Home' }))

    expect(screen.getByRole('button', { name: 'Restore Home' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Restore Old shop' })).toBeDisabled()

    finish()
    await waitFor(() => expect(screen.getByRole('button', { name: 'Restore Old shop' })).toBeEnabled())
  })
})
