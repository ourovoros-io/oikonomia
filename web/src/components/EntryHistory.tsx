import { useEffect, useState } from 'react'
import { api, formatDate, type EntryChange, type EntryHistoryItem } from '../lib/api'
import { useI18n } from '../lib/I18nProvider'

type State = { status: 'loading' } | { status: 'failed' } | { status: 'ready'; items: EntryHistoryItem[] }

const CHANGE_LABEL = {
  original: 'entry.history.original',
  reversal: 'entry.history.reversal',
  replacement: 'entry.history.replacement',
} as const satisfies Record<EntryChange, string>

/**
 * The audit trail of an edited entry: the earlier versions and reversals core
 * stored, with their stored descriptions. Renders nothing for an entry that
 * was never edited. Mount it with the entry's id as `key`, so a different
 * entry starts from a fresh load.
 */
export function EntryHistory({ entryId }: { entryId: string }) {
  const { t } = useI18n()
  const [state, setState] = useState<State>({ status: 'loading' })

  useEffect(() => {
    let current = true
    api
      .entryHistory(entryId)
      .then((items) => {
        if (current) setState({ status: 'ready', items })
      })
      .catch(() => {
        if (current) setState({ status: 'failed' })
      })

    return () => {
      current = false
    }
  }, [entryId])

  if (state.status === 'loading') return null
  if (state.status === 'ready' && state.items.length === 0) return null

  return (
    <section aria-labelledby="entry-history-title">
      <h3
        id="entry-history-title"
        className="mb-1 font-mono text-[11px] font-medium tracking-[0.14em] text-[var(--color-muted)] uppercase"
      >
        {t('entry.history.title')}
      </h3>
      {state.status === 'failed' ? (
        <p className="text-xs text-[var(--color-muted)]">{t('entry.history.error')}</p>
      ) : (
        <>
          <p className="mb-2 text-xs text-[var(--color-muted)]">{t('entry.history.note')}</p>
          <ul className="divide-y divide-[var(--color-border)] rounded-xl border border-[var(--color-border)]">
            {state.items.map((item) => (
              <li key={item.entry_id} className="flex flex-wrap items-baseline gap-x-3 gap-y-1 px-4 py-2.5">
                <span className="text-xs tabular-nums text-[var(--color-muted)]">
                  {formatDate(item.entry_date)}
                </span>
                <span className="text-xs font-medium text-[var(--color-fg-secondary)]">
                  {t(CHANGE_LABEL[item.change])}
                </span>
                <span className="min-w-0 flex-1 text-sm break-words text-[var(--color-fg)]">
                  {item.description}
                </span>
              </li>
            ))}
          </ul>
        </>
      )}
    </section>
  )
}
