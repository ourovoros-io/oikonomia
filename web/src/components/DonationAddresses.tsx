import { Check, Copy } from 'lucide-react'
import { useState } from 'react'
import type { DonationAddress } from '../lib/api'
import { t } from '../lib/i18n'
import { Button, ErrorBanner } from './ui'

/** Donation addresses from Rust. Renders and copies; holds no rules. */
export function DonationAddresses({ addresses }: { addresses: DonationAddress[] }) {
  const [copied, setCopied] = useState<string | null>(null)
  const [failed, setFailed] = useState(false)

  if (addresses.length === 0) return null

  async function onCopy(entry: DonationAddress) {
    try {
      await navigator.clipboard.writeText(entry.address)
      setFailed(false)
      setCopied(entry.coin)
    } catch {
      setCopied(null)
      setFailed(true)
    }
  }

  return (
    <div className="space-y-3">
      <p className="text-xs leading-snug text-[var(--color-muted)]">
        {t('settings.donate.warning')}
      </p>

      <ul className="divide-y divide-[var(--color-border)]">
        {addresses.map((entry) => (
          <li key={entry.coin} className="flex items-center gap-3 py-3">
            <div className="w-24 shrink-0">
              <div className="text-sm font-medium text-[var(--color-fg)]">{entry.coin}</div>
              <div className="text-xs text-[var(--color-muted)]">{entry.network}</div>
            </div>

            <div className="min-w-0 flex-1">
              <code className="block select-all break-all font-mono text-xs text-[var(--color-fg-secondary)]">
                {entry.address}
              </code>
              {entry.also_accepts.length > 0 ? (
                <div className="mt-1 text-xs text-[var(--color-muted)]">
                  {t('settings.donate.alsoAccepts', { tokens: entry.also_accepts.join(', ') })}
                </div>
              ) : null}
            </div>

            <Button
              variant="secondary"
              size="iconSm"
              onClick={() => void onCopy(entry)}
              aria-label={t('settings.donate.copy', { coin: entry.coin })}
              title={t('settings.donate.copy', { coin: entry.coin })}
            >
              {copied === entry.coin ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
            </Button>
            {copied === entry.coin ? (
              <span role="status" className="text-xs text-[var(--color-muted)]">
                {t('settings.donate.copied')}
              </span>
            ) : null}
          </li>
        ))}
      </ul>

      <ErrorBanner message={failed ? t('settings.donate.copyFailed') : null} />
    </div>
  )
}
