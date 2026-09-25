import type {
  Account,
  AccountType,
  DocumentMeta,
  Entity,
  PostedEntryView,
  RecurringTemplate,
} from '../../src/lib/api'

export type DemoLang = 'en' | 'el'

/** The day every capture pretends is today. Nothing in the ledger is later. */
export const DEMO_TODAY = '2026-09-24'

export type DemoLedger = {
  lang: DemoLang
  entity: Entity
  accounts: Account[]
  entries: PostedEntryView[]
  documents: DocumentMeta[]
  recurring: RecurringTemplate[]
}

type Named = { en: string; el: string }

type AccountSeed = { code: string; type: AccountType; name: Named }

const ENTITY_ID = 'demo-entity'

const ACCOUNTS: AccountSeed[] = [
  { code: '1000', type: 'asset', name: { en: 'Cash drawer', el: 'Ταμείο' } },
  { code: '1020', type: 'asset', name: { en: 'Business account', el: 'Επαγγελματικός λογαριασμός' } },
  { code: '2000', type: 'liability', name: { en: 'Suppliers', el: 'Προμηθευτές' } },
  { code: '3000', type: 'equity', name: { en: 'Owner\'s capital', el: 'Κεφάλαιο' } },
  { code: '4000', type: 'income', name: { en: 'Studio sales', el: 'Πωλήσεις εργαστηρίου' } },
  { code: '4010', type: 'income', name: { en: 'Pottery classes', el: 'Μαθήματα κεραμικής' } },
  { code: '5000', type: 'expense', name: { en: 'Clay and glaze', el: 'Πηλός και σμάλτα' } },
  { code: '5010', type: 'expense', name: { en: 'Kiln electricity', el: 'Ρεύμα κλιβάνου' } },
  { code: '5020', type: 'expense', name: { en: 'Studio rent', el: 'Ενοίκιο εργαστηρίου' } },
  { code: '5030', type: 'expense', name: { en: 'Insurance', el: 'Ασφάλιση' } },
  { code: '5040', type: 'expense', name: { en: 'Packaging and shipping', el: 'Συσκευασία και αποστολές' } },
]

type Kind = 'income' | 'expense'

/** One recurring shape per month. `amount` is in cents and varies by month index. */
type Pattern = {
  day: number
  kind: Kind
  account: string
  wallet: '1000' | '1020'
  amount: (month: number) => number
  description: Named
  receipt?: string
}

const PATTERNS: Pattern[] = [
  {
    day: 1,
    kind: 'expense',
    account: '5020',
    wallet: '1020',
    amount: () => 65000,
    description: { en: 'Studio rent', el: 'Ενοίκιο εργαστηρίου' },
  },
  {
    day: 3,
    kind: 'income',
    account: '4010',
    wallet: '1020',
    amount: (m) => 48000 + m * 6000,
    description: { en: 'Wheel-throwing class, evening group', el: 'Μάθημα τροχού, βραδινό τμήμα' },
  },
  {
    day: 5,
    kind: 'expense',
    account: '5000',
    wallet: '1020',
    amount: (m) => 21840 + m * 1250,
    description: { en: 'Stoneware clay, 10 bags', el: 'Πηλός stoneware, 10 σακιά' },
    receipt: 'clay',
  },
  {
    day: 7,
    kind: 'income',
    account: '4000',
    wallet: '1000',
    amount: (m) => 31500 + m * 2200,
    description: { en: 'Market stall, Saturday', el: 'Πάγκος στην αγορά, Σάββατο' },
  },
  {
    day: 9,
    kind: 'expense',
    account: '5040',
    wallet: '1020',
    amount: (m) => 4680 + m * 310,
    description: { en: 'Shipping boxes and paper', el: 'Κούτες και χαρτί αποστολής' },
  },
  {
    day: 11,
    kind: 'income',
    account: '4000',
    wallet: '1020',
    amount: (m) => 86000 + m * 9500,
    description: { en: 'Tableware order, café in Ladadika', el: 'Παραγγελία σερβίτσιων, καφέ στα Λαδάδικα' },
  },
  {
    day: 12,
    kind: 'expense',
    account: '5010',
    wallet: '1020',
    amount: (m) => 13920 + m * 840,
    description: { en: 'Kiln electricity bill', el: 'Λογαριασμός ρεύματος κλιβάνου' },
    receipt: 'power',
  },
  {
    day: 14,
    kind: 'income',
    account: '4000',
    wallet: '1000',
    amount: (m) => 27400 + m * 1800,
    description: { en: 'Market stall, Saturday', el: 'Πάγκος στην αγορά, Σάββατο' },
  },
  {
    day: 15,
    kind: 'expense',
    account: '5030',
    wallet: '1020',
    amount: () => 9500,
    description: { en: 'Studio insurance', el: 'Ασφάλιση εργαστηρίου' },
  },
  {
    day: 17,
    kind: 'income',
    account: '4010',
    wallet: '1020',
    amount: (m) => 36000 + m * 4000,
    description: { en: 'Weekend workshop', el: 'Εργαστήριο Σαββατοκύριακου' },
  },
  {
    day: 19,
    kind: 'expense',
    account: '5000',
    wallet: '1020',
    amount: (m) => 12760 + m * 900,
    description: { en: 'Glaze pigments', el: 'Χρωστικές σμάλτων' },
    receipt: 'glaze',
  },
  {
    day: 21,
    kind: 'income',
    account: '4000',
    wallet: '1020',
    amount: (m) => 54200 + m * 3100,
    description: { en: 'Online shop orders', el: 'Παραγγελίες ηλεκτρονικού καταστήματος' },
  },
  {
    day: 23,
    kind: 'expense',
    account: '5040',
    wallet: '1020',
    amount: (m) => 3890 + m * 260,
    description: { en: 'Courier pickups', el: 'Παραλαβές courier' },
  },
  {
    day: 26,
    kind: 'income',
    account: '4000',
    wallet: '1000',
    amount: (m) => 29800 + m * 1500,
    description: { en: 'Market stall, Saturday', el: 'Πάγκος στην αγορά, Σάββατο' },
  },
  {
    day: 28,
    kind: 'expense',
    account: '5000',
    wallet: '1020',
    amount: (m) => 8450 + m * 500,
    description: { en: 'Kiln shelves and cones', el: 'Ράφια κλιβάνου και κώνοι' },
  },
]

