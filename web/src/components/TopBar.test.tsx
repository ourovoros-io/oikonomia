/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render, screen, waitFor, within } from '@testing-library/react'
import { useCallback, useMemo, useState, type ReactNode } from 'react'
import { afterEach, describe, expect, test } from 'vitest'

import { TopBarContext } from '../lib/topBar'
import { TopBar } from './TopBar'

afterEach(() => {
  cleanup()
})

/** The same wiring App uses: two slots and a claim count. */
function Shell({ children }: { children?: ReactNode }) {
  const [title, setTitle] = useState<HTMLElement | null>(null)
  const [actions, setActions] = useState<HTMLElement | null>(null)
  const [claims, setClaims] = useState(0)
  const claimTitle = useCallback(() => {
    setClaims((n) => n + 1)
    return () => setClaims((n) => n - 1)
  }, [])
  const slots = useMemo(() => ({ title, actions, claimTitle }), [title, actions, claimTitle])

  return (
    <TopBarContext.Provider value={slots}>
      <header>
        <div data-testid="title-slot" ref={setTitle} />
        {claims === 0 ? <span>Book title</span> : null}
        <div data-testid="actions-slot" ref={setActions} />
      </header>
      <main>{children}</main>
    </TopBarContext.Provider>
  )
}

describe('TopBar', () => {
  test('outside the shell it renders inline, title first', () => {
    render(<TopBar title="Transactions" subtitle="Personal · EUR" actions={<button type="button">New Entry</button>} />)

    expect(screen.getByRole('heading', { level: 1, name: 'Transactions' })).toBeInTheDocument()
    expect(screen.getByText('Personal · EUR')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'New Entry' })).toBeInTheDocument()
  })

  test('inside the shell it fills the header slots and hides the book title', async () => {
    render(
      <Shell>
        <TopBar title="Transactions" actions={<button type="button">New Entry</button>} />
      </Shell>,
    )

    await waitFor(() => {
      expect(within(screen.getByTestId('title-slot')).getByRole('heading', { name: 'Transactions' })).toBeInTheDocument()
    })
    expect(within(screen.getByTestId('actions-slot')).getByRole('button', { name: 'New Entry' })).toBeInTheDocument()
    expect(screen.queryByText('Book title')).toBeNull()
    expect(within(screen.getByRole('main')).queryByRole('heading')).toBeNull()
  })

  test('gives the title back when the page goes away', async () => {
    const { rerender } = render(
      <Shell>
        <TopBar title="Transactions" />
      </Shell>,
    )
    await waitFor(() => expect(screen.queryByText('Book title')).toBeNull())

    rerender(<Shell />)

    expect(await screen.findByText('Book title')).toBeInTheDocument()
  })
})
