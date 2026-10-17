# Marketing captures

Real screenshots of this app for the marketing site (`oikonomia-website`).

`marketing.html` mounts the real `App` with Tauri's `mockIPC`, answering reads
from a fictional demo ledger (`fixture/`). It is a dev-only entry and is never
part of the desktop build.

One-time setup, installs the Chromium build Playwright drives:

    npx playwright install chromium

Then, whenever a capture is needed:

    npm run capture:marketing -- --out ../../oikonomia-website/public/app

Writes `{en,el}/{dashboard,transactions,documents,reports}.png`: 1440x900 at
2x, transparent background (the site draws its own aurora behind the panes).

Fails on any console error, page error or missing file. A new app command the
fixture does not answer fails with `marketing fixture has no answer for <cmd>`:
add it to `fixture/handler.ts`, typed from `src/lib/api.ts`.

Re-run whenever the app UI changes, then commit the PNGs in the site repo.

## Layout check

`marketing.html?lang=` takes `en`, `el`, `fr` or `de`. French and German show
the English demo ledger under their own UI text; only English and Greek have
a ledger of their own.

    npm run check:layout

Measures the fixture in Chromium at 960, 1024, 1280, 1440 and 1600 px in all
four languages, on every screen, and fails if:

- a Button label wraps, or the button is narrower than its label (a label
  that truncates inside its own element may end in an ellipsis);
- a dashboard arc tile label or hint is cut off or runs past its tile;
- an Accounts row action is out of line with the same action in the other
  rows, or a Set balance label takes more than one line.

Console errors and page errors fail it too. CI runs it in the `layout` job.
