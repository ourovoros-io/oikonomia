/** @vitest-environment jsdom */

import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import '@testing-library/jest-dom/vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'

vi.mock('../lib/tauri', () => ({
  vaultInit: vi.fn(),
  vaultUnlock: vi.fn(),
  vaultRestore: vi.fn(),
  vaultStatus: vi.fn(),
  vaultPickBackup: vi.fn(),
  updateCheck: vi.fn(),
  updateInstall: vi.fn(),
  updateCancel: vi.fn(),
  updateTakeNotice: vi.fn(),
}))

import {
  updateCancel,
  updateCheck,
  updateInstall,
  updateTakeNotice,
  vaultUnlock,
} from '../lib/tauri'
import { UnlockScreen } from './UnlockScreen'
import { resetI18nForTests, setLocale } from '../lib/i18n'
import {
  CHECKING_MIN_MS,
  resetStartupUpdateNoticeForTests,
  type InstallProgress,
} from '../lib/updateCheck'

afterEach(() => {
  cleanup()
  resetI18nForTests()
  resetStartupUpdateNoticeForTests()
  window.history.replaceState({}, '', '/')
  vi.useRealTimers()
})

beforeEach(() => {
  vi.mocked(updateCheck).mockReset().mockResolvedValue({ kind: 'upToDate' })
  vi.mocked(updateInstall).mockReset().mockResolvedValue(undefined)
  vi.mocked(updateCancel).mockReset().mockResolvedValue(false)
  vi.mocked(updateTakeNotice).mockReset().mockResolvedValue(null)
  vi.mocked(vaultUnlock).mockReset().mockResolvedValue('unlocked')
})

function click(name: string) {
  fireEvent.click(screen.getByRole('button', { name }))
}

async function advance(ms: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms)
  })
}

