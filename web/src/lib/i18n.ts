import en from '../locales/en.json' with { type: 'json' }
import el from '../locales/el.json' with { type: 'json' }

/** Web locale persist. Values are `en` | `el` only. No UiPrefs dual-write yet. */
export const LOCALE_STORAGE_KEY = 'oikonomia.locale'

export type Locale = 'en' | 'el'
export type MessageKey = keyof typeof en
export type TranslateVars = Record<string, string | number>

type JsonLeaf = string | { [key: string]: JsonLeaf }

/** Flatten Writer's nested catalog (and already-flat en.json) to dotted keys. */
export function flattenMessages(input: unknown, prefix = ''): Record<string, string> {
  if (typeof input === 'string') {
    return prefix ? { [prefix]: input } : {}
  }
  if (!input || typeof input !== 'object' || Array.isArray(input)) return {}
  const out: Record<string, string> = {}
  for (const [k, v] of Object.entries(input as JsonLeaf)) {
    const path = prefix ? `${prefix}.${k}` : k
    Object.assign(out, flattenMessages(v, path))
  }
  return out
}

/**
 * Current t() keys → Writer dotted paths. el.json is Writer-owned and nested;
 * en.json stays flat for pages Writer has not catalogued yet.
 */
const KEY_ALIASES: Record<string, string> = {
  'nav.dashboard': 'app.nav.dashboard',
  'nav.transactions': 'app.nav.transactions',
  'nav.documents': 'app.nav.documents',
  'nav.accounts': 'app.nav.accounts',
  'nav.reports': 'app.nav.reports',
  'nav.settings': 'app.nav.settings',
  'app.failedBackend': 'app.error.backend',
  'app.failedLock': 'app.error.lock',
  'common.loading': 'app.loading',
  'app.localLedger': 'app.brand.tagline',
  'app.book': 'app.book.label',
  'app.activeEntity': 'app.book.aria',
  'app.noEntitiesYet': 'app.book.emptyOption',
  'app.versionEncrypted': 'app.sidebar.versionEncrypted',
  'app.noBookSelected': 'app.header.noBook',
  'app.entityChart': 'app.header.chartMeta',
  'app.createEntityInSettings': 'app.header.createHint',
  'app.switchToLight': 'app.theme.ariaToLight',
  'app.switchToDark': 'app.theme.ariaToDark',
  'app.lightMode': 'app.theme.titleLight',
  'app.darkMode': 'app.theme.titleDark',
  'app.lockVault': 'app.lock.aria',
  'app.locking': 'app.lock.busy',
  'app.lock': 'app.lock.label',
  'chart.personal': 'app.chartTemplate.personal',
  'chart.company': 'app.chartTemplate.company',
  'chart.blank': 'app.chartTemplate.blank',

  'unlock.titleCreate': 'unlock.title.setup',
  'unlock.titleWelcome': 'unlock.title.unlock',
  'unlock.bodyCreate': 'unlock.body.setup',
  'unlock.bodyWelcome': 'unlock.body.unlock',
  'unlock.password': 'unlock.field.password',
  'unlock.confirmPassword': 'unlock.field.confirm',
  'unlock.createVault': 'unlock.submit.setup',
  'unlock.unlock': 'unlock.submit.unlock',
  'common.working': 'unlock.submit.busy',
  'unlock.restoreFromBackup': 'unlock.restore.link',
  'unlock.passwordsMismatch': 'unlock.error.mismatch',
  'unlock.incorrectPassword': 'unlock.error.badPassword',
  'unlock.unlockFailed': 'unlock.error.generic',

  'quickAdd.vaultLocked': 'quickAdd.locked.body',
  'common.open': 'quickAdd.locked.open',
  'quickAdd.saved': 'quickAdd.success.saved',
  'quickAdd.createBookFirst': 'quickAdd.empty.body',
  'kind.expense': 'quickAdd.kind.expense',
  'kind.income': 'quickAdd.kind.income',
  'kind.bill': 'quickAdd.kind.bill',
  'kind.transfer': 'quickAdd.kind.transfer',
  'kind.expenseShort': 'quickAdd.kind.expenseShort',
  'kind.incomeShort': 'quickAdd.kind.incomeShort',
  'kind.billShort': 'quickAdd.kind.billShort',
  'kind.transferShort': 'quickAdd.kind.transferShort',
  'common.next': 'quickAdd.next.aria',
  'common.back': 'quickAdd.back.aria',
  'quickAdd.book': 'quickAdd.book.aria',
  'quickAdd.moreBooks': 'quickAdd.book.moreSr',
  'quickAdd.more': 'quickAdd.book.moreOption',
  'quickAdd.entryType': 'quickAdd.type.aria',
  'quickAdd.amount': 'quickAdd.amount.aria',
  'quickAdd.category': 'quickAdd.accounts.category.label',
  'quickAdd.catPrefix': 'quickAdd.accounts.category.prefix',
  'quickAdd.income': 'quickAdd.accounts.income.label',
  'quickAdd.incPrefix': 'quickAdd.accounts.income.prefix',
  'quickAdd.wallet': 'quickAdd.accounts.wallet.label',
  'quickAdd.walletPrefix': 'quickAdd.accounts.wallet.prefix',
  'quickAdd.payable': 'quickAdd.accounts.payable.label',
  'quickAdd.payablePrefix': 'quickAdd.accounts.payable.prefix',
  'quickAdd.from': 'quickAdd.accounts.from.label',
  'quickAdd.fromPrefix': 'quickAdd.accounts.from.prefix',
  'quickAdd.to': 'quickAdd.accounts.to.label',
  'quickAdd.toPrefix': 'quickAdd.accounts.to.prefix',
  'quickAdd.billStatus': 'quickAdd.billStatus.aria',
  'quickAdd.due': 'quickAdd.billStatus.due',
  'quickAdd.paid': 'quickAdd.billStatus.paid',
  'quickAdd.memo': 'quickAdd.memo.placeholder',
  'quickAdd.drop': 'quickAdd.drop.hint',
  'common.cancel': 'quickAdd.save.cancel',
  'common.save': 'quickAdd.save.submit',
  'quickAdd.analyzing': 'quickAdd.analyze.busy',
  'quickAdd.fileTooLarge': 'quickAdd.error.fileTooLarge',
  'quickAdd.couldNotAnalyze': 'quickAdd.error.analyze',
  'quickAdd.noFilePath': 'quickAdd.error.noPath',
  'quickAdd.invalidAmount': 'quickAdd.error.invalidAmount',
  'quickAdd.pickAccounts': 'quickAdd.error.pickAccounts',
  'quickAdd.pickCategory': 'quickAdd.error.pickCategory',
  'quickAdd.pickWallet': 'quickAdd.error.pickWallet',
  'quickAdd.pickPayable': 'quickAdd.error.pickPayable',
  'quickAdd.pickDifferentAccounts': 'quickAdd.error.pickDifferent',

  'settings.eyebrow': 'settings.header.eyebrow',
  'settings.title': 'settings.header.title',
  'settings.description': 'settings.header.description',
  'settings.meta': 'settings.header.meta',
  'settings.autoLock.title': 'settings.lock.title',
  'settings.autoLock.description': 'settings.lock.description',
  'settings.lock.5min': 'settings.lock.preset.5min',
  'settings.lock.15min': 'settings.lock.preset.15min',
  'settings.lock.30min': 'settings.lock.preset.30min',
  'settings.lock.1hour': 'settings.lock.preset.1hour',
  'settings.lockTimeoutMin': 'settings.lock.error.min',
  'settings.masterPassword.title': 'settings.password.title',
  'settings.masterPassword.description': 'settings.password.description',
  'settings.currentPassword': 'settings.password.current',
  'settings.newPassword': 'settings.password.new',
  'settings.confirmNewPassword': 'settings.password.confirm',
  'settings.changePassword': 'settings.password.submit',
  'settings.reencrypting': 'settings.password.busy',
  'settings.passwordsMismatch': 'settings.password.error.mismatch',
  'settings.currentPasswordIncorrect': 'settings.password.error.badCurrent',
  'settings.changePasswordFailed': 'settings.password.error.generic',
  'settings.passwordChanged': 'settings.password.notice.changed',
  'settings.vaultBackup.title': 'settings.backup.title',
  'settings.vaultBackup.description': 'settings.backup.description',
  'settings.vaultBackup.body': 'settings.backup.body',
  'settings.vaultBackup.hint': 'settings.backup.hint',
  'settings.vaultBackup.backupVault': 'settings.backup.action.backup',
  'settings.vaultBackup.restore': 'settings.backup.action.restore',
  'settings.vaultBackup.emptyTitle': 'settings.backup.banner.empty.title',
  'settings.vaultBackup.emptyBody': 'settings.backup.banner.empty.body',
  'settings.vaultBackup.missingTitle': 'settings.backup.banner.missing.title',
  'settings.vaultBackup.missingBody': 'settings.backup.banner.missing.body',
  'settings.restore.loadTitle': 'settings.backup.restore.load.title',
  'settings.restore.loadBody': 'settings.backup.restore.load.body',
  'settings.restore.loadConfirm': 'settings.backup.restore.load.confirm',
  'settings.restore.replaceTitle': 'settings.backup.restore.replace.title',
  'settings.restore.replaceBody': 'settings.backup.restore.replace.body',
  'settings.restore.replaceConfirm': 'settings.backup.restore.replace.confirm',
  'settings.vaultBackup.errUninitialized': 'settings.backup.error.uninitialized',
  'settings.vaultBackup.errInvalid': 'settings.backup.error.invalid',
  'settings.vaultBackup.errOverwrite': 'settings.backup.error.wouldOverwrite',
  'settings.vaultBackup.errNotFound': 'settings.backup.error.notFound',
  'settings.vaultBackup.errIo': 'settings.backup.error.io',
  'settings.vaultBackup.errDefault': 'settings.backup.error.generic',
  'settings.entities.none': 'settings.entities.emptyCount',
  'settings.entities.oneBook': 'settings.entities.countOne',
  'settings.entities.nBooks': 'settings.entities.countMany',
  'settings.entities.empty': 'settings.entities.emptyBody',
  'common.delete': 'settings.entities.deleteTitle',
  'settings.deleteEntityTitle': 'settings.entities.deleteConfirm.title',
  'settings.deleteEntityBody': 'settings.entities.deleteConfirm.body',
  'settings.deleteFailed': 'settings.entities.deleteError',
  'settings.newEntity.title': 'settings.entityCreate.title',
  'settings.newEntity.description': 'settings.entityCreate.description',
  'settings.newEntity.name': 'settings.entityCreate.name',
  'settings.newEntity.namePlaceholder': 'settings.entityCreate.namePlaceholder',
  'settings.newEntity.currency': 'settings.entityCreate.currency',
  'settings.newEntity.chartTemplate': 'settings.entityCreate.chartLabel',
  'settings.template.personal.description': 'settings.entityCreate.template.personal.description',
  'settings.template.company.description': 'settings.entityCreate.template.company.description',
  'settings.template.blank.description': 'settings.entityCreate.template.blank.description',
  'settings.newEntity.create': 'settings.entityCreate.submit',
  'settings.newEntity.creating': 'settings.entityCreate.busy',
}

