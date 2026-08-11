# Entity creation from the Entities section header — design

Date: 2026-08-11
Status: approved (brainstorm with owner)
Branch: `feature/documents-entry-detail` (round 3, small).

## Problem

Settings has two stacked collapsible sections: "New entity" (an inline
create form) and "Entities" (the list). The owner wants one: a "New
entity" button on the Entities section header that opens the create form
in a modal.

## Design

1. **`CollapsibleSection` gains `actions?: ReactNode`**
   (`web/src/components/ui.tsx`). The header becomes a flex row: the
   existing toggle `<button>` keeps the icon, title, description, chevron,
   and all toggle behavior; `actions` renders beside it as a sibling
   element (same pattern as `Panel`'s `actions`). Never nested inside the
   toggle button — HTML forbids interactive elements inside a `<button>`,
   and action clicks must not toggle the section.

2. **SettingsPage** (`web/src/pages/SettingsPage.tsx`):
   - Delete the standalone "New entity" `CollapsibleSection`.
   - Add `showCreate` state. The Entities section gets
     `actions={<Button size="sm" onClick={() => setShowCreate(true)}>
     <Plus className="size-3.5" /> New entity</Button>}`.
   - The create form (name, currency select, chart-template
     `ChoiceCard`s, submit) moves unchanged into a `Modal` with
     `title="New entity"`,
     `description="Separate books for personal and company"`, and the
     house busy-guarded close (`onClose={() => { if (!busy)
     setShowCreate(false) }}`).
   - `onCreate` success additionally calls `setShowCreate(false)`
     (before `onSelectEntity`, which navigates away). Failure keeps the
     modal open with the form filled; errors surface in the page's
     existing `ErrorBanner`.
   - Empty-state copy changes from "Create an entity above to start
     posting entries." to "Use the New entity button above to create
     your first book."

## Non-goals

No backend changes; no change to the create-entity fields or validation;
no redesign of other Settings sections.

## Testing

`cd web && npm run build && npm run lint` clean; live check: creating a
book through the modal selects and opens it exactly as the inline form
did; cancel path; busy-guarded close; the section toggle is unaffected
by clicks on the header button.
