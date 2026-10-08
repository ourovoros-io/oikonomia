# Greek review sheet

Every Greek string in `web/src/locales/el.json` that changed since the 0.1.0 tag
(commits after 2026-10-07, plus the book wording change in the same pull request as this
sheet). Fill the "Your edit" column where you want a different wording; leave it empty to
keep the current text. Placeholders such as `{name}` and `{n}` must stay as they are.

This sheet lists strings, not verdicts. Nothing below is claimed to be wrong.

## Please check

The earlier reviewer flagged these as doubtful.

| Please check | Key path | English source | Current Greek | Question |
| --- | --- | --- | --- | --- |
| Bill | `tx.form.kind.bill`, `quickAdd.kind.bill`, `tx.csv.kind.bill` | Bill | Τιμολόγιο | Is "Τιμολόγιο" the right word, or is "Πάγιο" or "Λογαριασμός ρεύματος" closer to a utility bill? Elsewhere "τιμολόγιο" also means invoice (`dropzone.title.idle`). |
| Debit/credit indicator | `tx.csv.field.direction`, `tx.csv.field.directionOptional` | Debit/credit indicator | Ένδειξη Χ/Π | Will a Greek user recognise "Χ/Π" (Χρέωση/Πίστωση) as the debit/credit column of a bank export? |
| Due count | `recurring.dueCount.one`, `recurring.dueCount.other`, `recurring.due` | {n} due | {n} οφειλή / {n} οφειλές | "Οφειλή" reads as a debt owed; the English means "due to be posted". |
| Other | `entry.kind.other` | Opening balance | Υπόλοιπο έναρξης | The kind label went "άλλο", then "Άλλο" (commit 3bbfcae), and now says "Υπόλοιπο έναρξης". Please check the wording you want for this kind, and for the similar "Άλλα…" (`moreOption`). |
| "Δεν ήταν δυνατή η ..." | every string with this opening (book errors, `settings.language.error`, `settings.prefs.resetFailed`, `error.*`) | Could not ... | Δεν ήταν δυνατή η ... | This replaced the headline style "Αποτυχία ...". Please check it reads naturally with each noun. Older ones (`documents.error.*`, `dropzone.error.analyze`) still lack a full stop. |
| Data folder unreadable | startup message `DataFolderUnreadable` in `apps/desktop/src-tauri/src/startup.rs` (not in `el.json`) | Its data folder cannot be read. | Ο φάκελος δεδομένων του δεν μπορεί να διαβαστεί. | Please check the sentence and how it follows the vault-damaged sentence. |
| CSV integer message | `error.csvInvalidInteger` | see the Error messages table | Το «{value}» δεν είναι ακέραιος αριθμός. ... | Please check that "ακέραιες μονάδες υποδιαίρεσης του νομίσματος (π.χ. λεπτά)" is clear to a non-accountant. |

## All changed Greek strings

### Accounts

