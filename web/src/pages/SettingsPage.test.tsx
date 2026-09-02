/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
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
    getLockTimeout: vi.fn(async () => 900),
    setLockTimeout: vi.fn(),
    licenseStatus: vi.fn(),
    licenseInstall: vi.fn(),
    eulaText: vi.fn(),
    entityCreate: vi.fn(),
  },
}))

vi.mock('@tauri-apps/plugin-opener', () => ({
  openUrl: vi.fn(),
}))

import { api } from '../lib/api'
import { vaultBackup, vaultChangePassword, vaultPickBackup, vaultRestore } from '../lib/tauri'
import { openUrl } from '@tauri-apps/plugin-opener'
import { SettingsPage } from './SettingsPage'
import { resetI18nForTests } from '../lib/i18n'

const entity: Entity = {
  id: 'e1',
  name: 'Personal',
  base_currency: 'EUR',
  fiscal_year_start_month: 1,
  chart_template: 'personal',
}

const BACKUP_PATH = '/tmp/backup.oikonomia-backup'

const noopAsync = async () => {}

afterEach(() => {
  cleanup()
  resetI18nForTests()
})

beforeEach(() => {
  vi.mocked(vaultBackup).mockReset()
  vi.mocked(vaultRestore).mockReset()
  vi.mocked(vaultChangePassword).mockReset()
  vi.mocked(vaultPickBackup).mockReset()
  vi.mocked(vaultPickBackup).mockResolvedValue(BACKUP_PATH)
  vi.mocked(api.getLockTimeout).mockReset().mockResolvedValue(900)
  vi.mocked(api.licenseStatus).mockReset().mockResolvedValue({
    state: 'trial',
    days_remaining: 12,
  })
  vi.mocked(api.licenseInstall).mockReset()
  vi.mocked(api.eulaText).mockReset().mockResolvedValue('')
  vi.mocked(api.entityCreate).mockReset()
  vi.mocked(api.setLockTimeout).mockReset()
  vi.mocked(openUrl).mockReset()
})

async function expandVaultBackup() {
  await userEvent.click(screen.getByRole('button', { name: /vault backup/i }))
}

async function expandEntities() {
  await userEvent.click(screen.getByRole('button', { name: /entities/i }))
}

