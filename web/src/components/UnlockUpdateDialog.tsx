import { useEffect, useId, useRef, type ReactNode } from 'react'
import { cn } from '../lib/cn'
import { ERROR_CODE_KEYS } from '../lib/commandError'
import type { Locale } from '../lib/i18n'
import { useI18n } from '../lib/I18nProvider'
import { updateCheck } from '../lib/tauri'
import {
  downloadPercent,
  formatDecimalMegabytes,
  pollsRefusedInstall,
  upToDateVersion,
  watchRefusedInstall,
  type ParsedIpcUpdate,
  type UpdateUiState,
} from '../lib/updateCheck'
import { UPDATE_DIALOG_BUTTON_WIDTHS } from '../lib/updateDialogButtons'
import { useDialogFocus } from './useDialogFocus'
import { Button } from './ui'

type DialogAction = { kind: 'secondary'; label: string } | { kind: 'primary'; label: string }

type Translate = (key: string, vars?: Record<string, string | number>) => string

type DialogRegions = {
  title: string
  body: ReactNode
  detail: ReactNode
  actions: DialogAction[]
}

type OpenUpdate = Exclude<UpdateUiState, { kind: 'idle' }>

type CopyInput = {
  t: Translate
  locale: Locale
  currentVersion: string | null
}

type ProgressUpdate = Extract<OpenUpdate, { kind: 'checking' | 'downloading' | 'installing' }>

type OfferUpdate = Extract<OpenUpdate, { kind: 'available' | 'availableManually' | 'upToDate' }>

type Props = {
  state: UpdateUiState
  appVersion: string | null
  onDismiss: () => void
  onInstall: () => void
  onResolve: (result: ParsedIpcUpdate) => void
  onCap: () => void
}