describe('checking is held and the footer button stays busy', () => {
  beforeEach(() => {
    vi.useFakeTimers({
      toFake: ['setTimeout', 'clearTimeout', 'Date'],
    })
  })

  test('a fast result stays on Checking for 600ms, and the footer button is busy', async () => {
    vi.mocked(updateCheck).mockResolvedValue({ kind: 'upToDate' })
    render(<UnlockScreen status="locked" onUnlocked={() => {}} />)

    click('Check for updates')
    await advance(0)

    expect(screen.getByRole('dialog', { name: 'Checking' })).toBeTruthy()
    expect(screen.queryByRole('dialog', { name: 'You’re up to date' })).toBeNull()
    const check = screen.getByRole('button', { name: 'Check for updates' })
    expect(check).toBeDisabled()
    expect(check).toHaveAttribute('aria-busy', 'true')

    await advance(CHECKING_MIN_MS - 1)
    expect(screen.getByRole('dialog', { name: 'Checking' })).toBeTruthy()

    await advance(1)
    expect(screen.getByRole('dialog', { name: 'You’re up to date' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Check for updates' })).not.toBeDisabled()
    expect(screen.getByRole('button', { name: 'Check for updates' })).toHaveAttribute(
      'aria-busy',
      'false',
    )
  })

  test('cancelling during the hold drops the late result', async () => {
    vi.mocked(updateCheck).mockResolvedValue({ kind: 'available', version: '0.1.4' })
    render(<UnlockScreen status="locked" onUnlocked={() => {}} />)

    click('Check for updates')
    await advance(0)
    click('Cancel')
    await advance(CHECKING_MIN_MS)

    expect(screen.queryByRole('dialog')).toBeNull()
    expect(screen.queryByText('Oikonomia 0.1.4')).toBeNull()
    expect(updateInstall).not.toHaveBeenCalled()
  })
})

describe('download bars and cancel', () => {
  test('a known total is a determinate bar and an unknown total is not', () => {
    window.history.replaceState({}, '', '/?unlockUpdate=downloading')
    const { unmount } = render(<UnlockScreen status="locked" onUnlocked={() => {}} />)

    const bar = screen.getByRole('progressbar')
    expect(bar).toHaveAttribute('data-bar', 'determinate')
    expect(bar.firstElementChild).toHaveStyle({ width: '40%' })
    expect(screen.getByRole('dialog').textContent).toContain('9.6\u00a0MB of 24.0\u00a0MB')
    expect(screen.getByRole('dialog').textContent).toContain('40%')
    expect(screen.getByRole('button', { name: 'Check for updates' })).toBeDisabled()
    expect(screen.getByRole('button', { name: 'Check for updates' })).toHaveAttribute(
      'aria-busy',
      'true',
    )

    unmount()
    window.history.replaceState({}, '', '/?unlockUpdate=downloading-unknown')
    render(<UnlockScreen status="locked" onUnlocked={() => {}} />)

    expect(screen.getByRole('progressbar')).toHaveAttribute('data-bar', 'indeterminate')
    expect(screen.getByRole('dialog').textContent).toContain('9.6\u00a0MB downloaded')
    expect(screen.queryByText('40%')).toBeNull()
    expect(screen.getByRole('progressbar').firstElementChild).toHaveClass('oik-indeterminate-fill')
  })

  test('checking uses the indeterminate bar', () => {
    window.history.replaceState({}, '', '/?unlockUpdate=checking')
    render(<UnlockScreen status="locked" onUnlocked={() => {}} />)

    expect(screen.getByRole('progressbar')).toHaveAttribute('data-bar', 'indeterminate')
    expect(screen.getByRole('progressbar').firstElementChild).toHaveClass('oik-indeterminate-fill')
  })

  test('cancel returns to the offer only when Rust aborted the download', async () => {
    vi.mocked(updateCheck).mockResolvedValue({ kind: 'available', version: '0.1.4' })
    vi.mocked(updateCancel).mockResolvedValue(false)
    vi.mocked(updateInstall).mockImplementation(
      () => new Promise(() => undefined),
    )
    render(<UnlockScreen status="locked" onUnlocked={() => {}} />)

    await userEvent.click(screen.getByRole('button', { name: 'Check for updates' }))
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Install and restart' })).toBeTruthy()
    })
    await userEvent.click(screen.getByRole('button', { name: 'Install and restart' }))
    expect(screen.getByRole('dialog', { name: 'Downloading' })).toBeTruthy()

    await userEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    expect(updateCancel).toHaveBeenCalledTimes(1)
    expect(screen.getByRole('dialog', { name: 'Downloading' })).toBeTruthy()

    vi.mocked(updateCancel).mockResolvedValue(true)
    await userEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: 'An update is available' })).toBeTruthy()
    })
    expect(screen.getByText('Oikonomia 0.1.4')).toBeTruthy()
    expect(screen.queryByRole('dialog', { name: 'Downloading' })).toBeNull()

    await userEvent.click(screen.getByRole('button', { name: 'Install and restart' }))
    await waitFor(() => {
      expect(updateInstall).toHaveBeenCalledTimes(2)
    })
    expect(screen.getByRole('dialog', { name: 'Downloading' })).toBeTruthy()
  })

  test('update_install cancelled returns to the offer', async () => {
    vi.mocked(updateCheck).mockResolvedValue({ kind: 'available', version: '0.1.4' })
    vi.mocked(updateInstall).mockResolvedValue({ kind: 'cancelled' })
    render(<UnlockScreen status="locked" onUnlocked={() => {}} />)

    await userEvent.click(screen.getByRole('button', { name: 'Check for updates' }))
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Install and restart' })).toBeTruthy()
    })
    await userEvent.click(screen.getByRole('button', { name: 'Install and restart' }))
    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: 'An update is available' })).toBeTruthy()
    })
    expect(screen.getByRole('button', { name: 'Install and restart' })).toBeTruthy()
    expect(screen.queryByRole('dialog', { name: 'Downloading' })).toBeNull()
    expect(screen.queryByRole('dialog', { name: 'Couldn’t check' })).toBeNull()
  })

  test('a cached artifact fills the bar, then installing shows at once', async () => {
    vi.mocked(updateCheck).mockResolvedValue({ kind: 'available', version: '0.1.4' })
    vi.mocked(updateInstall).mockImplementation(async (_available, onProgress) => {
      onProgress({ kind: 'downloading', received: 0, total: null })
      onProgress({ kind: 'downloading', received: 24_000_000, total: 24_000_000 })
      onProgress({ kind: 'installing' })
      return new Promise(() => undefined)
    })
    render(<UnlockScreen status="locked" onUnlocked={() => {}} />)

    await userEvent.click(screen.getByRole('button', { name: 'Check for updates' }))
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Install and restart' })).toBeTruthy()
    })
    await userEvent.click(screen.getByRole('button', { name: 'Install and restart' }))
    await waitFor(() => {
      expect(screen.getByRole('dialog', { name: 'Installing' })).toBeTruthy()
    })
    expect(screen.getByRole('progressbar')).toHaveAttribute('data-bar', 'full')
  })

  test('a complete cache report paints the full download bar before installing', async () => {
    vi.mocked(updateCheck).mockResolvedValue({ kind: 'available', version: '0.1.4' })
    vi.mocked(updateInstall).mockImplementation(async (_available, onProgress) => {
      onProgress({ kind: 'downloading', received: 24_000_000, total: 24_000_000 })
      return new Promise(() => undefined)
    })
    render(<UnlockScreen status="locked" onUnlocked={() => {}} />)

    await userEvent.click(screen.getByRole('button', { name: 'Check for updates' }))
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Install and restart' })).toBeTruthy()
    })
    await userEvent.click(screen.getByRole('button', { name: 'Install and restart' }))
    await waitFor(() => {
      expect(screen.getByRole('progressbar')).toHaveAttribute('data-bar', 'determinate')
    })
    expect(screen.getByRole('progressbar').firstElementChild).toHaveStyle({ width: '100%' })
    expect(screen.getByRole('dialog').textContent).toContain('24.0\u00a0MB of 24.0\u00a0MB')
    expect(screen.getByRole('dialog').textContent).toContain('100%')
    expect(screen.queryByRole('dialog', { name: 'Installing' })).toBeNull()
  })

  test('install failures show the existing sentence for each Rust code', async () => {
    const codes = [
      'update_network',
      'update_artifact_integrity',
      'update_manifest_signature',
      'update_cache_io',
      'update_install_failed',
    ] as const
    const sentence = 'Could not check for or install the update. Try again later.'

    for (const code of codes) {
      cleanup()
      resetI18nForTests()
      vi.mocked(updateCheck).mockResolvedValue({ kind: 'available', version: '0.1.4' })
      vi.mocked(updateInstall).mockResolvedValue({ kind: 'failed', code })
      render(<UnlockScreen status="locked" onUnlocked={() => {}} />)

      await userEvent.click(screen.getByRole('button', { name: 'Check for updates' }))
      await waitFor(() => {
        expect(screen.getByRole('button', { name: 'Install and restart' })).toBeTruthy()
      })
      await userEvent.click(screen.getByRole('button', { name: 'Install and restart' }))
      await waitFor(() => {
        expect(screen.getByText(sentence)).toBeTruthy()
      })
      expect(screen.queryByText(code)).toBeNull()
    }
  })

  test('a thrown install keeps the command code on the failed frame', async () => {
    const sentence = 'Could not check for or install the update. Try again later.'
    vi.mocked(updateCheck).mockResolvedValue({ kind: 'available', version: '0.1.4' })
    vi.mocked(updateInstall).mockRejectedValue({
      code: 'update_install_not_allowed',
      message: 'no offer',
    })
    render(<UnlockScreen status="locked" onUnlocked={() => {}} />)

    await userEvent.click(screen.getByRole('button', { name: 'Check for updates' }))
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Install and restart' })).toBeTruthy()
    })
    await userEvent.click(screen.getByRole('button', { name: 'Install and restart' }))
    await waitFor(() => {
      expect(screen.getByText(sentence)).toBeTruthy()
    })
    expect(screen.queryByText('update_install_not_allowed')).toBeNull()
    expect(screen.queryByText('Nothing was changed. You can try again later.')).toBeNull()
  })

  test('a thrown cancel leaves the download and shows the command sentence', async () => {
    vi.mocked(updateCheck).mockResolvedValue({ kind: 'available', version: '0.1.4' })
    vi.mocked(updateInstall).mockImplementation(() => new Promise(() => undefined))
    vi.mocked(updateCancel).mockRejectedValue({
      code: 'task_failed',
      message: 'panicked',
    })
    render(<UnlockScreen status="locked" onUnlocked={() => {}} />)

    await userEvent.click(screen.getByRole('button', { name: 'Check for updates' }))
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Install and restart' })).toBeTruthy()
    })
    await userEvent.click(screen.getByRole('button', { name: 'Install and restart' }))
    expect(screen.getByRole('dialog', { name: 'Downloading' })).toBeTruthy()

    await userEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    await waitFor(() => {
      expect(
        screen.getByText('Something went wrong in the background. Please try again.'),
      ).toBeTruthy()
    })
    expect(screen.queryByRole('dialog', { name: 'Downloading' })).toBeNull()
    expect(screen.queryByText('task_failed')).toBeNull()
  })

  test('installing has no cancel and ignores Escape', () => {
    window.history.replaceState({}, '', '/?unlockUpdate=installing')
    render(<UnlockScreen status="locked" onUnlocked={() => {}} />)

    expect(screen.getByRole('dialog', { name: 'Installing' })).toBeTruthy()
    expect(
      screen.getByText('The window will close and open again by itself.'),
    ).toBeTruthy()
    expect(screen.getByRole('progressbar')).toHaveAttribute('data-bar', 'full')
    expect(screen.queryByRole('button', { name: 'Cancel' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Close' })).toBeNull()
    expect(screen.getByRole('button', { name: 'Check for updates' })).toBeDisabled()

    fireEvent.keyDown(document, { key: 'Escape' })
    expect(screen.getByRole('dialog', { name: 'Installing' })).toBeTruthy()
  })

  test('focus returns to the dialog when its button unmounts', async () => {
    vi.mocked(updateCheck).mockResolvedValue({ kind: 'upToDate' })
    const first = render(<UnlockScreen status="locked" onUnlocked={() => {}} />)

    await userEvent.click(screen.getByRole('button', { name: 'Check for updates' }))
    const close = await screen.findByRole('button', { name: 'Close' })
    close.focus()
    fireEvent.keyDown(document, { key: 'Escape' })
    await waitFor(() => {
      expect(screen.getByLabelText('Password')).toHaveFocus()
    })
    expect(screen.queryByRole('dialog')).toBeNull()
    first.unmount()

    let report: (progress: InstallProgress) => void = () => {}
    vi.mocked(updateCheck).mockResolvedValue({ kind: 'available', version: '0.1.4' })
    vi.mocked(updateInstall).mockImplementation((_available, onProgress) => {
      report = onProgress
      return new Promise(() => undefined)
    })
    render(<UnlockScreen status="locked" onUnlocked={() => {}} />)

    await userEvent.click(screen.getByRole('button', { name: 'Check for updates' }))
    const install = await screen.findByRole('button', { name: 'Install and restart' })
    install.focus()
    await userEvent.click(install)

    // 04 and 04b are both Downloading. Install unmounts as that frame opens.
    const downloading = await screen.findByRole('dialog', { name: 'Downloading' })
    await waitFor(() => {
      expect(downloading).toHaveFocus()
    })

    act(() => {
      report({ kind: 'downloading', received: 9_600_000, total: 24_000_000 })
    })
    const cancel = screen.getByRole('button', { name: 'Cancel' })
    cancel.focus()
    act(() => {
      report({ kind: 'installing' })
    })

    const installing = await screen.findByRole('dialog', { name: 'Installing' })
    await waitFor(() => {
      expect(installing).toHaveFocus()
    })
  })

  test('available shows the version, an empty second line, and locale widths', () => {
    window.history.replaceState({}, '', '/?unlockUpdate=available')
    const view = render(<UnlockScreen status="locked" onUnlocked={() => {}} />)

    const body = document.querySelector('[data-update-region="body"]')
    expect(body?.querySelectorAll('p')).toHaveLength(2)
    expect(body?.textContent).toContain('Oikonomia 0.1.4')
    expect(screen.queryByText('Shows progress while an update downloads.')).toBeNull()
    expect(screen.getByRole('button', { name: 'Later' })).toHaveStyle({ width: '96px' })
    expect(screen.getByRole('button', { name: 'Install and restart' })).toHaveStyle({
      width: '144px',
    })

    setLocale('fr')
    view.rerender(<UnlockScreen status="locked" onUnlocked={() => {}} />)
    expect(screen.getByRole('button', { name: 'Plus tard' })).toHaveStyle({ width: '96px' })
    expect(screen.getByRole('button', { name: 'Installer et redémarrer' })).toHaveStyle({
      width: '176px',
    })

    setLocale('el')
    view.rerender(<UnlockScreen status="locked" onUnlocked={() => {}} />)
    expect(screen.getByRole('button', { name: 'Αργότερα' })).toHaveStyle({ width: '96px' })
    expect(screen.getByRole('button', { name: 'Εγκατάσταση και επανεκκίνηση' })).toHaveStyle({
      width: '232px',
    })

    setLocale('de')
    view.rerender(<UnlockScreen status="locked" onUnlocked={() => {}} />)
    expect(screen.getByRole('button', { name: 'Später' })).toHaveStyle({ width: '104px' })
    expect(screen.getByRole('button', { name: 'Installieren und neu starten' })).toHaveStyle({
      width: '208px',
    })
  })
})

describe('relaunch notice', () => {
  test('renders once, does not steal focus, and Unlock still submits', async () => {
    vi.mocked(updateTakeNotice).mockResolvedValue({ from: '0.1.3', to: '0.1.4' })
    const onUnlocked = vi.fn()
    const first = render(<UnlockScreen status="locked" onUnlocked={onUnlocked} />)

    await waitFor(() => {
      expect(screen.getByRole('status')).toHaveTextContent('Oikonomia was updated to 0.1.4.')
    })
    expect(screen.getByRole('status')).toHaveAttribute('aria-live', 'polite')
    expect(screen.getByLabelText('Password')).toHaveFocus()
    expect(updateTakeNotice).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole('dialog')).toBeNull()

    first.unmount()
    render(<UnlockScreen status="locked" onUnlocked={onUnlocked} />)
    await waitFor(() => {
      expect(screen.getByRole('status')).toBeTruthy()
    })
    expect(updateTakeNotice).toHaveBeenCalledTimes(1)
    expect(screen.getAllByRole('status')).toHaveLength(1)

    await userEvent.type(screen.getByLabelText('Password'), 'secret')
    await userEvent.click(screen.getByRole('button', { name: 'Unlock' }))
    await waitFor(() => {
      expect(vaultUnlock).toHaveBeenCalledWith('secret')
    })
  })

  test('the notice goes away on its own or from the dismiss control', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
    vi.mocked(updateTakeNotice).mockResolvedValue({ from: '0.1.3', to: '0.1.4' })
    render(<UnlockScreen status="locked" onUnlocked={() => {}} />)

    await advance(0)
    expect(screen.getByRole('status')).toBeTruthy()

    await advance(7999)
    expect(screen.getByRole('status')).toBeTruthy()
    await advance(1)
    expect(screen.queryByRole('status')).toBeNull()
  })

  test('the dismiss control removes the notice without a dialog', async () => {
    vi.mocked(updateTakeNotice).mockResolvedValue({ from: '0.1.3', to: '0.1.4' })
    render(<UnlockScreen status="locked" onUnlocked={() => {}} />)
    await waitFor(() => {
      expect(screen.getByRole('status')).toBeTruthy()
    })

    await userEvent.click(screen.getByRole('button', { name: 'Dismiss' }))
    expect(screen.queryByRole('status')).toBeNull()
    expect(screen.getByLabelText('Password')).toBeTruthy()
  })
})

describe('reduced motion', () => {
  test('the indeterminate segment freezes and the fades and width transition stop', () => {
    const css = readFileSync(join(process.cwd(), 'src', 'index.css'), 'utf8')
    const reduced = css.slice(css.lastIndexOf('@media (prefers-reduced-motion: reduce)'))

    expect(css).toContain('@keyframes oik-indeterminate')
    expect(css).toContain('1200ms cubic-bezier(0.65, 0, 0.35, 1)')
    expect(css).toMatch(/\.oik-determinate-fill\s*\{\s*transition:\s*width 200ms linear/)
    expect(reduced).toMatch(/\.oik-indeterminate-fill\s*\{[^}]*animation:\s*none/)
    expect(reduced).toMatch(/margin-left:\s*35%/)
    expect(reduced).toMatch(/\.oik-determinate-fill\s*\{[^}]*transition:\s*none/)
    expect(reduced).toMatch(/\.update-dialog-fade\s*\{[^}]*animation:\s*none/)
  })
})
