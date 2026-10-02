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
    donationAddresses: vi.fn(),
    setLockTimeout: vi.fn(),
    openSupportEmail: vi.fn(),
    entityCreate: vi.fn(),
  },
}))

import { api } from '../lib/api'
import { vaultBackup, vaultChangePassword, vaultPickBackup, vaultRestore } from '../lib/tauri'
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
  vi.mocked(api.donationAddresses).mockReset().mockResolvedValue([
    { coin: 'BTC', network: 'Bitcoin', also_accepts: [], address: 'bc1qexampleexampleexample' },
  ])
  vi.mocked(api.openSupportEmail).mockReset().mockResolvedValue(undefined)
  vi.mocked(api.entityCreate).mockReset()
  vi.mocked(api.setLockTimeout).mockReset()
})

async function expandVaultBackup() {
  await userEvent.click(screen.getByRole('button', { name: /vault backup/i }))
}

describe('SettingsPage', () => {
  test('Settings has no License section and no trial copy', async () => {
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    const titles = screen.getAllByRole('heading', { level: 3 }).map((el) => el.textContent)
    expect(titles[0]).toBe('Language')
    expect(titles).not.toContain('License')
    expect(screen.queryByText(/trial/i)).toBeNull()
    expect(screen.queryByRole('button', { name: /import license/i })).toBeNull()
    expect(screen.queryByRole('button', { name: /license agreement/i })).toBeNull()
  })

  test('Donate section sits after Language and lists the Rust addresses', async () => {
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    // The section appears once the Rust call resolves.
    await screen.findByRole('heading', { level: 3, name: 'Donate' })
    const titles = screen.getAllByRole('heading', { level: 3 }).map((el) => el.textContent)
    expect(titles.slice(0, 2)).toEqual(['Language', 'Donate'])

    await userEvent.click(screen.getByRole('button', { name: /donate/i }))
    expect(await screen.findByText('bc1qexampleexampleexample')).toBeTruthy()
  })

  test('Donate section is absent when Rust returns no addresses', async () => {
    vi.mocked(api.donationAddresses).mockResolvedValue([])
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    // Wait for the call to be made and settled, or the absence check is vacuous.
    await waitFor(() => expect(api.donationAddresses).toHaveBeenCalled())
    await vi.mocked(api.donationAddresses).mock.results[0].value
    await waitFor(() => expect(screen.getAllByRole('heading', { level: 3 }).length).toBeGreaterThan(0))

    expect(screen.queryByRole('heading', { name: 'Donate' })).toBeNull()
  })

  test('New entity is enabled when the vault already has a book', async () => {
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    expect(screen.queryByRole('button', { name: /new entity/i })).not.toBeInTheDocument()

    await userEvent.click(screen.getByRole('button', { name: /entities/i }))

    const newEntity = await screen.findByRole('button', { name: /new entity/i })
    expect(newEntity).toBeEnabled()
    expect(newEntity).not.toHaveAttribute('title')
  })

  test('creating an entity sends the form payload and selects the new book', async () => {
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

    await userEvent.click(screen.getByRole('button', { name: /entities/i }))
    await userEvent.click(await screen.findByRole('button', { name: /new entity/i }))
    await userEvent.type(screen.getByLabelText('Name'), 'Work')
    await userEvent.click(screen.getByRole('button', { name: /create entity/i }))

    await waitFor(() => {
      expect(onSelectEntity).toHaveBeenCalledWith('e2')
    })
    expect(api.entityCreate).toHaveBeenCalledTimes(1)
    expect(api.entityCreate).toHaveBeenCalledWith({
      name: 'Work',
      base_currency: 'EUR',
      chart_template: 'personal',
      fiscal_year_start_month: 1,
    })
    expect(onEntitiesChange).toHaveBeenCalledTimes(1)
    expect(onSelectEntity).toHaveBeenCalledTimes(1)
  })

  test('a failed entity create shows the error and does not select a book', async () => {
    vi.mocked(api.entityCreate).mockRejectedValue({
      code: 'name_taken',
      message: 'the name "Personal" is already in use',
      params: { name: 'Personal' },
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

    await userEvent.click(screen.getByRole('button', { name: /entities/i }))
    await userEvent.click(await screen.findByRole('button', { name: /new entity/i }))
    await userEvent.type(screen.getByLabelText('Name'), 'Personal')
    await userEvent.click(screen.getByRole('button', { name: /create entity/i }))

    expect(
      await screen.findByText('The name \u201cPersonal\u201d is already in use. Choose a different name.'),
    ).toBeInTheDocument()
    expect(screen.queryByText(/is already in use$/)).toBeNull()
    expect(api.entityCreate).toHaveBeenCalledTimes(1)
    expect(onEntitiesChange).not.toHaveBeenCalled()
    expect(onSelectEntity).not.toHaveBeenCalled()
  })

  test('a failed entity create with an unknown code never shows the raw message', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    vi.mocked(api.entityCreate).mockRejectedValue({
      code: 'brand_new',
      message: 'UNIQUE constraint failed: entities.name',
    })
    render(
      <SettingsPage entities={[entity]} onEntitiesChange={vi.fn(noopAsync)} onSelectEntity={vi.fn()} />,
    )

    await userEvent.click(screen.getByRole('button', { name: /entities/i }))
    await userEvent.click(await screen.findByRole('button', { name: /new entity/i }))
    await userEvent.type(screen.getByLabelText('Name'), 'Personal')
    await userEvent.click(screen.getByRole('button', { name: /create entity/i }))

    expect(await screen.findByText('Something went wrong.')).toBeInTheDocument()
    expect(screen.queryByText(/UNIQUE constraint/)).toBeNull()
  })

  const appInfo = { name: 'Oikonomia', version: '0.1.0-dev', support_email: 'info@ourovoros.io' }

  test('Support section shows the Rust-provided address and asks Rust to open the mail client', async () => {
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
        appInfo={appInfo}
      />,
    )
    expect(
      screen.getByText('Questions and bug reports: info@ourovoros.io'),
    ).toBeTruthy()
    await userEvent.click(screen.getByRole('button', { name: /^support/i }))
    expect(screen.getByText(/Write to info@ourovoros\.io with the app version \(0\.1\.0-dev\)/)).toBeTruthy()
    await userEvent.click(screen.getByRole('button', { name: /email support/i }))
    expect(api.openSupportEmail).toHaveBeenCalledTimes(1)
  })

  test('Support button failure falls back to the on-screen address', async () => {
    vi.mocked(api.openSupportEmail).mockRejectedValue({ code: 'io', message: 'no mail client' })
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
        appInfo={appInfo}
      />,
    )
    await userEvent.click(screen.getByRole('button', { name: /^support/i }))
    await userEvent.click(screen.getByRole('button', { name: /email support/i }))
    await waitFor(() => {
      expect(
        screen.getByText('Could not open your mail app. Write to info@ourovoros.io instead.'),
      ).toBeTruthy()
    })
    expect(screen.queryByText('no mail client')).toBeNull()
  })

  test('Support section waits for app info instead of inventing an address', () => {
    render(
      <SettingsPage
        entities={[entity]}
        onEntitiesChange={noopAsync}
        onSelectEntity={() => {}}
      />,
    )
    expect(screen.queryByRole('button', { name: /^support/i })).toBeNull()
    expect(screen.queryByText(/info@ourovoros\.io/)).toBeNull()
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

  test('a vague restore failure says the restore failed, not the backup', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    vi.mocked(vaultPickBackup).mockResolvedValue(BACKUP_PATH)
    vi.mocked(vaultRestore).mockRejectedValue({ code: 'crypto', message: 'bad tag' })
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
      expect(screen.getByText('Could not restore the backup.')).toBeTruthy()
    })
    expect(screen.queryByText('Could not complete the backup.')).toBeNull()
  })

  test('a failure choosing the backup file says the restore failed', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    vi.mocked(vaultPickBackup).mockRejectedValue({ code: 'crypto', message: 'x' })
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
      expect(screen.getByText('Could not restore the backup.')).toBeTruthy()
    })
  })

  test('a vague backup failure still says the backup failed', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    vi.mocked(vaultBackup).mockRejectedValue({ code: 'crypto', message: 'bad tag' })
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
      expect(screen.getByText('Could not complete the backup.')).toBeTruthy()
    })
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

  test('a password change failing with a known code shows that code copy, with its parameters', async () => {
    vi.mocked(vaultChangePassword).mockRejectedValue({
      code: 'password_too_short',
      message: 'password must be at least 12 characters',
      params: { min: '12' },
    })
    render(
      <SettingsPage entities={[entity]} onEntitiesChange={noopAsync} onSelectEntity={() => {}} />,
    )
    await userEvent.click(screen.getByRole('button', { name: /master password/i }))
    await userEvent.type(await screen.findByLabelText('Current password'), 'oldpass')
    await userEvent.type(screen.getByLabelText('New password'), 'short')
    await userEvent.type(screen.getByLabelText('Confirm new password'), 'short')
    await userEvent.click(screen.getByRole('button', { name: /change password/i }))

    expect(
      await screen.findByText('That password is too short. Use at least 12 characters.'),
    ).toBeInTheDocument()
    expect(screen.queryByText('password must be at least 12 characters')).toBeNull()
  })

  test('a password change failing with an unknown code shows the screen fallback, never the raw message', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    vi.mocked(vaultChangePassword).mockRejectedValue({
      code: 'brand_new',
      message: 'sqlcipher: rekey failed',
    })
    render(
      <SettingsPage entities={[entity]} onEntitiesChange={noopAsync} onSelectEntity={() => {}} />,
    )
    await userEvent.click(screen.getByRole('button', { name: /master password/i }))
    await userEvent.type(await screen.findByLabelText('Current password'), 'oldpass')
    await userEvent.type(screen.getByLabelText('New password'), 'newpass12345')
    await userEvent.type(screen.getByLabelText('Confirm new password'), 'newpass12345')
    await userEvent.click(screen.getByRole('button', { name: /change password/i }))

    expect(await screen.findByText('Could not change the password.')).toBeInTheDocument()
    expect(screen.queryByText(/sqlcipher/)).toBeNull()
  })

  test('a password change rejected with undefined shows the screen fallback and does not throw', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    vi.mocked(vaultChangePassword).mockRejectedValue(undefined)
    render(
      <SettingsPage entities={[entity]} onEntitiesChange={noopAsync} onSelectEntity={() => {}} />,
    )
    await userEvent.click(screen.getByRole('button', { name: /master password/i }))
    await userEvent.type(await screen.findByLabelText('Current password'), 'oldpass')
    await userEvent.type(screen.getByLabelText('New password'), 'newpass12345')
    await userEvent.type(screen.getByLabelText('Confirm new password'), 'newpass12345')
    await userEvent.click(screen.getByRole('button', { name: /change password/i }))

    expect(await screen.findByText('Could not change the password.')).toBeInTheDocument()
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

  test('an unrelated success after a failed password change clears the stale invalid field', async () => {
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

    await userEvent.type(current, 'oldpass')
    await userEvent.type(next, 'newpass1')
    await userEvent.type(confirm, 'newpass2')
    await userEvent.click(screen.getByRole('button', { name: /change password/i }))
    expect(confirm).toHaveAttribute('aria-invalid', 'true')
    expect(confirm).toHaveAttribute('aria-describedby')

    // An unrelated action succeeds (auto-lock preset) — the stale
    // aria-invalid/aria-describedby from the earlier password error must
    // not survive it, or aria-describedby would point at a banner that no
    // longer describes this field (or is unmounted once error clears).
    await userEvent.click(screen.getByRole('button', { name: /auto-lock/i }))
    await userEvent.click(await screen.findByRole('button', { name: '5 min' }))

    await waitFor(() => {
      expect(api.setLockTimeout).toHaveBeenCalled()
    })
    expect(confirm).not.toHaveAttribute('aria-invalid')
    expect(confirm).not.toHaveAttribute('aria-describedby')
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
