import { openUrl } from '@tauri-apps/plugin-opener'
import { t } from '../lib/i18n'
import type { LicenseStatus } from '../lib/license'
import { Button } from './ui'

const SHOW_AT_DAYS = 7

export function TrialBanner({ license }: { license: LicenseStatus | null }) {
  if (!license) return null
  const expiring =
    license.state === 'trial' &&
    license.days_remaining != null &&
    license.days_remaining <= SHOW_AT_DAYS
  const expired = license.state === 'expired'
  if (!expiring && !expired) return null

  return (
    <div
      role="status"
      className="flex items-center justify-between gap-3 border-b border-[var(--color-border-strong)] bg-[var(--color-surface-elevated)] px-4 py-2 text-sm"
    >
      <span>
        {expired
          ? t('trial.banner.expired')
          : t('trial.banner.expiring', { n: license.days_remaining ?? 0 })}
      </span>
      {license.buy_url ? (
        <Button onClick={() => void openUrl(license.buy_url ?? '')}>
          {t('settings.license.buy')}
        </Button>
      ) : null}
    </div>
  )
}