export function UnlockUpdateDialog({
  state,
  appVersion,
  onDismiss,
  onInstall,
  onResolve,
  onCap,
}: Props) {
  const { t, locale } = useI18n()
  const titleId = useId()
  const panelRef = useRef<HTMLDivElement>(null)
  const onResolveRef = useRef(onResolve)
  const onCapRef = useRef(onCap)
  const open = state.kind !== 'idle'
  const watchRefused = pollsRefusedInstall(state)

  useDialogFocus(panelRef, open, () => {
    if (state.kind !== 'installing' && state.kind !== 'idle') onDismiss()
  })

  useEffect(() => {
    onResolveRef.current = onResolve
    onCapRef.current = onCap
  }, [onResolve, onCap])

  // A local install is not watched. Cleanup drops a late answer.
  useEffect(() => {
    if (!watchRefused) return

    return watchRefusedInstall(
      updateCheck,
      (result) => {
        onResolveRef.current(result)
      },
      () => {
        onCapRef.current()
      },
    )
  }, [watchRefused])

  if (state.kind === 'idle') return null

  const copy = dialogCopy(state, {
    t,
    locale,
    currentVersion: upToDateVersion(appVersion),
  })
  const widths = UPDATE_DIALOG_BUTTON_WIDTHS[locale]

  return (
    <div
      className={cn(
        'update-dialog-fade absolute inset-0 z-40 flex items-center justify-center',
        'bg-[rgba(0,0,0,0.5)] px-4',
      )}
    >
      <div
        ref={panelRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        className="glass-dialog update-dialog w-full max-w-md rounded-[24px] p-6 outline-none"
      >
        <h2
          id={titleId}
          data-update-region="title"
          className="h-7 text-xl leading-7 font-semibold text-[var(--color-fg)]"
        >
          {copy.title}
        </h2>
        <div
          data-update-region="body"
          className="mt-2 h-10 text-sm leading-5 text-[var(--color-muted)]"
        >
          {copy.body}
        </div>
        <div data-update-region="detail" className="mt-4 h-10 overflow-hidden">
          {copy.detail}
        </div>
        <div data-update-region="actions" className="mt-6 flex h-10 justify-end gap-2">
          {copy.actions.map((action) => (
            <Button
              key={action.label}
              variant={action.kind === 'primary' ? 'primary' : 'secondary'}
              className="shrink-0 whitespace-nowrap"
              style={{ width: action.kind === 'primary' ? widths.primary : widths.secondary }}
              onClick={action.kind === 'primary' ? onInstall : onDismiss}
            >
              {action.label}
            </Button>
          ))}
        </div>
      </div>
    </div>
  )
}

function dialogCopy(state: OpenUpdate, input: CopyInput): DialogRegions {
  if (state.kind === 'failed') return failedCopy(input.t, state.code)
  if (state.kind === 'updateError') return updateErrorCopy(input.t)
  if (isProgressFrame(state)) return progressCopy(state, input)
  return offerCopy(state, input)
}

function isProgressFrame(state: OpenUpdate): state is ProgressUpdate {
  return (
    state.kind === 'checking' || state.kind === 'downloading' || state.kind === 'installing'
  )
}

function progressCopy(state: ProgressUpdate, input: CopyInput): DialogRegions {
  switch (state.kind) {
    case 'checking':
      return checkingCopy(input.t)
    case 'downloading':
      return downloadingCopy(state, input)
    case 'installing':
      return installingCopy(input.t)
  }
}

function offerCopy(state: OfferUpdate, input: CopyInput): DialogRegions {
  switch (state.kind) {
    case 'available':
      return availableCopy(input.t, state.version)
    case 'availableManually':
      return manualCopy(input.t, state.version)
    case 'upToDate':
      return upToDateCopy(input.t, input.currentVersion)
  }
}

function checkingCopy(t: Translate): DialogRegions {
  return {
    title: t('unlock.update.checking.title'),
    body: <p>{t('unlock.update.checking.body')}</p>,
    detail: (
      <>
        <div className="h-5" />
        <UpdateProgressBar
          mode="indeterminate"
          percent={null}
          label={t('unlock.update.checking.title')}
        />
      </>
    ),
    actions: [{ kind: 'secondary', label: t('unlock.update.cancel') }],
  }
}

function upToDateCopy(t: Translate, currentVersion: string | null): DialogRegions {
  return {
    title: t('unlock.update.upToDate.title'),
    body: <p>{t('unlock.update.upToDate.body')}</p>,
    detail: currentVersion ? (
      <p
        className={cn(
          'h-5 text-[13px] leading-5 font-medium',
          'text-[var(--color-fg-secondary)] tabular-nums',
        )}
      >
        {t('unlock.update.available.version', { version: currentVersion })}
      </p>
    ) : null,
    actions: [{ kind: 'secondary', label: t('unlock.update.close') }],
  }
}

function availableCopy(t: Translate, version: string): DialogRegions {
  return {
    title: t('unlock.update.available.title'),
    body: versionWithEmptyLine(t('unlock.update.available.version', { version })),
    detail: (
      <p className="line-clamp-2 text-[13px] leading-5 text-[var(--color-muted)]">
        {t('unlock.update.available.honesty')}
      </p>
    ),
    actions: [
      { kind: 'secondary', label: t('unlock.update.available.later') },
      { kind: 'primary', label: t('unlock.update.available.confirm') },
    ],
  }
}

function manualCopy(t: Translate, version: string): DialogRegions {
  return {
    title: t('unlock.update.available.title'),
    body: versionWithEmptyLine(t('unlock.update.available.version', { version })),
    detail: (
      <p
        data-update-slot="manual-note"
        className="text-[13px] leading-5 text-[var(--color-muted)]"
      >
        {t('unlock.update.availableManually.note')}
      </p>
    ),
    actions: [{ kind: 'secondary', label: t('unlock.update.close') }],
  }
}

function downloadingCopy(
  state: Extract<ProgressUpdate, { kind: 'downloading' }>,
  input: CopyInput,
): DialogRegions {
  return {
    title: input.t('unlock.update.downloading.title'),
    body: versionWithEmptyLine(
      input.t('unlock.update.available.version', { version: state.version }),
    ),
    detail: (
      <DownloadDetail
        received={state.received}
        total={state.total}
        t={input.t}
        locale={input.locale}
      />
    ),
    actions: [{ kind: 'secondary', label: input.t('unlock.update.cancel') }],
  }
}

function failedCopy(t: Translate, code: string | undefined): DialogRegions {
  return {
    title: t('unlock.update.failed.title'),
    body: <p>{updateFailureBody(code, t)}</p>,
    detail: null,
    actions: [{ kind: 'secondary', label: t('unlock.update.close') }],
  }
}

function updateErrorCopy(t: Translate): DialogRegions {
  return {
    title: t('unlock.update.failed.title'),
    body: <p>{t('error.update')}</p>,
    detail: null,
    actions: [{ kind: 'secondary', label: t('unlock.update.close') }],
  }
}

function installingCopy(t: Translate): DialogRegions {
  return {
    title: t('unlock.update.installing.title'),
    body: <p>{t('unlock.update.installing.body')}</p>,
    detail: (
      <>
        <p className="h-5 text-[13px] leading-5 text-[var(--color-muted)]">
          {t('unlock.update.installing.restartNote')}
        </p>
        <UpdateProgressBar mode="full" percent={100} label={t('unlock.update.installing.title')} />
      </>
    ),
    actions: [],
  }
}

/**
 * The sentence under "Couldn't check": the copy mapped from the failure's
 * code, or the dialog's own sentence when this build has no sentence for it.
 *
 * Read from the map here, not through `commandErrorMessage`: that logs, and
 * this runs on every render of the dialog.
 */
function updateFailureBody(code: string | undefined, t: Translate): string {
  const fallback = t('unlock.update.failed.body')
  if (code === undefined || !Object.hasOwn(ERROR_CODE_KEYS, code)) return fallback

  const key = ERROR_CODE_KEYS[code]
  const copy = t(key)
  return copy === key ? fallback : copy
}

function barNow(
  mode: 'indeterminate' | 'determinate' | 'full',
  percent: number | null,
): number | undefined {
  if (mode === 'indeterminate') return undefined
  if (mode === 'full') return 100
  return percent ?? 0
}

function UpdateProgressBar({
  mode,
  percent,
  label,
}: {
  mode: 'indeterminate' | 'determinate' | 'full'
  percent: number | null
  label: string
}) {
  return (
    <div
      className="mt-4 h-1 overflow-hidden rounded-full bg-white/10"
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={barNow(mode, percent)}
      data-bar={mode}
      data-update-region="bar"
    >
      <div
        className={cn(
          'h-full rounded-full bg-[var(--color-accent)]',
          mode === 'indeterminate' && 'oik-indeterminate-fill',
          mode === 'determinate' && 'oik-determinate-fill',
          mode === 'full' && 'w-full',
        )}
        style={mode === 'determinate' ? { width: `${percent ?? 0}%` } : undefined}
      />
    </div>
  )
}

function DownloadDetail({
  received,
  total,
  t,
  locale,
}: {
  received: number
  total: number | null
  t: Translate
  locale: Locale
}) {
  const done = formatDecimalMegabytes(received, locale)
  const percent = downloadPercent(received, total)
  const left =
    total === null
      ? t('unlock.update.downloading.unknownSize', { done })
      : t('unlock.update.downloading.progress', {
          done,
          total: formatDecimalMegabytes(total, locale),
        })

  return (
    <>
      <div
        className={cn(
          'flex h-5 items-baseline justify-between text-[13px] leading-5',
          'text-[var(--color-muted)] tabular-nums',
        )}
      >
        <span>{left}</span>
        {percent === null ? null : (
          <span>{t('unlock.update.downloading.percent', { percent })}</span>
        )}
      </div>
      <UpdateProgressBar
        mode={percent === null ? 'indeterminate' : 'determinate'}
        percent={percent}
        label={t('unlock.update.downloading.title')}
      />
    </>
  )
}

function versionLine(version: string) {
  return <p className="font-medium text-[var(--color-fg-secondary)]">{version}</p>
}

/** The notes line stays empty. Feed notes are stripped, and the slot stays two lines tall. */
function versionWithEmptyLine(version: string) {
  return (
    <>
      {versionLine(version)}
      <p className="h-5" />
    </>
  )
}
