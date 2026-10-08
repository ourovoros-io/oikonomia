import { cn } from '../lib/cn'
import type { Locale } from '../lib/api'
import { useI18n } from '../lib/I18nProvider'
import { LOCALES } from '../lib/i18n'

/** Language pills: one row of equal-width segments (fr columns under w-fit size to the widest autonym). */
export function LanguagePill({
  value,
  onChange,
  ariaLabel,
}: {
  value: Locale
  onChange: (locale: Locale) => void
  ariaLabel: string
}) {
  const { t } = useI18n()
  return (
    <div
      role="radiogroup"
      aria-label={ariaLabel}
      className="grid h-10 w-fit grid-flow-col auto-cols-fr gap-0.5 rounded-full bg-white/[0.04] p-1 shadow-[inset_0_0_0_1px_rgba(255,255,255,0.08)]"
    >
      {LOCALES.map((id) => {
        const active = value === id
        return (
          <button
            key={id}
            type="button"
            role="radio"
            aria-checked={active}
            onClick={() => onChange(id)}
            className={cn(
              // Same look as the shared Segmented control.
              // The ring is drawn inside the segment: outside it would overflow the group.
              'inline-flex h-8 w-full items-center justify-center rounded-full px-3 text-[13px] font-medium transition focus-visible:outline-offset-[-2px]',
              active
                ? 'bg-white/10 text-[var(--color-fg)] shadow-[inset_0_1px_0_rgba(255,255,255,0.12)]'
                : 'text-[var(--color-muted)] hover:text-[var(--color-fg)]',
            )}
          >
            {t(`settings.language.option.${id}`)}
          </button>
        )
      })}
    </div>
  )
}