| Key path | English source | Current Greek | Your edit |
| --- | --- | --- | --- |
| `accounts.form.created` | Account {code} · {name} added. | Προστέθηκε ο λογαριασμός {code} · {name}. | |
| `accounts.balance.error.invalidOwed` | Enter a valid amount (e.g. 350,00) | Εισαγάγετε έγκυρο ποσό (π.χ. 350,00) | |
| `accounts.balance.saved` | Balance of {name} set to {amount}. | Το υπόλοιπο του {name} ορίστηκε σε {amount}. | |
| `accounts.balance.owedLabel` | Amount owed ({ccy}) | Οφειλόμενο ποσό ({ccy}) | |
| `accounts.balance.owedToday` | Amount owed today: | Οφειλόμενο ποσό σήμερα: | |
| `accounts.balance.owedDescription` | State how much you owe on this account; the difference is posted against Opening Balances. | Δηλώστε πόσα οφείλετε σε αυτόν τον λογαριασμό· η διαφορά καταχωρίζεται έναντι των Υπολοίπων έναρξης. | |
| `accounts.balance.owedHelp` | Enter what you owe as a positive number: 350 for a card with 350 to pay. A negative number means the account is in your favour. | Εισαγάγετε αυτό που οφείλετε ως θετικό αριθμό: 350 για κάρτα με 350 προς πληρωμή. Ένας αρνητικός αριθμός σημαίνει ότι ο λογαριασμός είναι υπέρ σας. | |
| `accounts.balance.owedNegative` | A negative amount means this account holds money in your favour, not a debt. If you owe money, enter it as a positive number. | Ένα αρνητικό ποσό σημαίνει ότι ο λογαριασμός περιέχει χρήματα υπέρ σας, όχι χρέος. Αν οφείλετε χρήματα, εισαγάγετέ τα ως θετικό αριθμό. | |
| `accounts.deactivate.confirmTitle` | Deactivate {name}? | Απενεργοποίηση του {name}; | |
| `accounts.deactivate.confirmBody` | New entries can no longer use this account. Its existing entries and balance stay in the books, and you can reactivate it at any time. | Οι νέες εγγραφές δεν μπορούν πλέον να χρησιμοποιούν αυτόν τον λογαριασμό. Οι υπάρχουσες εγγραφές και το υπόλοιπό του παραμένουν στα βιβλία και μπορείτε να τον ενεργοποιήσετε ξανά οποτεδήποτε. | |
| `accounts.deactivate.done` | {name} deactivated. | Το {name} απενεργοποιήθηκε. | |
| `accounts.list.balance` | Balance | Υπόλοιπο | |
| `accounts.empty.body` | This book has an empty chart of accounts. | Αυτό το βιβλίο έχει κενό λογιστικό σχέδιο. | |
| `accounts.reactivate.title` | Reactivate | Επανενεργοποίηση | |
| `accounts.reactivate.aria` | Reactivate {name} | Επανενεργοποίηση του {name} | |
| `accounts.reactivate.done` | {name} reactivated. | Το {name} ενεργοποιήθηκε ξανά. | |
| `accounts.rename.title` | Rename account | Μετονομασία λογαριασμού | |
| `accounts.rename.aria` | Rename {name} | Μετονομασία του {name} | |

### App shell (header, sidebar, banners)

| Key path | English source | Current Greek | Your edit |
| --- | --- | --- | --- |
| `app.prefsDamaged.open` | Open Settings | Άνοιγμα Ρυθμίσεων | |
| `app.prefsDamaged.dismiss` | Dismiss | Απόκρυψη | |
| `app.book.emptyOption` | No books yet | Δεν υπάρχουν βιβλία ακόμη | |
| `app.sidebar.quickAddNeedsBook` | Create a book to add entries. | Δημιουργήστε πρώτα ένα βιβλίο. | |
| `app.header.createHint` | Create a book in Settings | Δημιουργήστε βιβλίο στις Ρυθμίσεις | |

### Common

| Key path | English source | Current Greek | Your edit |
| --- | --- | --- | --- |
| `common.rename` | Rename | Μετονομασία | |

### Dashboard

| Key path | English source | Current Greek | Your edit |
| --- | --- | --- | --- |
| `dashboard.empty.body` | Add a personal or company book under Settings. Each book has its own chart of accounts and reports. | Προσθέστε προσωπικό ή εταιρικό βιβλίο στις Ρυθμίσεις. Κάθε βιβλίο έχει δικό του λογιστικό σχέδιο και αναφορές. | |
| `dashboard.hero.entryCount.one` | {count} entry this {period} | {n} καταχώριση {period} | |
| `dashboard.hero.entryCount.other` | {count} entries this {period} | {n} καταχωρίσεις {period} | |

### Error messages

