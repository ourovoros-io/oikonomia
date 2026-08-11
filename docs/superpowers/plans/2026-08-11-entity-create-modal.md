# Entity Creation Modal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the standalone "New entity" settings section with a "New entity" button on the Entities section header that opens the create form in a modal.

**Architecture:** `CollapsibleSection` gains an optional `actions` slot rendered as a sibling of its header toggle button (interactive elements cannot nest inside a `<button>`, and action clicks must not toggle the section — same pattern `Panel` already uses). The Settings page moves the existing create form verbatim into a `Modal` and deletes the old section.

**Tech Stack:** React 19 + Vite + Tailwind (frontend only; no Rust changes).

Spec: `docs/superpowers/specs/2026-08-11-entity-create-modal-design.md`
Branch: `feature/documents-entry-detail`.

## Global Constraints

- No emojis. No `Co-Authored-By` in commits.
- Gate: `cd web && npm run build && npm run lint` (only the 3 pre-existing warnings in AccountsPage/DashboardPage/ui.tsx are acceptable).
- House idioms: busy-guarded modal close (`if (!busy) ...`), Cancel + submit footer with `border-t` as in the new-entry modal, `Button size="sm"` header actions.
- The create form's fields, validation, and post-create behavior (refresh list, select and open the new book) are unchanged.

---

### Task 1: `actions` slot + create-entity modal

**Files:**
- Modify: `web/src/components/ui.tsx:167-224` (`CollapsibleSection`)
- Modify: `web/src/pages/SettingsPage.tsx` (imports, state, `onCreate`, sections)

**Interfaces:**
- Produces: `CollapsibleSection` prop `actions?: ReactNode` — rendered right-aligned in the header, outside the toggle button; clicking it never toggles the section. All existing call sites compile unchanged (prop optional).

- [ ] **Step 1: Add the `actions` slot to `CollapsibleSection`**

In `web/src/components/ui.tsx`, add `actions` to the props type and destructure it:

```tsx
export function CollapsibleSection({
  title,
  description,
  icon,
  tone = 'accent',
  defaultOpen = false,
  flush = false,
  actions,
  children,
}: {
  title: string
  description?: string
  icon?: ReactNode
  tone?: 'accent' | 'success' | 'danger' | 'warning' | 'info' | 'muted'
  defaultOpen?: boolean
  /** Body without padding, for lists that manage their own edges. */
  flush?: boolean
  /** Right-aligned header controls, outside the toggle button (buttons cannot nest). */
  actions?: ReactNode
  children: ReactNode
}) {
```

Wrap the header so the toggle button and the actions are siblings — replace the current header `<button>...</button>` with:

```tsx
      <div className="flex items-center">
        <button
          type="button"
          onClick={() => setOpen((v) => !v)}
          aria-expanded={open}
          className="flex min-w-0 flex-1 items-center gap-3 px-5 py-4 text-left transition hover:bg-[var(--color-surface-2)]/60"
        >
          {icon ? (
            <IconBadge tone={tone} size="sm">
              {icon}
            </IconBadge>
          ) : null}
          <div className="min-w-0 flex-1">
            <h3 className="text-sm font-semibold text-[var(--color-fg)]">{title}</h3>
            {description ? <p className="text-xs text-[var(--color-muted)]">{description}</p> : null}
          </div>
          <ChevronDown
            className={cn(
              'size-4 shrink-0 text-[var(--color-muted)] transition-transform duration-200',
              open && 'rotate-180',
            )}
          />
        </button>
        {actions ? <div className="shrink-0 pr-5">{actions}</div> : null}
      </div>
```

(The only changes to the button itself: `w-full` becomes `min-w-0 flex-1` so it shares the row.)

- [ ] **Step 2: Rework `SettingsPage`**

2a. Imports: add `Plus` to the lucide import list; add `import { Modal } from '../components/Modal'` next to the `ConfirmDialog` import.

2b. State: after the existing `const [busy, setBusy] = useState(false)` add:

```tsx
  const [showCreate, setShowCreate] = useState(false)
```

2c. `onCreate` success path — close the modal after the list refresh, before navigation:

```tsx
      setName('')
      await onEntitiesChange()
      setShowCreate(false)
      onSelectEntity(entity.id)
```

2d. Delete the entire `New entity` `CollapsibleSection` block (`SettingsPage.tsx:275-327`, the one with `title="New entity"`).

2e. Add the modal directly after the existing `<ConfirmDialog ... />` element (page level, before the sections). The form body is the deleted section's form, unchanged, plus the house Cancel/submit footer replacing the bare submit button:

```tsx
      <Modal
        open={showCreate}
        title="New entity"
        description="Separate books for personal and company"
        onClose={() => {
          if (!busy) setShowCreate(false)
        }}
      >
        <form onSubmit={onCreate} className="space-y-4">
          <div className="grid gap-4 sm:grid-cols-2">
            <Field label="Name">
              <Input
                value={name}
                onChange={(e) => setName(e.target.value)}
                required
                placeholder="Personal"
              />
            </Field>
            <Field label="Currency">
              <Select value={currency} onChange={(e) => setCurrency(e.target.value)} required>
                {CURRENCIES.map((c) => (
                  <option key={c.code} value={c.code}>
                    {c.code} — {c.label}
                  </option>
                ))}
              </Select>
            </Field>
          </div>

          <div>
            <span className="mb-1.5 block text-[11px] font-medium tracking-wide text-[var(--color-muted)] uppercase">
              Chart template
            </span>
            <div className="grid gap-3 sm:grid-cols-3">
              {TEMPLATES.map((t) => {
                const Icon = t.icon
                return (
                  <ChoiceCard
                    key={t.id}
                    selected={template === t.id}
                    onClick={() => setTemplate(t.id)}
                    icon={<Icon className="size-4" strokeWidth={1.75} />}
                    title={t.title}
                    description={t.description}
                  />
                )
              })}
            </div>
          </div>

          <div className="flex justify-end gap-2 border-t border-[var(--color-border)] pt-4">
            <Button
              type="button"
              variant="secondary"
              disabled={busy}
              onClick={() => setShowCreate(false)}
            >
              Cancel
            </Button>
            <Button type="submit" busy={busy}>
              {busy ? 'Creating…' : 'Create entity'}
            </Button>
          </div>
        </form>
      </Modal>
```

2f. The Entities `CollapsibleSection` gains the header button:

```tsx
        actions={
          <Button size="sm" onClick={() => setShowCreate(true)}>
            <Plus className="size-3.5" />
            New entity
          </Button>
        }
```

2g. Empty-state copy inside the Entities section changes from
`Create an entity above to start posting entries.` to
`Use the New entity button above to create your first book.`

- [ ] **Step 3: Gate**

Run: `cd web && npm run build && npm run lint`
Expected: clean build; lint shows only the 3 pre-existing warnings.

- [ ] **Step 4: Commit**

```bash
git add web/src/components/ui.tsx web/src/pages/SettingsPage.tsx
git commit -m "feat: create entities from a modal on the Entities section header

The standalone New-entity section is gone; the form opens in a modal
from a header button. CollapsibleSection gains an actions slot rendered
beside the toggle button, never inside it (interactive elements cannot
nest), so the button does not toggle the section."
```

- [ ] **Step 5: Live check (user or controller)**

Launch `make app`: create a book through the modal (selects and opens it as before), Cancel path, busy-guarded close, section toggle unaffected by the header button.