describe('SettingsPage license', () => {
  test('Language is first; License is present without a Buy button or extra trial helper', async () => {
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    const titles = screen.getAllByRole('heading', { level: 3 }).map((el) => el.textContent)
    expect(titles[0]).toBe('Language')
    expect(titles).toContain('License')
    await waitFor(() => {
      expect(screen.getByText('12 days left in your trial')).toBeTruthy()
    })
    // No buy_url on this mock (pre-buy-path status shape), so no Buy button.
    // (Not /buy/i: the section's collapsible header button also contains the
    // description text, which now mentions where to buy a license.)
    expect(screen.queryByRole('button', { name: /buy a license/i })).toBeNull()
    expect(screen.queryByText(/one machine/i)).toBeNull()
    expect(screen.queryByText(/full app during the trial/i)).toBeNull()
    expect(screen.getByRole('button', { name: /import license/i })).toBeTruthy()
  })

  test('trial banner uses days remaining from license_status', async () => {
    vi.mocked(api.licenseStatus).mockResolvedValue({ state: 'trial', days_remaining: 3 })
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    await waitFor(() => {
      expect(screen.getByText('3 days left in your trial')).toBeTruthy()
    })
  })

  test('licensed pill formats the date and offers Import another file', async () => {
    vi.mocked(api.licenseStatus).mockResolvedValue({
      state: 'licensed',
      licensed_until: '2027-08-20',
    })
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    await waitFor(() => {
      expect(
        screen.getByText((content) =>
          content.replace(/\u00a0|\u202f/g, ' ').includes('Licensed until 20 Aug 2027'),
        ),
      ).toBeTruthy()
    })
    expect(screen.getByRole('button', { name: /import another file/i })).toBeTruthy()
  })

  test('import license_invalid shows Writer generic, not Rust Display', async () => {
    vi.mocked(api.licenseInstall).mockRejectedValue({
      code: 'license_invalid',
      message: 'ed25519: signature verification failed on blob',
    })
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    await userEvent.click(screen.getByRole('button', { name: /import license/i }))
    await waitFor(() => {
      expect(screen.getByText('Could not import the license.')).toBeTruthy()
    })
    expect(screen.queryByText(/ed25519/i)).toBeNull()
    expect(screen.queryByText(/signature verification failed/i)).toBeNull()
  })

  test('import license_expired becomes the expired banner, not an import error', async () => {
    vi.mocked(api.licenseInstall).mockRejectedValue({
      code: 'license_expired',
      message: 'LicenseExpired: rust Display must never appear',
    })
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    await userEvent.click(screen.getByRole('button', { name: /import license/i }))
    await waitFor(() => {
      expect(
        screen.getByText('Trial ended. You can still back up, restore, and export CSV.'),
      ).toBeTruthy()
    })
    expect(screen.queryByText('Could not import the license.')).toBeNull()
    expect(screen.queryByText(/rust Display/i)).toBeNull()
  })

  test('cancelled license_install leaves status unchanged', async () => {
    vi.mocked(api.licenseInstall).mockResolvedValue(null)
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    await waitFor(() => {
      expect(screen.getByText('12 days left in your trial')).toBeTruthy()
    })
    await userEvent.click(screen.getByRole('button', { name: /import license/i }))
    await waitFor(() => {
      expect(api.licenseInstall).toHaveBeenCalledTimes(1)
    })
    expect(screen.getByText('12 days left in your trial')).toBeTruthy()
    expect(screen.queryByText('Could not import the license.')).toBeNull()
  })

  test('clicking License agreement shows the bundled EULA text', async () => {
    vi.mocked(api.eulaText).mockResolvedValue('OIKONOMIA END-USER LICENSE AGREEMENT — Ourovoros.io')
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    expect(screen.queryByText(/OIKONOMIA END-USER LICENSE AGREEMENT/)).toBeNull()
    await userEvent.click(screen.getByRole('button', { name: /license agreement/i }))
    await waitFor(() => {
      expect(
        screen.getByText(/OIKONOMIA END-USER LICENSE AGREEMENT — Ourovoros\.io/),
      ).toBeTruthy()
    })
  })

  test('expired Settings still enables Backup and Restore', async () => {
    vi.mocked(api.licenseStatus).mockResolvedValue({ state: 'expired' })
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    await waitFor(() => {
      expect(
        screen.getByText('Trial ended. You can still back up, restore, and export CSV.'),
      ).toBeTruthy()
    })
    await expandVaultBackup()
    expect(screen.getByRole('button', { name: /backup vault/i })).toBeEnabled()
    expect(screen.getByRole('button', { name: /restore from backup/i })).toBeEnabled()
  })

  test('write-gated command surfaces license_expired as expired banner, not a crash', async () => {
    vi.mocked(api.entityCreate).mockRejectedValue({
      code: 'license_expired',
      message: 'LicenseExpired: rust Display must never appear',
    })
    render(
      <SettingsPage
        entities={[]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    await expandEntities()
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /new entity/i })).toBeEnabled()
    })
    await userEvent.click(screen.getByRole('button', { name: /new entity/i }))
    await userEvent.type(screen.getByLabelText('Name'), 'Work')
    await userEvent.click(screen.getByRole('button', { name: /create entity/i }))
    await waitFor(() => {
      expect(
        screen.getByText('Trial ended. You can still back up, restore, and export CSV.'),
      ).toBeTruthy()
    })
    expect(screen.queryByText(/rust Display/i)).toBeNull()
    expect(screen.queryByText(/LicenseExpired/i)).toBeNull()
  })

  test('trial with one entity disables add-book and does not open create', async () => {
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    await waitFor(() => {
      expect(screen.getByText('12 days left in your trial')).toBeTruthy()
    })
    await expandEntities()
    const add = screen.getByRole('button', { name: /new entity/i })
    expect(add).toBeDisabled()
    expect(
      screen.getByText('Import a signed license to keep more than one book in this vault.'),
    ).toBeTruthy()
    await userEvent.click(add)
    expect(screen.queryByRole('dialog', { name: /new entity/i })).toBeNull()
    expect(api.entityCreate).not.toHaveBeenCalled()
  })

  test('licensed with one entity keeps add-book enabled and create can proceed', async () => {
    vi.mocked(api.licenseStatus).mockResolvedValue({
      state: 'licensed',
      licensed_until: '2027-08-20',
    })
    vi.mocked(api.entityCreate).mockResolvedValue({
      id: 'e2',
      name: 'Work',
      base_currency: 'EUR',
      fiscal_year_start_month: 1,
      chart_template: 'personal',
    })
    const onEntitiesChange = vi.fn(noopAsync)
    const onSelectEntity = vi.fn()
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={onEntitiesChange}
        onSelectEntity={onSelectEntity}
      />,
    )
    await expandEntities()
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /new entity/i })).toBeEnabled()
    })
    await userEvent.click(screen.getByRole('button', { name: /new entity/i }))
    await userEvent.type(screen.getByLabelText('Name'), 'Work')
    await userEvent.click(screen.getByRole('button', { name: /create entity/i }))
    await waitFor(() => {
      expect(api.entityCreate).toHaveBeenCalledTimes(1)
    })
    expect(onSelectEntity).toHaveBeenCalledWith('e2')
  })

  test('expired first-book create maps license_expired, not entityLimit', async () => {
    vi.mocked(api.licenseStatus).mockResolvedValue({ state: 'expired' })
    vi.mocked(api.entityCreate).mockRejectedValue({
      code: 'license_expired',
      message: 'LicenseExpired: rust Display must never appear',
    })
    render(
      <SettingsPage
        entities={[]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    await expandEntities()
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /new entity/i })).toBeEnabled()
    })
    await userEvent.click(screen.getByRole('button', { name: /new entity/i }))
    await userEvent.type(screen.getByLabelText('Name'), 'Work')
    await userEvent.click(screen.getByRole('button', { name: /create entity/i }))
    await waitFor(() => {
      expect(
        screen.getByText('Trial ended. You can still back up, restore, and export CSV.'),
      ).toBeTruthy()
    })
    expect(screen.queryByText('A license is required to add another book.')).toBeNull()
    expect(screen.queryByText(/rust Display/i)).toBeNull()
  })

  test('Buy a license renders for a trial with a buy_url and opens it', async () => {
    vi.mocked(api.licenseStatus).mockResolvedValue({
      state: 'trial',
      days_remaining: 3,
      buy_url: 'https://ourovoros.io/oikonomia',
    })
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    const buy = await screen.findByRole('button', { name: /buy a license/i })
    await userEvent.click(buy)
    expect(openUrl).toHaveBeenCalledWith('https://ourovoros.io/oikonomia')
  })

  test('Buy a license renders for an expired status with a buy_url', async () => {
    vi.mocked(api.licenseStatus).mockResolvedValue({
      state: 'expired',
      buy_url: 'https://ourovoros.io/oikonomia',
    })
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /buy a license/i })).toBeTruthy()
    })
  })

  test('Buy a license is absent once licensed, even with a buy_url', async () => {
    vi.mocked(api.licenseStatus).mockResolvedValue({
      state: 'licensed',
      licensed_until: '2027-08-20',
      buy_url: 'https://ourovoros.io/oikonomia',
    })
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /import another file/i })).toBeTruthy()
    })
    expect(screen.queryByRole('button', { name: /buy a license/i })).toBeNull()
  })

  test('entity_create license_entity_limit shows Writer copy, not Rust Display', async () => {
    vi.mocked(api.licenseStatus).mockResolvedValue({
      state: 'licensed',
      licensed_until: '2027-08-20',
    })
    vi.mocked(api.entityCreate).mockRejectedValue({
      code: 'license_entity_limit',
      message: 'LicenseEntityLimit: rust Display must never appear',
    })
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    await expandEntities()
    await waitFor(() => {
      expect(screen.getByRole('button', { name: /new entity/i })).toBeEnabled()
    })
    await userEvent.click(screen.getByRole('button', { name: /new entity/i }))
    await userEvent.type(screen.getByLabelText('Name'), 'Work')
    await userEvent.click(screen.getByRole('button', { name: /create entity/i }))
    await waitFor(() => {
      expect(screen.getByText('A license is required to add another book.')).toBeTruthy()
    })
    expect(screen.queryByText(/rust Display/i)).toBeNull()
    expect(screen.queryByText(/LicenseEntityLimit/i)).toBeNull()
  })
})