| Key path | English source | Current Greek | Your edit |
| --- | --- | --- | --- |
| `error.pageRender` | This page could not be shown. Your data is safe. Try again or open another page. | Αυτή η σελίδα δεν μπόρεσε να εμφανιστεί. Τα δεδομένα σας είναι ασφαλή. Δοκιμάστε ξανά ή ανοίξτε άλλη σελίδα. | |
| `error.pageRenderRetry` | Try again | Δοκιμάστε ξανά | |
| `error.csvNotUtf8` | That CSV file is not UTF-8 text. Save or export it again as CSV with UTF-8 encoding. | Αυτό το αρχείο CSV δεν είναι κείμενο UTF-8. Αποθηκεύστε το ή εξαγάγετέ το ξανά ως CSV με κωδικοποίηση UTF-8. | |
| `error.csvTooLarge` | That CSV file is larger than 8 MB. Export a shorter period, or split the file. | Αυτό το αρχείο CSV είναι μεγαλύτερο από 8 MB. Εξαγάγετε συντομότερη περίοδο ή χωρίστε το αρχείο. | |
| `error.csvJournalExport` | That file is an Oikonomia journal export. It lists journal lines, not one account’s movements, so it cannot be imported as a bank statement. Import a CSV from your bank instead. | Αυτό το αρχείο είναι εξαγωγή ημερολογίου του Oikonomia. Περιέχει γραμμές ημερολογίου και όχι κινήσεις ενός λογαριασμού, οπότε δεν μπορεί να εισαχθεί ως κίνηση τράπεζας. Εισαγάγετε ένα CSV από την τράπεζά σας. | |
| `error.csvMissingColumn` | That journal CSV has no “{column}” column. Use a file exported from Oikonomia with all its columns. | Αυτό το CSV ημερολογίου δεν έχει στήλη «{column}». Χρησιμοποιήστε ένα αρχείο εξαγωγής του Oikonomia με όλες τις στήλες του. | |
| `error.csvInvalidInteger` | “{value}” is not a whole number. A journal CSV holds amounts as whole minor units, such as 1250 for 12.50. | Το «{value}» δεν είναι ακέραιος αριθμός. Ένα CSV ημερολογίου περιέχει τα ποσά ως ακέραιες μονάδες υποδιαίρεσης του νομίσματος (π.χ. λεπτά), όπως 1250 για 12,50. | |
| `error.role.receivable` | Receivable account | Λογαριασμός απαιτήσεων | |

### Other (analyze)

| Key path | English source | Current Greek | Your edit |
| --- | --- | --- | --- |
| `analyze.notes.ocrUnreadable` | Most of this scan could not be read, so the amount is left empty. Check the amount and every other field against the document. | Το μεγαλύτερο μέρος αυτής της σάρωσης δεν διαβάστηκε, οπότε το ποσό αφέθηκε κενό. Ελέγξτε το ποσό και κάθε άλλο πεδίο με το έγγραφο. | |

### Other (date)

| Key path | English source | Current Greek | Your edit |
| --- | --- | --- | --- |
| `date.placeholder` | dd/mm/yyyy | ηη/μμ/εεεε | |
| `date.invalid` | Enter a real date from 1900 to 2100, such as 25/12/2026. | Εισαγάγετε υπαρκτή ημερομηνία από το 1900 έως το 2100, π.χ. 25/12/2026. | |

### Other (entry)

| Key path | English source | Current Greek | Your edit |
| --- | --- | --- | --- |
| `entry.kind.other` | Opening balance | Υπόλοιπο έναρξης | |

### Other (form)

| Key path | English source | Current Greek | Your edit |
| --- | --- | --- | --- |
| `form.dismiss` | Dismiss | Απόκρυψη | |
| `form.fieldRequired` | A required field is empty. Fill it in and try again. | Ένα υποχρεωτικό πεδίο είναι κενό. Συμπληρώστε το και δοκιμάστε ξανά. | |
| `form.passwordRequired` | Enter your password. | Εισαγάγετε τον κωδικό σας. | |

### Other (hiddenNote)

| Key path | English source | Current Greek | Your edit |
| --- | --- | --- | --- |
| `hiddenNote.one` | Includes 1 hidden entry, which is omitted from exports. | Περιλαμβάνει 1 κρυφή κίνηση, η οποία παραλείπεται από τις εξαγωγές. | |
| `hiddenNote.other` | Includes {count} hidden entries, which are omitted from exports. | Περιλαμβάνει {count} κρυφές κινήσεις, οι οποίες παραλείπονται από τις εξαγωγές. | |

