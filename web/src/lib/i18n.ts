import en from '../locales/en.json' with { type: 'json' }
import el from '../locales/el.json' with { type: 'json' }
import fr from '../locales/fr.json' with { type: 'json' }
import de from '../locales/de.json' with { type: 'json' }

/** Optimistic mirror only. Durable store is UiPrefs.locale. */
export const LOCALE_STORAGE_KEY = 'oikonomia.locale'

export type Locale = 'en' | 'el' | 'fr' | 'de'
export const LOCALES: readonly Locale[] = ['en', 'el', 'fr', 'de']
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
export const KEY_ALIASES: Record<string, string> = {
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
  'app.noEntitiesYet': 'app.book.emptyOption',
  'app.versionEncrypted': 'app.sidebar.versionEncrypted',
  'app.noBookSelected': 'app.header.noBook',
  'app.entityChart': 'app.header.chartMeta',
  'app.createEntityInSettings': 'app.header.createHint',
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
  'kind.other': 'entry.kind.other',
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

  'settings.title': 'settings.header.title',
  'settings.description': 'settings.header.description',
  'settings.autoLock.title': 'settings.lock.title',
  'settings.autoLock.description': 'settings.lock.description',
  'settings.lock.5min': 'settings.lock.preset.5min',
  'settings.lock.15min': 'settings.lock.preset.15min',
  'settings.lock.30min': 'settings.lock.preset.30min',
  'settings.lock.1hour': 'settings.lock.preset.1hour',
  'settings.lockTimeoutMin': 'settings.lock.error.min',
  'settings.lock.saved': 'settings.lock.notice.saved',
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
  'settings.vaultBackup.errRestoreDefault': 'settings.backup.error.restoreGeneric',
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

  'common.confirm': 'dialog.confirm.default',
  'common.close': 'dialog.modal.closeAria',
  'date.openCalendar': 'date.openCalendar.aria',
  'date.calendar': 'date.openCalendar.title',
  'date.prevMonth': 'date.prevMonth.aria',
  'date.nextMonth': 'date.nextMonth.aria',
  'date.weekday.mo': 'date.weekday.mon',
  'date.weekday.tu': 'date.weekday.tue',
  'date.weekday.we': 'date.weekday.wed',
  'date.weekday.th': 'date.weekday.thu',
  'date.weekday.fr': 'date.weekday.fri',
  'date.weekday.sa': 'date.weekday.sat',
  'date.weekday.su': 'date.weekday.sun',
  'date.month.1': 'date.month.01',
  'date.month.2': 'date.month.02',
  'date.month.3': 'date.month.03',
  'date.month.4': 'date.month.04',
  'date.month.5': 'date.month.05',
  'date.month.6': 'date.month.06',
  'date.month.7': 'date.month.07',
  'date.month.8': 'date.month.08',
  'date.month.9': 'date.month.09',
  'currency.EUR': 'currency.eur.label',
  'currency.USD': 'currency.usd.label',
  'currency.GBP': 'currency.gbp.label',
  'currency.CHF': 'currency.chf.label',
  'currency.JPY': 'currency.jpy.label',
  'currency.CAD': 'currency.cad.label',
  'currency.AUD': 'currency.aud.label',
  'currency.SEK': 'currency.sek.label',
  'currency.NOK': 'currency.nok.label',
  'currency.DKK': 'currency.dkk.label',
  'currency.PLN': 'currency.pln.label',
  'currency.CZK': 'currency.czk.label',
  'currency.HUF': 'currency.huf.label',
  'currency.TRY': 'currency.try.label',
  'currency.INR': 'currency.inr.label',
  'currency.CNY': 'currency.cny.label',
  'currency.BRL': 'currency.brl.label',
  'currency.RON': 'currency.ron.label',

  'dash.createTitle': 'dashboard.empty.title',
  'dash.createBody': 'dashboard.empty.body',
  'dash.month': 'dashboard.period.month',
  'dash.year': 'dashboard.period.year',
  'dash.period.month': 'dashboard.period.word.month',
  'dash.period.year': 'dashboard.period.word.year',
  'dash.netThis': 'dashboard.hero.netLabel',
  'dash.entriesThis.one': 'dashboard.hero.entryCount.one',
  'dash.entriesThis.other': 'dashboard.hero.entryCount.other',
  'dash.recentActivity': 'dashboard.activity.title',
  'dash.noEntries': 'dashboard.activity.empty',

  'tx.noBookTitle': 'common.emptyBook.title',
  'tx.noBookBody': 'common.emptyBook.body',
  'tx.title': 'tx.header.title',
  'tx.search': 'tx.search.label',
  'tx.searchPlaceholder': 'tx.search.placeholder',
  'tx.from': 'tx.filter.from',
  'tx.to': 'tx.filter.to',
  'tx.account': 'tx.filter.account',
  'tx.filterFrom': 'tx.filter.fromAria',
  'tx.filterTo': 'tx.filter.toAria',
  'tx.allAccounts': 'tx.filter.allAccounts',
  'tx.newTitle': 'tx.form.titleNew',
  'tx.editTitle': 'tx.form.titleEdit',
  'tx.newDescription': 'tx.form.descNew',
  'tx.editDescription': 'tx.form.descEdit',
  'tx.docWillStore': 'tx.form.scan.willStore',
  'tx.docSuggested': 'tx.form.scan.suggested',
  'tx.docReview': 'tx.form.scan.review',
  'tx.date': 'tx.form.date',
  'tx.entryDate': 'tx.form.dateAria',
  'tx.amount': 'tx.form.amount',
  'tx.amountPlaceholder': 'tx.form.amountPlaceholder',
  'tx.reference': 'tx.form.reference',
  'tx.referencePlaceholder': 'tx.form.referencePlaceholder',
  'tx.descriptionLabel': 'tx.form.description',
  'tx.descPlaceholder.expense': 'tx.form.descPlaceholder.expense',
  'tx.descPlaceholder.income': 'tx.form.descPlaceholder.income',
  'tx.descPlaceholder.bill': 'tx.form.descPlaceholder.bill',
  'tx.descPlaceholder.transfer': 'tx.form.descPlaceholder.transfer',
  'tx.categoryWhatFor': 'tx.form.categoryWhatFor',
  'tx.paidFrom': 'tx.form.paidFrom',
  'tx.incomeType': 'tx.form.incomeType',
  'tx.receivedInto': 'tx.form.receivedInto',
  'tx.billStatus': 'tx.form.billStatus.label',
  'tx.billPaidNow': 'tx.form.billStatus.paid',
  'tx.billUnpaid': 'tx.form.billStatus.unpaid',
  'tx.billPayExisting': 'tx.form.billStatus.payExisting',
  'tx.billCategory': 'tx.form.billCategory',
  'tx.payFrom': 'tx.form.payFrom',
  'tx.billsPayableAccount': 'tx.form.payableAccount',
  'tx.noLiability': 'tx.form.noLiability',
  'tx.billsPayableTip': 'tx.form.payableTip',
  'tx.saveEntry': 'tx.form.save',
  'tx.saveChanges': 'tx.form.saveChanges',
  'common.saving': 'tx.form.busy',
  'tx.invalidAmount': 'tx.form.error.amount',
  'tx.deleteTitle': 'tx.void.title',
  'tx.deleteBody': 'tx.void.body',
  'tx.deleteFailed': 'tx.void.error',
  'tx.deleteEntry': 'tx.void.aria',
  'tx.allEntries': 'tx.list.title',
  'tx.newEntry': 'tx.list.new',
  'tx.emptyTitle': 'tx.empty.title',
  'tx.emptyBody': 'tx.empty.body',
  'tx.noMatchTitle': 'tx.empty.filtered.title',
  'tx.noMatchBody': 'tx.empty.filtered.body',
  'tx.hasDocument': 'tx.row.hasDocument',
  'tx.couldNotReadDoc': 'dropzone.error.unreadable',

  'acct.noBookTitle': 'common.emptyBook.title',
  'acct.noBookBody': 'common.emptyBook.body',
  'acct.title': 'accounts.header.title',
  'acct.close': 'accounts.add.close',
  'acct.addAccount': 'accounts.add.label',
  'acct.activeAccounts': 'accounts.metric.active',
  'acct.inThisBook': 'accounts.metric.activeHint',
  'acct.assets': 'accounts.metric.assets',
  'acct.assetsHint': 'accounts.metric.assetsHint',
  'acct.income': 'accounts.metric.income',
  'acct.incomeHint': 'accounts.metric.incomeHint',
  'acct.expenses': 'accounts.metric.expenses',
  'acct.expensesHint': 'accounts.metric.expensesHint',
  'accountType.asset': 'accounts.type.asset',
  'accountType.liability': 'accounts.type.liability',
  'accountType.equity': 'accounts.type.equity',
  'accountType.income': 'accounts.type.income',
  'accountType.expense': 'accounts.type.expense',
  'acct.newAccount': 'accounts.form.title',
  'acct.newAccountHint': 'accounts.form.subtitle',
  'acct.code': 'accounts.form.code',
  'acct.codePlaceholder': 'accounts.form.codePlaceholder',
  'acct.name': 'accounts.form.name',
  'acct.namePlaceholder': 'accounts.form.namePlaceholder',
  'acct.type': 'accounts.form.type',
  'acct.createAccount': 'accounts.form.submit',
  'acct.setBalanceNamed': 'accounts.balance.title',
  'acct.setBalance': 'accounts.balance.titleFallback',
  'acct.setBalanceDesc': 'accounts.balance.description',
  'acct.ledgerBalanceToday': 'accounts.balance.ledgerToday',
  'acct.actualBalance': 'accounts.balance.actual',
  'acct.asOf': 'accounts.balance.asOf',
  'acct.balanceAsOf': 'accounts.balance.asOfAria',
  'acct.posting': 'accounts.balance.busy',
  'acct.invalidAmount': 'accounts.balance.error.invalid',
  'acct.setBalanceAria': 'accounts.balance.aria',
  'acct.deactivateAria': 'accounts.deactivate.aria',
  'acct.deactivate': 'accounts.deactivate.title',
  'common.system': 'accounts.badge.system',
  'common.active': 'accounts.status.active',
  'common.inactive': 'accounts.status.inactive',
  'acct.chartTitle': 'accounts.list.title',
  'acct.chartDesc': 'accounts.list.description',
  'acct.noAccountsTitle': 'accounts.empty.title',
  'acct.noAccountsBody': 'accounts.empty.body',

  'rpt.noBookTitle': 'common.emptyBook.title',
  'rpt.noBookBody': 'common.emptyBook.body',
  'rpt.title': 'reports.header.title',
  'rpt.pnl': 'reports.tab.pnl',
  'rpt.balanceSheet': 'reports.tab.bs',
  'rpt.trialBalance': 'reports.tab.trial',
  'rpt.from': 'reports.filter.from',
  'rpt.to': 'reports.filter.to',
  'rpt.asOf': 'reports.filter.asOf',
  'rpt.fromDate': 'reports.filter.fromAria',
  'rpt.toDate': 'reports.filter.toAria',
  'rpt.asOfDate': 'reports.filter.asOfAria',
  'rpt.amountsIn': 'reports.statement.amountsIn',
  'rpt.profitLoss': 'reports.pnl.title',
  'rpt.income': 'reports.pnl.income',
  'rpt.expenses': 'reports.pnl.expenses',
  'rpt.noIncome': 'reports.pnl.emptyIncome',
  'rpt.noExpenses': 'reports.pnl.emptyExpenses',
  'rpt.totalIncome': 'reports.pnl.totalIncome',
  'rpt.totalExpenses': 'reports.pnl.totalExpenses',
  'rpt.netIncome': 'reports.pnl.netIncome',
  'rpt.expenseBreakdown': 'reports.pnl.donutTitle',
  'rpt.expenseBreakdownDesc': 'reports.pnl.donutDescription',
  'donut.noExpenses': 'reports.donut.empty',
  'rpt.asOfPeriod': 'reports.bs.asOf',
  'rpt.noSectionAccounts': 'reports.bs.emptySection',
  'rpt.totalSection': 'reports.bs.totalSection',
  'rpt.totalAssets': 'reports.bs.totalAssets',
  'rpt.totalLiabEquity': 'reports.bs.totalLiabEquity',
  'rpt.booksBalance': 'reports.bs.balanced',
  'rpt.outOfBalance': 'reports.bs.unbalanced',
  'rpt.account': 'reports.trial.account',
  'rpt.debit': 'reports.trial.debit',
  'rpt.credit': 'reports.trial.credit',
  'rpt.total': 'reports.trial.total',
  'rpt.section.assets': 'reports.section.assets',
  'rpt.section.liabilities': 'reports.section.liabilities',
  'rpt.section.equity': 'reports.section.equity',

  'docs.noBookTitle': 'common.emptyBook.title',
  'docs.noBookBody': 'common.emptyBook.body',
  'docs.title': 'documents.header.title',
  'docs.emptyTitle': 'documents.empty.title',
  'docs.emptyBody': 'documents.empty.body',
  'docs.vaultFiles': 'documents.list.title',
  'docs.vaultFilesDesc': 'documents.list.description',
  'docs.linkedEntry': 'documents.linkedEntry.fallback',
  'docs.viewAria': 'documents.view.aria',
  'docs.view': 'documents.view.title',
  'docs.saveCopyAria': 'documents.export.aria',
  'docs.saveCopy': 'documents.export.title',
  'viewer.saving': 'documents.export.busy',
  'docs.deleteAria': 'documents.delete.aria',
  'docs.deleteTitle': 'documents.deleteConfirm.title',
  'docs.deleteBody': 'documents.deleteConfirm.body.list',
  'docs.deleteFailed': 'documents.error.delete',
  'docs.exportFailed': 'documents.error.export',
  'viewer.document': 'documents.viewer.fallbackTitle',
  'viewer.noPreview': 'documents.viewer.noPreview',
  'viewer.openFailed': 'documents.viewer.openError',
  'viewer.saveCopy': 'documents.export.title',
  'viewer.exportFailed': 'documents.error.export',
  'entry.unknownAccount': 'documents.detail.unknownAccount',
  'entry.account': 'documents.detail.table.account',
  'entry.debit': 'documents.detail.table.debit',
  'entry.credit': 'documents.detail.table.credit',
  'entry.memo': 'documents.detail.table.memo',
  'entry.attachments': 'documents.detail.attachments',
  'entry.attachFile': 'documents.detail.attach',
  'entry.noFiles': 'documents.detail.noFiles',
  'entry.edit': 'documents.detail.edit',
  'entry.ref': 'documents.detail.ref',
  'entry.history.title': 'documents.detail.history.title',
  'entry.history.note': 'documents.detail.history.note',
  'entry.history.original': 'documents.detail.history.original',
  'entry.history.reversal': 'documents.detail.history.reversal',
  'entry.history.replacement': 'documents.detail.history.replacement',
  'entry.history.error': 'documents.detail.history.error',
  'entry.attachFailed': 'documents.detail.attachError',
  'entry.updateFailed': 'documents.detail.updateError',
  'entry.deleteDocTitle': 'documents.deleteConfirm.title',
  'entry.deleteDocBody': 'documents.deleteConfirm.body.detail',
  'entry.deleteFailed': 'documents.error.delete',
  'entry.exportFailed': 'documents.error.export',
  'entry.fileTooLarge': 'common.error.fileTooLarge',
  'drop.fileTooLarge': 'common.error.fileTooLarge',
  'drop.analyzing': 'dropzone.title.busy',
  'drop.title': 'dropzone.title.idle',
  'drop.body': 'dropzone.body',
  'drop.statusUnavailable': 'dropzone.statusUnavailable',
  'drop.analyzeFailed': 'dropzone.error.analyze',
  'drop.noPath': 'dropzone.error.noPathTauri',
  'drop.noFile': 'dropzone.error.noFileBrowser',
}

