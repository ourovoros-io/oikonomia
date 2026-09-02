/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'

vi.mock('@tauri-apps/plugin-opener', () => ({
  openUrl: vi.fn(),
}))

import { openUrl } from '@tauri-apps/plugin-opener'
import { TrialBanner } from './TrialBanner'
import { resetI18nForTests } from '../lib/i18n'

afterEach(() => {
  cleanup()
  resetI18nForTests()
})

beforeEach(() => {
  vi.mocked(openUrl).mockReset()
})

describe('TrialBanner', () => {
  test('renders nothing for a fresh trial (20 days left)', () => {
    render(<TrialBanner license={{ state: 'trial', days_remaining: 20 }} />)
    expect(screen.queryByRole('status')).toBeNull()
  })

  test('renders nothing when license is null', () => {
    render(<TrialBanner license={null} />)
    expect(screen.queryByRole('status')).toBeNull()
  })

  test('renders the countdown copy when the trial is about to expire', () => {
    render(<TrialBanner license={{ state: 'trial', days_remaining: 3 }} />)
    const banner = screen.getByRole('status')
    expect(banner).toHaveTextContent('3 days left in your trial.')
  })

  test('renders expired copy and a Buy button that opens buy_url', async () => {
    render(
      <TrialBanner
        license={{ state: 'expired', buy_url: 'https://ourovoros.io/oikonomia' }}
      />,
    )
    const banner = screen.getByRole('status')
    expect(banner).toHaveTextContent(
      'Trial ended. Your books stay readable; buy a license to keep writing.',
    )
    const buy = screen.getByRole('button', { name: /buy a license/i })
    await userEvent.click(buy)
    expect(openUrl).toHaveBeenCalledWith('https://ourovoros.io/oikonomia')
  })

  test('expired without a buy_url shows no Buy button', () => {
    render(<TrialBanner license={{ state: 'expired' }} />)
    expect(screen.queryByRole('button', { name: /buy a license/i })).toBeNull()
  })
})
