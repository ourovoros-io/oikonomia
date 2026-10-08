import { useI18n } from '../lib/I18nProvider'
import { Button } from './ui'

/**
 * Tells the user, wherever they landed after unlocking, that the preferences
 * file is damaged. The reset itself stays in Settings, where the file is
 * explained; this only points there and can be dismissed.
 */
export function DamagedPrefsNotice({
  onOpenSettings,
  onDismiss,
}: {
  onOpenSettings: () => void
  onDismiss: () => void
}) {
  const { t } = useI18n()

  return (
    <div
      role="status"
      aria-live="polite"
      className="flex flex-wrap items-center gap-3 rounded-xl border border-[var(--color-warning)]/25 bg-[var(--color-warning-soft)] px-4 py-3 text-sm text-[var(--color-fg-secondary)]"
    >
      <p className="min-w-0 flex-1">{t('settings.prefs.unreadable')}</p>
      <Button variant="secondary" size="sm" onClick={onOpenSettings}>
        {t('app.prefsDamaged.open')}
      </Button>
      <Button variant="ghost" size="sm" onClick={onDismiss}>
        {t('app.prefsDamaged.dismiss')}
      </Button>
    </div>
  )
}
