/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, test, vi } from 'vitest'
import type { DonationAddress } from '../lib/api'
import { resetI18nForTests } from '../lib/i18n'
import { DonationAddresses } from './DonationAddresses'

const addresses: DonationAddress[] = [
  { coin: 'BTC', network: 'Bitcoin', also_accepts: [], address: 'bc1qexampleexampleexample' },
  {
    coin: 'ETH',
    network: 'Ethereum',
    also_accepts: ['USDC', 'USDT'],
    address: '0x00000000000000000000000000000000000000aa',
  },
]

function stubClipboard(writeText: (text: string) => Promise<void>) {
  Object.defineProperty(navigator, 'clipboard', {
    value: { writeText },
    configurable: true,
  })
}

afterEach(() => {
  cleanup()
  resetI18nForTests()
})

describe('DonationAddresses', () => {
  test('shows one row per address with coin, network, and stablecoin note', () => {
    render(<DonationAddresses addresses={addresses} />)

    expect(screen.getByText('bc1qexampleexampleexample')).toBeTruthy()
    expect(screen.getByText('0x00000000000000000000000000000000000000aa')).toBeTruthy()
    expect(screen.getByText('Bitcoin')).toBeTruthy()
    expect(screen.getByText('Also accepts USDC, USDT')).toBeTruthy()
    expect(screen.getAllByText(/also accepts/i)).toHaveLength(1)
    expect(screen.getByText(/cannot be reversed/i)).toBeTruthy()
  })

  test('copy writes exactly the address and confirms', async () => {
    const writeText = vi.fn(async () => {})
    stubClipboard(writeText)
    render(<DonationAddresses addresses={addresses} />)

    await userEvent.click(screen.getByRole('button', { name: 'Copy ETH address' }))

    expect(writeText).toHaveBeenCalledWith('0x00000000000000000000000000000000000000aa')
    expect(await screen.findByText('Copied')).toBeTruthy()
  })

  test('a refused clipboard write tells the user to copy by hand', async () => {
    stubClipboard(vi.fn(async () => Promise.reject(new Error('denied'))))
    render(<DonationAddresses addresses={addresses} />)

    await userEvent.click(screen.getByRole('button', { name: 'Copy BTC address' }))

    expect(await screen.findByRole('alert')).toHaveTextContent(/copy it by hand/i)
    expect(screen.queryByText('Copied')).toBeNull()
  })

  test('renders nothing when there are no addresses', () => {
    const { container } = render(<DonationAddresses addresses={[]} />)
    expect(container).toBeEmptyDOMElement()
  })
})
