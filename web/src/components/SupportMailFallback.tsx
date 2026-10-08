import { Check, Copy } from 'lucide-react'
import { useState } from 'react'
import { t } from '../lib/i18n'
import { Button } from './ui'

/**
 * The support address with a copy button, shown after "Email support" was
 * clicked. On Linux `xdg-open` reports success when it hands a `mailto:` link
 * to the browser, so the app cannot tell whether a mail app opened; the
 * address stays in reach either way.
 */
export function SupportMailFallback({ email }: { email: string }) {
  const [copied, setCopied] = useState(false)
  const [failed, setFailed] = useState(false)

  async function onCopy() {
    try {
      await navigator.clipboard.writeText(email)
      setFailed(false)
      setCopied(true)
    } catch {
      setCopied(false)
      setFailed(true)
    }
  }

  return (
    <div className="flex flex-wrap items-center gap-2 text-sm text-[var(--color-fg-secondary)]">
      <span>{t('settings.support.noMailApp')}</span>
      <code className="select-all font-mono text-xs text-[var(--color-fg)]">{email}</code>
      <Button
        variant="secondary"
        size="iconSm"
        onClick={() => void onCopy()}
        aria-label={t('settings.support.copy')}
        title={t('settings.support.copy')}
      >
        {copied ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
      </Button>
      <span role="status" className="text-xs text-[var(--color-muted)]">
        {copied ? t('settings.donate.copied') : failed ? t('settings.donate.copyFailed') : ''}
      </span>
    </div>
  )
}
