import { createContext } from 'react'

/**
 * The app header's slots. A page that renders <TopBar> puts its title and
 * actions here; App shows the book's name only while no page claims the title.
 */
export type TopBarSlots = {
  /** Where the page title goes; null until the header has mounted. */
  title: HTMLElement | null
  /** Where the page's actions go, left of Lock; null until mounted. */
  actions: HTMLElement | null
  /** Call while a page owns the title; the returned function gives it back. */
  claimTitle: () => () => void
}

export const TopBarContext = createContext<TopBarSlots | null>(null)