describe('SettingsPage hidden chrome HOLD', () => {
  test('Settings has no Hidden export control', () => {
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    expect(screen.queryByText('Hidden')).toBeNull()
    expect(screen.queryByText('Hide from export')).toBeNull()
    expect(screen.queryByRole('checkbox', { name: /hidden/i })).toBeNull()
  })
})

describe('SettingsPage vault backup', () => {
  test('disables Backup vault when there are no entities', async () => {
    render(
      <SettingsPage
        entities={[]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    await expandVaultBackup()
    expect(screen.getByRole('button', { name: /backup vault/i })).toBeDisabled()
    expect(screen.getByRole('button', { name: /restore from backup/i })).toBeEnabled()
    expect(screen.getByText('Nothing to back up')).toBeTruthy()
    expect(
      screen.getByText(
        'This vault has not been initialized yet. Create a book first, or restore an existing backup.',
      ),
    ).toBeTruthy()
  })

  test('disables Backup vault when the vault is missing', async () => {
    render(
      <SettingsPage
        entities={[]}
        vaultPresent={false}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    await expandVaultBackup()
    expect(screen.getByRole('button', { name: /backup vault/i })).toBeDisabled()
    expect(screen.getByRole('button', { name: /restore from backup/i })).toBeEnabled()
    expect(screen.getByText('No vault on this computer')).toBeTruthy()
    expect(
      screen.getByText('Nothing to back up. Restore from a backup, or set up a new vault.'),
    ).toBeTruthy()
  })

  test('enables Backup vault when entities exist', async () => {
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    await expandVaultBackup()
    expect(screen.getByRole('button', { name: /backup vault/i })).toBeEnabled()
  })

  test('clicking Backup vault invokes vaultBackup', async () => {
    vi.mocked(vaultBackup).mockResolvedValue('/tmp/oikonomia-backup-2026-08-13.oikonomia-backup')
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    await expandVaultBackup()
    await userEvent.click(screen.getByRole('button', { name: /backup vault/i }))
    await waitFor(() => {
      expect(vaultBackup).toHaveBeenCalledTimes(1)
    })
  })

  test('pick then confirm then vaultRestore with path and replace:true', async () => {
    vi.mocked(vaultRestore).mockResolvedValue(BACKUP_PATH)
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    await expandVaultBackup()
    await userEvent.click(screen.getByRole('button', { name: /restore from backup/i }))
    await waitFor(() => {
      expect(vaultPickBackup).toHaveBeenCalledTimes(1)
    })
    expect(vaultRestore).not.toHaveBeenCalled()
    expect(screen.getByRole('dialog', { name: 'Replace local vault?' })).toBeTruthy()
    await userEvent.click(screen.getByRole('button', { name: 'Replace vault' }))
    await waitFor(() => {
      expect(vaultRestore).toHaveBeenCalledWith({ path: BACKUP_PATH, replace: true })
    })
  })

  test('vaultBackup CommandError codes render ErrorBanner', async () => {
    const cases: Array<{ code: string; text: string }> = [
      {
        code: 'vault_uninitialized',
        text: 'Nothing to back up. This vault has not been initialized yet.',
      },
      {
        code: 'backup_invalid',
        text: 'That file is not a valid Oikonomia backup.',
      },
      {
        code: 'restore_would_overwrite',
        text: 'A vault already exists on this computer. Confirm replace to continue.',
      },
      { code: 'not_found', text: 'Backup file not found.' },
      { code: 'io', text: 'Could not read or write the backup file.' },
    ]
    for (const { code, text } of cases) {
      cleanup()
      vi.mocked(vaultBackup).mockReset().mockRejectedValue({ code, message: '' })
      render(
        <SettingsPage
          entities={[entity]}
          onEntitiesChange={noopAsync}
          onSelectEntity={() => {}}
        />,
      )
      await expandVaultBackup()
      await userEvent.click(screen.getByRole('button', { name: /backup vault/i }))
      await waitFor(() => {
        expect(screen.getByText(text)).toBeTruthy()
      })
    }
  })

  test('vaultRestore CommandError codes render ErrorBanner', async () => {
    const cases: Array<{ code: string; text: string }> = [
      {
        code: 'vault_uninitialized',
        text: 'Nothing to back up. This vault has not been initialized yet.',
      },
      {
        code: 'backup_invalid',
        text: 'That file is not a valid Oikonomia backup.',
      },
      {
        code: 'restore_would_overwrite',
        text: 'A vault already exists on this computer. Confirm replace to continue.',
      },
      { code: 'not_found', text: 'Backup file not found.' },
      { code: 'io', text: 'Could not read or write the backup file.' },
    ]
    for (const { code, text } of cases) {
      cleanup()
      vi.mocked(vaultPickBackup).mockReset().mockResolvedValue(BACKUP_PATH)
      vi.mocked(vaultRestore).mockReset().mockRejectedValue({ code, message: '' })
      render(
        <SettingsPage
          entities={[entity]}
          onEntitiesChange={noopAsync}
          onSelectEntity={() => {}}
        />,
      )
      await expandVaultBackup()
      await userEvent.click(screen.getByRole('button', { name: /restore from backup/i }))
      await waitFor(() => {
        expect(screen.getByRole('dialog', { name: 'Replace local vault?' })).toBeTruthy()
      })
      await userEvent.click(screen.getByRole('button', { name: 'Replace vault' }))
      await waitFor(() => {
        expect(screen.getByText(text)).toBeTruthy()
      })
    }
  })

  test('cancelled pick does not restore and does not show confirm', async () => {
    vi.mocked(vaultPickBackup).mockResolvedValue(null)
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    await expandVaultBackup()
    await userEvent.click(screen.getByRole('button', { name: /restore from backup/i }))
    await waitFor(() => {
      expect(vaultPickBackup).toHaveBeenCalledTimes(1)
    })
    expect(screen.queryByRole('dialog')).toBeNull()
    expect(vaultRestore).not.toHaveBeenCalled()
  })
})

describe('SettingsPage section design', () => {
  test('New entity lives in the Entities body, gated without a native tooltip', async () => {
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    expect(screen.queryByRole('button', { name: /new entity/i })).toBeNull()
    await userEvent.click(screen.getByRole('button', { name: /entities/i }))
    const newEntity = await screen.findByRole('button', { name: /new entity/i })
    expect(newEntity).toBeDisabled()
    expect(newEntity).not.toHaveAttribute('title')
    expect(
      screen.getAllByText('Import a signed license to keep more than one book in this vault.'),
    ).toHaveLength(1)
  })

  test('licensed vault enables New entity and drops the limit hint', async () => {
    vi.mocked(api.licenseStatus).mockResolvedValue({
      state: 'licensed',
      licensed_until: '2027-08-20',
    })
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    await userEvent.click(screen.getByRole('button', { name: /entities/i }))
    const newEntity = await screen.findByRole('button', { name: /new entity/i })
    await waitFor(() => {
      expect(newEntity).toBeEnabled()
    })
    expect(screen.queryByText(/keep more than one book/i)).toBeNull()
  })

  test('trial pill and Import license share one centered row', async () => {
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    const pill = await screen.findByText('12 days left in your trial')
    const row = pill.parentElement as HTMLElement
    expect(row.className).toMatch(/\bflex\b/)
    expect(row.className).toMatch(/\bitems-center\b/)
    expect(row).toContainElement(screen.getByRole('button', { name: /import license/i }))
  })

  test('master password form is width-constrained', async () => {
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    await userEvent.click(screen.getByRole('button', { name: /master password/i }))
    const submit = await screen.findByRole('button', { name: /change password/i })
    expect(submit.closest('form')?.className).toMatch(/\bmax-w-3xl\b/)
  })

  test('mismatched new passwords mark the confirm field invalid and describe it', async () => {
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    await userEvent.click(screen.getByRole('button', { name: /master password/i }))
    const current = await screen.findByLabelText('Current password')
    const next = screen.getByLabelText('New password')
    const confirm = screen.getByLabelText('Confirm new password')
    expect(confirm).not.toHaveAttribute('aria-invalid')

    await userEvent.type(current, 'oldpass')
    await userEvent.type(next, 'newpass1')
    await userEvent.type(confirm, 'newpass2')
    await userEvent.click(screen.getByRole('button', { name: /change password/i }))

    expect(confirm).toHaveAttribute('aria-invalid', 'true')
    expect(confirm).toHaveAccessibleDescription('New passwords do not match')
    expect(vaultChangePassword).not.toHaveBeenCalled()

    await userEvent.type(confirm, '3')
    expect(confirm).not.toHaveAttribute('aria-invalid')
  })

  test('an incorrect current password marks that field invalid and describes it', async () => {
    vi.mocked(vaultChangePassword).mockRejectedValue({ code: 'invalid_password', message: '' })
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    await userEvent.click(screen.getByRole('button', { name: /master password/i }))
    const current = await screen.findByLabelText('Current password')
    const next = screen.getByLabelText('New password')
    const confirm = screen.getByLabelText('Confirm new password')

    await userEvent.type(current, 'wrongpass')
    await userEvent.type(next, 'newpass1')
    await userEvent.type(confirm, 'newpass1')
    await userEvent.click(screen.getByRole('button', { name: /change password/i }))

    await waitFor(() => {
      expect(current).toHaveAttribute('aria-invalid', 'true')
    })
    expect(current).toHaveAccessibleDescription('Current password is incorrect.')
  })
})

describe('SettingsPage createBookIntent', () => {
  afterEach(() => {
    delete (window.HTMLElement.prototype as { scrollIntoView?: unknown }).scrollIntoView
  })

  test('opens the new-entity form and scrolls to it when createBookIntent increments', async () => {
    const scrollIntoView = vi.fn()
    window.HTMLElement.prototype.scrollIntoView = scrollIntoView
    const { rerender } = render(
      <SettingsPage
        entities={[]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
        createBookIntent={0}
      />,
    )
    expect(screen.queryByRole('dialog', { name: /new entity/i })).toBeNull()

    rerender(
      <SettingsPage
        entities={[]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
        createBookIntent={1}
      />,
    )

    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: /new entity/i })).toBeTruthy()
    })
    expect(scrollIntoView).toHaveBeenCalledWith({ behavior: 'smooth' })
  })

  test('does not open the form when createBookIntent is not provided', () => {
    render(
      <SettingsPage entities={[]} onEntitiesChange={noopAsync} onSelectEntity={() => {}} />,
    )
    expect(screen.queryByRole('dialog', { name: /new entity/i })).toBeNull()
  })

  test('calls onCreateBookIntentHandled once the intent is consumed', async () => {
    const onCreateBookIntentHandled = vi.fn()
    const { rerender } = render(
      <SettingsPage
        entities={[]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
        createBookIntent={0}
        onCreateBookIntentHandled={onCreateBookIntentHandled}
      />,
    )
    expect(onCreateBookIntentHandled).not.toHaveBeenCalled()

    rerender(
      <SettingsPage
        entities={[]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
        createBookIntent={1}
        onCreateBookIntentHandled={onCreateBookIntentHandled}
      />,
    )

    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: /new entity/i })).toBeTruthy()
    })
    expect(onCreateBookIntentHandled).toHaveBeenCalledTimes(1)

    // App resets the prop back to 0 in response; that rerender must not
    // call the handler again (the effect's guard makes it a no-op).
    rerender(
      <SettingsPage
        entities={[]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
        createBookIntent={0}
        onCreateBookIntentHandled={onCreateBookIntentHandled}
      />,
    )
    expect(onCreateBookIntentHandled).toHaveBeenCalledTimes(1)
  })

  test('missing scrollIntoView in the test DOM does not throw (jsdom guard)', async () => {
    const { rerender } = render(
      <SettingsPage
        entities={[]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
        createBookIntent={0}
      />,
    )
    rerender(
      <SettingsPage
        entities={[]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
        createBookIntent={1}
      />,
    )
    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: /new entity/i })).toBeTruthy()
    })
  })
})