### Quick Add

| Key path | English source | Current Greek | Your edit |
| --- | --- | --- | --- |
| `quickAdd.kind.bill` | Bill | Τιμολόγιο | |
| `quickAdd.kind.billShort` | Bill | Τιμ. | |

### Recurring

| Key path | English source | Current Greek | Your edit |
| --- | --- | --- | --- |
| `recurring.templatesCount.one` | {n} template | {n} πρότυπο | |
| `recurring.templatesCount.other` | {n} templates | {n} πρότυπα | |
| `recurring.dueCount.one` | {n} due | {n} οφειλή | |
| `recurring.dueCount.other` | {n} due | {n} οφειλές | |
| `recurring.form.dayOfWeek` | Day of the week | Ημέρα της εβδομάδας | |
| `recurring.form.month` | Month | Μήνας | |
| `recurring.form.nextDate` | Next date: {date} | Επόμενη ημερομηνία: {date} | |

### Settings

| Key path | English source | Current Greek | Your edit |
| --- | --- | --- | --- |
| `settings.prefs.unreadable` | The preferences file is damaged, so your language and Quick Add choices cannot be saved. Nothing in your vault is affected. You can reset the preferences in Settings. | Το αρχείο προτιμήσεων είναι κατεστραμμένο, γι’ αυτό η γλώσσα και οι επιλογές της Γρήγορης καταχώρισης δεν αποθηκεύονται. Η θυρίδα σας δεν επηρεάζεται. Μπορείτε να επαναφέρετε τις προτιμήσεις από τις Ρυθμίσεις. | |
| `settings.prefs.reset` | Reset preferences | Επαναφορά προτιμήσεων | |
| `settings.prefs.resetDone` | Preferences reset. The damaged file was kept beside the vault as ui-prefs.damaged.json. | Οι προτιμήσεις επαναφέρθηκαν. Το κατεστραμμένο αρχείο διατηρήθηκε δίπλα στη θυρίδα ως ui-prefs.damaged.json. | |
| `settings.prefs.resetFailed` | Could not reset the preferences. | Δεν ήταν δυνατή η επαναφορά των προτιμήσεων. | |
| `settings.support.noMailApp` | No mail app opened? Write to | Δεν άνοιξε εφαρμογή αλληλογραφίας; Γράψτε στο | |
| `settings.support.copy` | Copy address | Αντιγραφή διεύθυνσης | |
| `settings.lock.preset.1hour` | 60 min | 60 λεπτά | |
| `settings.lock.notice.saved` | Auto-lock saved. | Το αυτόματο κλείδωμα αποθηκεύτηκε. | |
| `settings.password.error.mismatch` | New passwords do not match. | Οι νέοι κωδικοί δεν ταιριάζουν. | |
| `settings.entities.title` | Books | Βιβλία | |
| `settings.entities.new` | New book | Νέο βιβλίο | |
| `settings.entities.emptyBody` | Use the New book button above to create your first book. | Χρησιμοποιήστε το κουμπί Νέο βιβλίο παραπάνω για το πρώτο σας βιβλίο. | |
| `settings.entities.deleteConfirm.title` | Delete book? | Διαγραφή βιβλίου; | |
| `settings.entities.deleteError` | Could not delete the book. | Δεν ήταν δυνατή η διαγραφή του βιβλίου. | |
| `settings.entities.archiveAria` | Archive {name} | Αρχειοθέτηση {name} | |
| `settings.entities.archiveTitle` | Archive | Αρχειοθέτηση | |
| `settings.entities.archiveConfirm.title` | Archive book? | Αρχειοθέτηση βιβλίου; | |
| `settings.entities.archiveConfirm.body` | “{name}” becomes read-only and leaves the list of books. Nothing is deleted, and you can restore it here at any time. | Το «{name}» γίνεται μόνο για ανάγνωση και αφαιρείται από τη λίστα των βιβλίων. Τίποτα δεν διαγράφεται και μπορείτε να το επαναφέρετε από εδώ οποιαδήποτε στιγμή. | |
| `settings.entities.archiveError` | Could not archive the book. | Δεν ήταν δυνατή η αρχειοθέτηση του βιβλίου. | |
| `settings.entities.archived.title` | Archived | Αρχειοθετημένα | |
| `settings.entities.archived.hint` | Archived books are kept as they are and cannot be opened or changed. Restore one to use it again. | Τα αρχειοθετημένα βιβλία διατηρούνται ως έχουν και δεν μπορούν να ανοιχτούν ή να αλλάξουν. Επαναφέρετε ένα για να το χρησιμοποιήσετε ξανά. | |
| `settings.entities.archived.loadError` | Could not load the archived books. | Δεν ήταν δυνατή η φόρτωση των αρχειοθετημένων βιβλίων. | |
| `settings.entities.restore` | Restore | Επαναφορά | |
| `settings.entities.restoreAria` | Restore {name} | Επαναφορά {name} | |
| `settings.entities.restoreError` | Could not restore the book. | Δεν ήταν δυνατή η επαναφορά του βιβλίου. | |
| `settings.entities.restoreNameTaken` | Another book is now named “{name}”. Archive or delete that book before restoring this one. | Ένα άλλο βιβλίο ονομάζεται πλέον «{name}». Αρχειοθετήστε ή διαγράψτε εκείνο το βιβλίο πριν επαναφέρετε αυτό. | |
| `settings.entities.created` | Book {name} created. | Το βιβλίο {name} δημιουργήθηκε. | |
| `settings.entities.rename.title` | Rename book | Μετονομασία βιβλίου | |
| `settings.entities.rename.aria` | Rename {name} | Μετονομασία του {name} | |
| `settings.entityCreate.title` | New book | Νέο βιβλίο | |
| `settings.entityCreate.submit` | Create book | Δημιουργία βιβλίου | |

