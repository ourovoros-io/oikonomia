# Marketing captures

Real screenshots of this app for the marketing site (`oikonomia-website`).

`marketing.html` mounts the real `App` with Tauri's `mockIPC`, answering reads
from a fictional demo ledger (`fixture/`). It is a dev-only entry and is never
part of the desktop build.

    npm run capture:marketing -- --out ../../oikonomia-website/public/app

Writes `{en,el}/{dashboard,transactions,documents,reports}.png`: 1440x900 at
2x, transparent background (the site draws its own aurora behind the panes).

Fails on any console error, page error or missing file. A new app command the
fixture does not answer fails with `marketing fixture has no answer for <cmd>`:
add it to `fixture/handler.ts`, typed from `src/lib/api.ts`.

Re-run whenever the app UI changes, then commit the PNGs in the site repo.