const MONTHS = ['2026-06', '2026-07', '2026-08', '2026-09']

const OPENING: Named = { en: 'Opening balance', el: 'Αρχικό υπόλοιπο' }

function accountId(code: string): string {
  return `acc-${code}`
}

function money(amount_minor: number) {
  return { amount_minor }
}

function line(entryId: string, n: number, account: string, debit: number, credit: number) {
  return {
    id: `${entryId}-l${n}`,
    entry_id: entryId,
    account_id: accountId(account),
    debit: money(debit),
    credit: money(credit),
    memo: null,
  }
}

function entry(id: string, date: string, description: string, debitAcc: string, creditAcc: string, amount: number): PostedEntryView {
  return {
    entry: {
      id,
      entity_id: ENTITY_ID,
      entry_date: date,
      description,
      reference: null,
      status: 'posted',
      hidden: false,
    },
    lines: [line(id, 1, debitAcc, amount, 0), line(id, 2, creditAcc, 0, amount)],
    is_voided: false,
  }
}

function unixSeconds(date: string): string {
  return String(Math.floor(new Date(`${date}T10:30:00`).getTime() / 1000))
}

export function buildLedger(lang: DemoLang): DemoLedger {
  const entity: Entity = {
    id: ENTITY_ID,
    name: lang === 'el' ? 'Εργαστήριο Κέραμος' : 'Keramos Studio',
    base_currency: 'EUR',
    fiscal_year_start_month: 1,
    chart_template: 'company',
  }

  const accounts: Account[] = ACCOUNTS.map((seed, i) => ({
    id: accountId(seed.code),
    entity_id: ENTITY_ID,
    code: seed.code,
    name: seed.name[lang],
    account_type: seed.type,
    parent_id: null,
    is_active: true,
    is_system: false,
    sort_order: i,
  }))

  const entries: PostedEntryView[] = [
    entry('e-opening', '2026-06-01', OPENING[lang], '1020', '3000', 850000),
  ]
  const documents: DocumentMeta[] = []

  MONTHS.forEach((month, m) => {
    for (const p of PATTERNS) {
      const date = `${month}-${String(p.day).padStart(2, '0')}`
      if (date > DEMO_TODAY) continue

      const id = `e-${month}-${p.day}-${p.account}`
      const amount = p.amount(m)
      const [debit, credit] = p.kind === 'income' ? [p.wallet, p.account] : [p.account, p.wallet]

      entries.push(entry(id, date, p.description[lang], debit, credit, amount))

      if (p.receipt) {
        documents.push({
          id: `doc-${id}`,
          entity_id: ENTITY_ID,
          entry_id: id,
          filename: `${p.receipt}-${date}.pdf`,
          mime_type: 'application/pdf',
          size_bytes: 48000 + p.day * 1300,
          created_at: unixSeconds(date),
          entry_description: p.description[lang],
        })
      }
    }
  })

  const recurring: RecurringTemplate[] = [
    {
      id: 'rec-rent',
      entity_id: ENTITY_ID,
      name: lang === 'el' ? 'Ενοίκιο εργαστηρίου' : 'Studio rent',
      kind: 'expense',
      bill_status: null,
      amount_minor: 65000,
      cadence: 'monthly',
      day_of_month: 1,
      category_account_id: accountId('5020'),
      wallet_account_id: accountId('1020'),
      payable_account_id: null,
      from_account_id: null,
      to_account_id: null,
      memo: null,
      next_date: '2026-10-01',
      due: false,
    },
  ]

  return { lang, entity, accounts, entries, documents, recurring }
}