### Transactions and entry form

| Key path | English source | Current Greek | Your edit |
| --- | --- | --- | --- |
| `tx.form.kind.bill` | Bill | Τιμολόγιο | |
| `tx.form.referencePlaceholder` | Invoice #, bill #… | Αρ. τιμολογίου… | |
| `tx.export.whisper` | Export omits hidden rows. Deleted entries are included, marked voided. | Η εξαγωγή παραλείπει τις κρυφές γραμμές. Οι διαγραμμένες εγγραφές περιλαμβάνονται, με την ένδειξη voided. | |
| `tx.csv.field.direction` | Debit/credit indicator | Ένδειξη Χ/Π | |
| `tx.csv.field.directionOptional` | Debit/credit indicator (optional) | Ένδειξη Χ/Π (προαιρετικό) | |
| `tx.csv.otherCurrency` | Some amounts are marked “{marker}”, but this book is in {currency}. Check that the file is in {currency} before you post. | Ορισμένα ποσά φέρουν την ένδειξη «{marker}», ενώ αυτό το βιβλίο είναι σε {currency}. Ελέγξτε ότι το αρχείο είναι σε {currency} πριν την καταχώριση. | |
| `tx.csv.kind.bill` | Bill | Τιμολόγιο | |
| `tx.incomeStatus` | Income status | Κατάσταση εσόδου | |
| `tx.incomeReceivedNow` | Received now | Εισπράχθηκε τώρα | |
| `tx.incomeUnpaid` | Unpaid — owed to me (receivable) | Απλήρωτο — μου οφείλεται (απαίτηση) | |
| `tx.receivableAccount` | Receivable account | Λογαριασμός απαιτήσεων | |

### Unlock and updates

| Key path | English source | Current Greek | Your edit |
| --- | --- | --- | --- |
| `unlock.error.mismatch` | Passwords do not match. | Οι κωδικοί δεν ταιριάζουν. | |