/** Writer path → current en.json key, for t() calls that use Writer paths in EN. */
export const REVERSE_ALIASES: Record<string, string> = {
  'settings.entityCreate.template.personal.title': 'chart.personal',
  'settings.entityCreate.template.company.title': 'chart.company',
  'settings.entityCreate.template.blank.title': 'chart.blank',
}

for (const [from, to] of Object.entries(KEY_ALIASES)) {
  if (REVERSE_ALIASES[to] === undefined) REVERSE_ALIASES[to] = from
}

const catalogs: Record<Locale, Record<string, string>> = {
  en: flattenMessages(en),
  el: flattenMessages(el),
  fr: flattenMessages(fr),
  de: flattenMessages(de),
}

let current: Locale = 'en'
const listeners = new Set<() => void>()
let testMessages: Partial<Record<Locale, Record<string, string>>> = {}

export function parseLocale(value: unknown): Locale {
  switch (value) {
    case 'en':
    case 'el':
    case 'fr':
    case 'de':
      return value
    default:
      return 'en'
  }
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

/** Test-only: true when `key` resolves in `locale` without the en fallback. */
export function resolvesInLocale(locale: Locale, key: string): boolean {
  return lookup(locale, key) !== undefined
}

/** Look up `key`. Empty/missing `el` values fall back to English. */
export function t(key: string, vars?: TranslateVars): string {
  const raw = lookup(current, key) ?? lookup('en', key) ?? key
  return interpolate(raw, vars)
}

/**
 * Look up the plural form of `key` for `count`: `<key>.one`, `<key>.other`
 * and so on, by the CLDR rules of the app language, falling back to
 * `<key>.other`. `{count}` (and `{n}`) are filled in.
 */
export function tn(key: string, count: number, vars?: TranslateVars): string {
  const category = new Intl.PluralRules(current).select(count)
  const specific = `${key}.${category}`
  const found = lookup(current, specific) ?? lookup('en', specific)

  return t(found === undefined ? `${key}.other` : specific, { ...vars, count })
}

/** Apply locale in memory and mirror `oikonomia.locale`. Does not write UiPrefs. */
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

/** Hydrate the optimistic mirror. Missing or invalid → `en`. */
export function hydrateLocaleFromStorage(): Locale {
  const locale = readCachedLocale() ?? 'en'
  applyLocale(locale)
  return locale
}

type PersistFn = (locale: Locale) => void | Promise<void>
let persistLocale: PersistFn | null = null

/** Wire durable persist (`settings_set_locale`). Tests inject a mock. */
export function setLocalePersist(fn: PersistFn | null): void {
  persistLocale = fn
}

/**
 * Programmatic locale change. Mirrors localStorage, then persists via
 * `settings_set_locale` when a writer is wired.
 */
export function setLocale(locale: Locale): void {
  applyLocale(locale)
  void persistLocale?.(locale)
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
  persistLocale = null
  try {
    localStorage.removeItem(LOCALE_STORAGE_KEY)
  } catch {
    /* ignore */
  }
  notify()
}