/** Writer path → current en.json key, for t() calls that use Writer paths in EN. */
const REVERSE_ALIASES: Record<string, string> = {
  'settings.entityCreate.template.personal.title': 'chart.personal',
  'settings.entityCreate.template.company.title': 'chart.company',
  'settings.entityCreate.template.blank.title': 'chart.blank',
  'settings.entityCreate.cancel': 'common.cancel',
  'quickAdd.memo.aria': 'quickAdd.memo',
  'quickAdd.empty.open': 'common.open',
}

for (const [from, to] of Object.entries(KEY_ALIASES)) {
  if (REVERSE_ALIASES[to] === undefined) REVERSE_ALIASES[to] = from
}

const catalogs: Record<Locale, Record<string, string>> = {
  en: flattenMessages(en),
  el: flattenMessages(el),
}

let current: Locale = 'en'
const listeners = new Set<() => void>()
let testMessages: Partial<Record<Locale, Record<string, string>>> = {}

export function parseLocale(value: unknown): Locale {
  return value === 'el' || value === 'en' ? value : 'en'
}

export function getLocale(): Locale {
  return current
}

export function subscribeLocale(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

function notify(): void {
  for (const listener of listeners) listener()
}

function writeCache(locale: Locale): void {
  try {
    localStorage.setItem(LOCALE_STORAGE_KEY, locale)
  } catch {
    /* private mode / quota */
  }
}

/** Read the optimistic cache. Invalid values become `en`. Missing → null. */
export function readCachedLocale(): Locale | null {
  try {
    const raw = localStorage.getItem(LOCALE_STORAGE_KEY)
    if (raw === null) return null
    return parseLocale(raw)
  } catch {
    return null
  }
}

function nonempty(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() !== '' ? value : undefined
}

function lookup(locale: Locale, key: string): string | undefined {
  const override = testMessages[locale]
  if (override) {
    const hit =
      nonempty(override[key]) ??
      nonempty(KEY_ALIASES[key] ? override[KEY_ALIASES[key]] : undefined) ??
      nonempty(REVERSE_ALIASES[key] ? override[REVERSE_ALIASES[key]] : undefined)
    if (hit) return hit
  }
  const cat = catalogs[locale]
  return (
    nonempty(cat?.[key]) ??
    nonempty(KEY_ALIASES[key] ? cat?.[KEY_ALIASES[key]] : undefined) ??
    nonempty(REVERSE_ALIASES[key] ? cat?.[REVERSE_ALIASES[key]] : undefined)
  )
}

/** Writer and extract catalogs use different interpolation names. */
function expandVars(vars?: TranslateVars): TranslateVars | undefined {
  if (!vars) return vars
  const out: TranslateVars = { ...vars }
  if (out.n === undefined && out.count !== undefined) out.n = out.count
  if (out.count === undefined && out.n !== undefined) out.count = out.n
  if (out.currency === undefined && out.ccy !== undefined) out.currency = out.ccy
  if (out.ccy === undefined && out.currency !== undefined) out.ccy = out.currency
  if (out.template === undefined && out.chart !== undefined) out.template = out.chart
  if (out.chart === undefined && out.template !== undefined) out.chart = out.template
  if (out.money === undefined && out.amount !== undefined) out.money = out.amount
  if (out.amount === undefined && out.money !== undefined) out.amount = out.money
  return out
}

function interpolate(template: string, vars?: TranslateVars): string {
  const expanded = expandVars(vars)
  if (!expanded) return template
  return template.replace(/\{(\w+)\}/g, (match, name: string) =>
    expanded[name] === undefined ? match : String(expanded[name]),
  )
}

/** Look up `key`. Empty/missing `el` values fall back to English. */
export function t(key: string, vars?: TranslateVars): string {
  const raw = lookup(current, key) ?? lookup('en', key) ?? key
  return interpolate(raw, vars)
}

/** Apply locale in memory and persist to `oikonomia.locale`. */
export function applyLocale(locale: Locale): void {
  const next = parseLocale(locale)
  if (next === current) {
    writeCache(next)
    return
  }
  current = next
  writeCache(next)
  notify()
}

/** Hydrate from localStorage. Missing or invalid → `en`. */
export function hydrateLocaleFromStorage(): Locale {
  const locale = readCachedLocale() ?? 'en'
  applyLocale(locale)
  return locale
}

/** Programmatic locale change. Writes `localStorage['oikonomia.locale']` only. */
export function setLocale(locale: Locale): void {
  applyLocale(locale)
}

/** Test-only catalog overlay. */
export function setLocaleMessagesForTests(
  locale: Locale,
  messages: Record<string, string> | null,
): void {
  if (messages === null) {
    delete testMessages[locale]
  } else {
    testMessages[locale] = messages
  }
  notify()
}

export function resetI18nForTests(): void {
  current = 'en'
  testMessages = {}
  try {
    localStorage.removeItem(LOCALE_STORAGE_KEY)
  } catch {
    /* ignore */
  }
  notify()
}
