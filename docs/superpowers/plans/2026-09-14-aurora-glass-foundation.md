# Aurora Glass Foundation (Phase 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give every screen the Aurora glass look — typefaces, tokens, the aurora, glass primitives and a glass shell — and make the app dark-only end to end, without changing any page's composition.

**Architecture:** The look flows through three layers. CSS tokens and glass utilities live in `web/src/index.css`, with `@font-face` rules generated into `web/src/fonts.css`. The shared primitives in `web/src/components/ui.tsx` and the dialog shells consume those tokens, so pages restyle without being edited. `App.tsx` mounts one `Aurora` behind a floating glass sidebar. On the Rust side the theme preference is deleted and the native window is always dark.

**Tech Stack:** React 19, TypeScript 7, Tailwind CSS 4, Vite 8, Vitest 4 with Testing Library; Tauri 2; Rust (`oikonomia-core`, desktop crate `oikonomia`); Python 3 standard library for the font fetch script.

**Spec:** `docs/superpowers/specs/2026-09-14-aurora-glass-ui-design.md` — this plan is Rollout phase 1 ("Foundation"). Phases 2 (data), 3 (composition) and 4 (edges) get their own plans; the user reviews the running app after this phase.

## Global Constraints

- Business rules live in `oikonomia-core`, never in TypeScript. This phase changes no business rule.
- No emojis in UI, code, comments or commit messages. No `Co-Authored-By` or other attribution trailers; commits use the user's git identity only.
- Commit messages explain why. Small commits, one per task.
- Before every web commit: `cd web && npx tsc -b && npm run lint && npm test`. Before every Rust commit: `cargo fmt --all`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test -p oikonomia-core`, and `cargo build -p oikonomia` when `apps/desktop` changed.
- Code style: readable multi-line code; blank lines between logical steps; comments explain intent, invariants and trade-offs, never narrate the code.
- Dark-only: no `prefers-color-scheme`, no light palette, no `.dark` class, no theme toggle.
- Fonts are bundled in the repo and never fetched at runtime (the app must render offline). `web/public/fonts` carries each family's SIL OFL text.
- Barlow and IBM Plex Mono have **no Greek glyphs**; the `el` locale needs them. Font stacks: `"Barlow", "Sofia Sans", ...` and `"IBM Plex Mono", "JetBrains Mono", ...`.
- Locked colours (copy exactly):
  - Brand light, chrome only: `#2EE6A6 -> #37D5FF -> #7A8CFF`; dark ink on it `#06110D`.
  - Ledger money in `#1BA39A`, text tint `#6FD9C4`; money out `#E8603F`, text tint `#FF9B7E`.
  - Danger: `#FF8295` for text and icons; destructive button fill `#D7304C -> #B8327A` with white text.
  - Hazard: `#E2F23A`.
- Measured glass values (copy exactly): pane fill `rgba(16, 18, 24, 0.5)`; ink `#E7EBF1`; ink-mid `#AEB5BF`; ink-soft `#A3AAB5`; ink-dim `#848B98` (icons and inactive controls only, never text).
- The Reports `--viz-*` palette values do not change.
- Accessible names of existing controls do not change: the web test suite selects by role and name.
- Out of this phase: Dashboard and Transactions composition, `CashFlowLight`, `Arc`, the sidebar Quick add button, the Unlock screen and the tray Quick add restyle, and any new Rust command.

## File Structure

| File | Responsibility |
|---|---|
| `crates/oikonomia-core/src/prefs.rs` | UI prefs without a theme; proves old files still load |
| `apps/desktop/src-tauri/src/commands.rs` | loses the two theme commands and `native_theme` |
| `apps/desktop/src-tauri/src/lib.rs` | forces a dark native window at startup |
| `web/scripts/fetch-fonts.py` (new) | reproducible download of faces, ranges and OFL texts |
| `web/public/fonts/*` (new, generated) | woff2 faces and OFL texts |
| `web/src/fonts.css` (new, generated) | `@font-face` rules with Google's exact unicode ranges |
| `web/src/index.css` | tokens, glass utilities, aurora, field box, base |
| `web/tests/fonts.test.ts` (new) | design invariants for typefaces |
| `web/tests/tokens.test.ts` (new) | design invariants for colours and contrast |
| `web/tsconfig.node.json` | type-checks `web/tests` with Node types |
| `web/src/lib/motion.ts` (new) | `useMotionAllowed`: reduced motion, visibility, focus |
| `web/src/components/Aurora.tsx` (new) | the ambient layer |
| `web/src/components/ui.tsx` | glass treatment of every primitive |
| `web/src/components/Modal.tsx`, `ConfirmDialog.tsx`, `DateInput.tsx` | glass dialogs and popover |
| `web/src/App.tsx` | aurora mount, glass sidebar with a books list, no theme |
| `web/src/components/Logo.tsx` | the mark in the brand light |
| money amount sites in pages | Ledger tokens instead of status colours |

Design invariants that read CSS files live in `web/tests/`, not `web/src/`: Vitest turns every `.css` import, including `?raw`, into an empty string (verified), and `tsconfig.app.json` type-checks `src` without Node types.

---

### Task 1: Retire the theme preference in Rust

**Files:**
- Modify: `crates/oikonomia-core/src/prefs.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `oikonomia_core::prefs::UiPrefs` without a `theme` field; no `Theme` type; the IPC commands `settings_get_theme` and `settings_set_theme` no longer exist (Task 2 removes their TypeScript callers).

- [ ] **Step 1: Write the failing test**

In `crates/oikonomia-core/src/prefs.rs`, inside the `#[cfg(test)] mod tests` block, add after the test `unknown_locale_field_is_ignored`:

```rust
    #[test]
    fn retired_theme_key_still_loads() {
        let Ok(dir) = tempdir() else {
            return;
        };

        // Builds before the dark-only redesign wrote a "theme" key. It must
        // load as an ignored field, never make the file read as corrupt.
        let json = r#"{ "theme": "light", "locale": "el" }"#;
        assert!(fs::write(ui_prefs_path(dir.path()), json).is_ok());

        assert_eq!(
            load_ui_prefs(dir.path()),
            UiPrefs {
                locale: Locale::El,
                ..UiPrefs::default()
            }
        );
    }
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p oikonomia-core prefs::tests::retired_theme_key_still_loads`
Expected: FAIL — the assertion shows `theme: Light` on the left and `theme: Dark` on the right.

- [ ] **Step 3: Remove the theme from `prefs.rs`**

Replace the module doc lines

```rust
//! Kept outside the encrypted vault on purpose: the unlock screen must render
//! with the user's theme before any password has been entered. Nothing stored
//! here is sensitive.
```

with

```rust
//! Kept outside the encrypted vault on purpose: the tray menu and window
//! chrome must be built in the user's locale before any password has been
//! entered. Nothing stored here is sensitive.
//!
//! There is no theme preference: the app is dark-only, so a `"theme"` key in
//! a file written by an older build is ignored on load.
```

Delete this block entirely (including the blank line after it):

```rust
/// UI color theme.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// Dark theme (the default).
    #[default]
    Dark,
    /// Light theme.
    Light,
}
```

In `pub struct UiPrefs`, delete these two lines:

```rust
    /// Color theme.
    pub theme: Theme,
```

In the tests module make exactly these edits:

1. Delete the whole test function `theme_round_trips` (the locale round-trip tests already cover saving and loading).
2. In `unknown_fields_are_tolerated`, replace the expected value

   ```rust
            UiPrefs {
                theme: Theme::Light,
                ..UiPrefs::default()
            }
   ```

   with `UiPrefs::default()`.
3. In `last_used_round_trips`, delete the line `theme: Theme::Dark,`.
4. Delete **both** occurrences of the line `assert_eq!(prefs.theme, Theme::Light);` (in `missing_last_used_fields_default` and `missing_locale_defaults_to_en`).
5. In `unknown_locale_field_is_ignored`, delete the line `theme: Theme::Light,`.

Leave every JSON fixture containing `"theme": "light"` as it is: those strings now exercise the retired key.

- [ ] **Step 4: Remove the theme commands from `commands.rs`**

In the `use oikonomia_core::prefs::{...}` import, change

```rust
    LastRoleAccounts, Locale, Theme, UiPrefs, last_accounts_key, load_ui_prefs, save_ui_prefs,
```

to

```rust
    LastRoleAccounts, Locale, UiPrefs, last_accounts_key, load_ui_prefs, save_ui_prefs,
```

Delete exactly these two commands and their doc comments. **Stop at the closing brace of `settings_set_theme`: `settings_get_locale` and `settings_set_locale` sit directly below and must stay.**

```rust
/// Get the UI theme. Plaintext preference: readable before unlock so the
/// unlock screen already renders in the user's theme.
#[tauri::command]
pub fn settings_get_theme(state: State<'_, AppState>) -> Theme {
    load_ui_prefs(state.data_dir()).theme
}

/// Persist the UI theme and sync the native window appearance. Without the
/// sync, `WKWebView` keeps drawing scrollbars and native controls in the OS
/// appearance rather than the app's theme.
#[tauri::command]
pub fn settings_set_theme(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    theme: Theme,
) -> CommandResult<()> {
    let prefs_guard = state.lock_prefs();
    let mut prefs = load_ui_prefs(state.data_dir());
    prefs.theme = theme;
    save_ui_prefs(state.data_dir(), &prefs)?;
    drop(prefs_guard);
    app.set_theme(Some(native_theme(theme)));
    Ok(())
}
```

Delete this function and its doc comment:

```rust
/// Map the stored theme onto Tauri's native window theme.
pub fn native_theme(theme: Theme) -> tauri::Theme {
    match theme {
        Theme::Dark => tauri::Theme::Dark,
        Theme::Light => tauri::Theme::Light,
    }
}
```

Change the doc comment of `settings_get_ui_prefs` from `/// Full plaintext UI prefs (theme, locale, tray last-used). Safe before unlock.` to `/// Full plaintext UI prefs (locale, tray last-used). Safe before unlock.`

Confirm nothing was over-deleted: `grep -n "fn settings_get_locale\|fn settings_set_locale" apps/desktop/src-tauri/src/commands.rs` must print both functions.

- [ ] **Step 5: Force a dark native window in `lib.rs`**

Replace

```rust
            // Native window appearance (scrollbars, controls, title bar) must
            // match the stored theme, not the OS preference.
            let prefs = oikonomia_core::prefs::load_ui_prefs(app_state.data_dir());
            app.handle()
                .set_theme(Some(commands::native_theme(prefs.theme)));
```

with

```rust
            // Native window appearance (scrollbars, controls, title bar) must
            // match the app, not the OS preference. The UI is dark-only, so a
            // user who once picked the retired light theme still gets dark
            // native chrome instead of a light title bar around a dark window.
            let prefs = oikonomia_core::prefs::load_ui_prefs(app_state.data_dir());
            app.handle().set_theme(Some(tauri::Theme::Dark));
```

In the `tauri::generate_handler![...]` list, delete the two lines `commands::settings_get_theme,` and `commands::settings_set_theme,`.

- [ ] **Step 6: Verify**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test -p oikonomia-core && cargo build -p oikonomia`
Expected: clippy clean; every `oikonomia-core` test passes, including `retired_theme_key_still_loads`; the desktop crate builds. (`cargo build -p oikonomia` needs `web/dist` to exist; if it is missing, run `mkdir -p web/dist` first, as CI does.)

- [ ] **Step 7: Commit**

```bash
git add crates/oikonomia-core/src/prefs.rs apps/desktop/src-tauri/src/commands.rs apps/desktop/src-tauri/src/lib.rs
git commit -m "feat: retire the theme preference; the native window is always dark" -m "The Aurora glass UI is dark-only. Keeping the stored preference would let a user who once chose light get a light title bar and scrollbars around a dark window, so startup now sets Theme::Dark unconditionally and the preference, its two commands and native_theme are gone. UiPrefs is serde(default) and ignores unknown keys, so prefs files from older builds still load; a test pins that."
```

---

### Task 2: Make the web app dark-only

**Files:**
- Modify: `web/src/App.test.tsx`, `web/src/App.tsx`, `web/src/QuickAddApp.tsx`, `web/src/main.tsx`
- Modify: `web/src/lib/api.ts`, `web/src/lib/i18n.ts`
- Modify: `web/src/locales/en.json`, `web/src/locales/de.json`, `web/src/locales/el.json`, `web/src/locales/fr.json`
- Modify: `web/src/index.css`

**Interfaces:**
- Consumes: Task 1 (the commands these callers used are gone).
- Produces: no `Theme` type and no `api.getTheme` / `api.setTheme`; `UiPrefs` without `theme`; no `.dark` class anywhere.

- [ ] **Step 1: Write the failing test**

In `web/src/App.test.tsx`, add at the end of the file:

```tsx
describe('App shell', () => {
  test('is dark-only: offers no light or dark mode switch', async () => {
    render(<App />)
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Settings' })).toBeTruthy()
    })

    expect(screen.queryByRole('button', { name: /light mode|dark mode/i })).toBeNull()
  })
})
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cd web && npx vitest run src/App.test.tsx -t "dark-only"`
Expected: FAIL — `expected <button aria-label="Switch to light mode"> to be null`.

- [ ] **Step 3: Remove the theme from the React code**

`web/src/App.tsx`:
- In the `lucide-react` import, delete the lines `Moon,` and `Sun,`.
- Delete the line `const [dark, setDark] = useState(true)`.
- Delete this whole block (two effects and the toggle):

  ```tsx
  useEffect(() => {
    document.documentElement.classList.toggle('dark', dark)
  }, [dark])

  // Stored theme applies before unlock too; browser dev and first run keep
  // the dark default.
  useEffect(() => {
    void api
      .getTheme()
      .then((theme) => setDark(theme === 'dark'))
      .catch(() => undefined)
  }, [])

  function toggleTheme() {
    const next = !dark
    setDark(next)
    void api.setTheme(next ? 'dark' : 'light').catch(() => undefined)
  }
  ```

- Delete the toggle button in the header:

  ```tsx
            <Button
              variant="secondary"
              size="icon"
              onClick={toggleTheme}
              aria-label={dark ? t('app.switchToLight') : t('app.switchToDark')}
              title={dark ? t('app.lightMode') : t('app.darkMode')}
            >
              {dark ? <Sun className="size-4" /> : <Moon className="size-4" />}
            </Button>
  ```

`web/src/QuickAddApp.tsx`: delete the line `const [dark, setDark] = useState(true)` and this block:

```tsx
  useEffect(() => {
    void api
      .getTheme()
      .then((t) => setDark(t === 'dark'))
      .catch(() => undefined)
  }, [])

  useEffect(() => {
    document.documentElement.classList.toggle('dark', dark)
  }, [dark])

```

`web/src/main.tsx`: delete the line `document.documentElement.classList.add('dark')` and the blank line above it.

`web/src/lib/api.ts`:
- Delete `/** UI color theme, persisted outside the encrypted vault. */` and `export type Theme = 'dark' | 'light'` plus the blank line after them.
- In `export type UiPrefs = {`, delete the line `theme: Theme`.
- Replace every occurrence of `(theme + tray last-used + locale)` with `(tray last-used + locale)`.
- Delete the three lines `/** Theme is a plaintext pref (Rust side): readable before unlock. */`, `getTheme: () => call<Theme>('settings_get_theme'),` and `setTheme: (theme: Theme) => call<void>('settings_set_theme', { theme }),`.
- Replace `/** Locale is a plaintext pref (Rust side): readable before unlock. Same shape as theme. */` with `/** Locale is a plaintext pref (Rust side): readable before unlock. */`.

`web/src/App.test.tsx`: in the `vi.mock('./lib/api', ...)` factory, delete `getTheme: vi.fn(async () => 'dark'),` and `setTheme: vi.fn(),`.

- [ ] **Step 4: Remove the theme copy**

`web/src/lib/i18n.ts`: delete these four alias lines:

```ts
  'app.switchToLight': 'app.theme.ariaToLight',
  'app.switchToDark': 'app.theme.ariaToDark',
  'app.lightMode': 'app.theme.titleLight',
  'app.darkMode': 'app.theme.titleDark',
```

`web/src/locales/en.json`: delete the four entries `"app.switchToLight"`, `"app.switchToDark"`, `"app.lightMode"`, `"app.darkMode"`.

`web/src/locales/de.json`, `el.json`, `fr.json`: inside the `"app"` object, delete the whole `"theme": { ... },` object (its four keys are `ariaToLight`, `ariaToDark`, `titleLight`, `titleDark`). Keep the JSON valid: `node -e "for (const l of ['de','el','fr','en']) JSON.parse(require('fs').readFileSync('src/locales/'+l+'.json','utf8'))"` must print nothing.

- [ ] **Step 5: Remove the light palette from `index.css`**

- Delete the line `@custom-variant dark (&:where(.dark, .dark *));` and the blank line after it.
- Delete the light block from its comment through its closing brace:

  ```css
  /* Light theme overrides via html:not(.dark) — deeper hues for contrast
     on white, same green family. */
  html:not(.dark) {
    ...
    --color-info-soft: rgba(2, 132, 199, 0.1);
  }
  ```

- In `@layer base`, replace

  ```css
    /* Native form controls must follow the app's theme class, not the OS
       preference. Scrollbars additionally get explicit palette colors:
       WKWebView styles inner scrollers from the window appearance, so
       color-scheme alone leaves a light scrollbar on a dark window (the
       Rust side also syncs the native window theme via set_theme). */
    html {
      color-scheme: light;
      scrollbar-color: var(--color-border-strong) transparent;
    }
    html.dark {
      color-scheme: dark;
    }
  ```

  with

  ```css
    /* Dark-only. Scrollbars get explicit colours because WKWebView styles
       inner scrollers from the window appearance; the Rust side forces a dark
       native window at startup to match. */
    html {
      color-scheme: dark;
      scrollbar-color: var(--color-border-strong) transparent;
    }
  ```

- [ ] **Step 6: Verify**

Run: `cd web && npx tsc -b && npm run lint && npm test`
Expected: no type errors; oxlint reports warnings only (none new) and 0 errors; every test passes, including `is dark-only`.
Also run `grep -rn "getTheme\|setTheme\|classList.*'dark'\|switchToLight\|lightMode" src` — expected: no output.

- [ ] **Step 7: Commit**

```bash
git add web/src
git commit -m "feat(web): make the UI dark-only" -m "Aurora glass only reads against a near-black ground, and the theme preference is gone from Rust. The toggle, its API wrappers, the light palette, the .dark class and the toggle's copy in every locale go with it, so no path can paint a light screen."
```

---

### Task 3: Bundle the typefaces, with Greek covered

**Files:**
- Create: `web/scripts/fetch-fonts.py`
- Create (generated by the script): `web/public/fonts/*.woff2`, `web/public/fonts/OFL-*.txt`, `web/src/fonts.css`
- Create: `web/tests/fonts.test.ts`
- Modify: `web/tsconfig.node.json`, `web/src/index.css`, `web/package.json`, `web/package-lock.json`

**Interfaces:**
- Consumes: nothing.
- Produces: CSS families `"Barlow"` (400, 500, 600, Latin), `"IBM Plex Mono"` (500, Latin), `"Sofia Sans"` (variable 400–900, Latin and Greek), `"JetBrains Mono"` (500, Greek); theme tokens `--font-sans`, `--font-mono`, `--font-display`.

- [ ] **Step 1: Let `web/tests` type-check with Node types**

In `web/tsconfig.node.json`, change `"include": ["vite.config.ts"]` to `"include": ["vite.config.ts", "tests"]`.

- [ ] **Step 2: Write the failing test**

Create `web/tests/fonts.test.ts`:

```ts
import { existsSync, readFileSync } from 'node:fs'
import { describe, expect, test } from 'vitest'

// These invariants read CSS from disk: Vitest turns .css imports into empty
// strings, so importing the files would silently test nothing.
const indexCss = readFileSync(new URL('../src/index.css', import.meta.url), 'utf8')
const fontsCssUrl = new URL('../src/fonts.css', import.meta.url)
const fontsDir = new URL('../public/fonts/', import.meta.url)

function fontsCss(): string {
  return readFileSync(fontsCssUrl, 'utf8')
}

function stack(token: string): string[] {
  const match = new RegExp(`${token}:\\s*([^;]+);`).exec(indexCss)
  expect(match, `${token} is declared in index.css`).not.toBeNull()

  return (match?.[1] ?? '')
    .split(',')
    .map((family) => family.trim().replace(/"/g, ''))
}

describe('bundled typefaces', () => {
  test('every face fonts.css declares ships in public/fonts', () => {
    const files = [...fontsCss().matchAll(/url\("\/fonts\/([^"]+)"\)/g)].map((m) => m[1])

    expect(files.length).toBeGreaterThan(0)
    for (const file of files) {
      expect(existsSync(new URL(file, fontsDir)), file).toBe(true)
    }
  })

  test('Latin faces fall back to Greek-capable faces, in order', () => {
    // Barlow and IBM Plex Mono carry no Greek glyphs; the el locale needs them.
    const sans = stack('--font-sans')
    const mono = stack('--font-mono')

    expect(sans.indexOf('Barlow')).toBe(0)
    expect(sans.indexOf('Sofia Sans')).toBe(1)
    expect(mono.indexOf('IBM Plex Mono')).toBe(0)
    expect(mono.indexOf('JetBrains Mono')).toBe(1)
  })

  test('the Greek fallbacks really carry Greek', () => {
    const faces = fontsCss().split('@font-face')

    for (const family of ['Sofia Sans', 'JetBrains Mono']) {
      const greek = faces.some((face) => face.includes(`"${family}"`) && face.includes('U+0370-0377'))
      expect(greek, family).toBe(true)
    }
  })

  test('each bundled family ships its SIL OFL text', () => {
    const licences = ['OFL-Barlow.txt', 'OFL-IBMPlexMono.txt', 'OFL-SofiaSans.txt', 'OFL-JetBrainsMono.txt']

    for (const licence of licences) {
      expect(existsSync(new URL(licence, fontsDir)), licence).toBe(true)
    }
  })

  test('nothing is fetched from a font service at runtime', () => {
    expect(indexCss + fontsCss()).not.toMatch(/fonts\.(googleapis|gstatic)\.com|@fontsource/)
  })
})
```

- [ ] **Step 3: Run it and watch it fail**

Run: `cd web && npx vitest run tests/fonts.test.ts`
Expected: FAIL — `ENOENT: no such file or directory` for `src/fonts.css`, and `--font-sans` starts with `Inter Variable`.

- [ ] **Step 4: Write the fetch script**

Create `web/scripts/fetch-fonts.py`:

```python
#!/usr/bin/env python3
"""Fetch the Aurora glass typefaces into web/public/fonts and write web/src/fonts.css.

The app renders offline, so its fonts ship in the repository; this script is
how they got there and how to refresh them. It asks the Google Fonts CSS API
for exactly the families, weights and scripts the design uses, downloads each
woff2 once, copies the unicode ranges Google publishes (so a Greek face is only
ever used for Greek), and fetches each family's SIL OFL text, which the licence
requires next to distributed copies.

Run from anywhere: python3 web/scripts/fetch-fonts.py
"""

import re
import sys
import urllib.request
from pathlib import Path

# The CSS API answers a Safari user agent with woff2 sources.
USER_AGENT = (
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0) AppleWebKit/605.1.15 "
    "(KHTML, like Gecko) Version/17.0 Safari/605.1.15"
)

WEB_DIR = Path(__file__).resolve().parent.parent
FONTS_DIR = WEB_DIR / "public" / "fonts"
CSS_PATH = WEB_DIR / "src" / "fonts.css"

# family -> (css2 query, subsets to keep, directory under google/fonts/ofl)
FAMILIES = {
    "Barlow": ("Barlow:wght@400;500;600", {"latin"}, "barlow"),
    "IBM Plex Mono": ("IBM+Plex+Mono:wght@500", {"latin"}, "ibmplexmono"),
    "Sofia Sans": ("Sofia+Sans:wght@400;500;600;900", {"latin", "greek"}, "sofiasans"),
    "JetBrains Mono": ("JetBrains+Mono:wght@500", {"greek"}, "jetbrainsmono"),
}

FACE_PATTERN = re.compile(r"/\*\s*([a-z-]+)\s*\*/\s*@font-face\s*\{([^}]*)\}")


def fetch(url: str) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=30) as response:
        return response.read()


def required(pattern: str, body: str, what: str) -> str:
    match = re.search(pattern, body)
    if match is None:
        sys.exit(f"@font-face without {what}: {body!r}")
    return match.group(1).strip()


def faces_for(family: str, query: str, subsets: set[str]) -> list[dict]:
    css = fetch(f"https://fonts.googleapis.com/css2?family={query}&display=swap").decode()

    # Variable families return the same file for every weight. Group by URL so
    # each file downloads once and declares the weight range it covers.
    by_url: dict[str, dict] = {}
    for subset, body in FACE_PATTERN.findall(css):
        if subset not in subsets:
            continue

        url = required(r"url\((https://[^)]+)\)", body, "src url")
        face = by_url.setdefault(
            url,
            {
                "family": family,
                "subset": subset,
                "url": url,
                "range": required(r"unicode-range:\s*([^;]+);", body, "unicode-range"),
                "weights": [],
            },
        )
        face["weights"].append(int(required(r"font-weight:\s*([^;]+);", body, "font-weight")))

    found = {face["subset"] for face in by_url.values()}
    if found != subsets:
        sys.exit(f"{family}: expected subsets {sorted(subsets)}, got {sorted(found)}")

    return list(by_url.values())


def weights(face: dict) -> list[int]:
    return sorted(set(face["weights"]))


def file_name(face: dict) -> str:
    covered = weights(face)
    weight = str(covered[0]) if len(covered) == 1 else "variable"
    return f"{face['family'].replace(' ', '')}-{weight}-{face['subset']}.woff2"


def font_face_css(face: dict) -> str:
    covered = weights(face)
    weight = str(covered[0]) if len(covered) == 1 else f"{covered[0]} {covered[-1]}"

    return (
        "@font-face {\n"
        f'  font-family: "{face["family"]}";\n'
        f'  src: url("/fonts/{file_name(face)}") format("woff2");\n'
        f"  font-weight: {weight};\n"
        "  font-style: normal;\n"
        "  font-display: swap;\n"
        f"  unicode-range: {face['range']};\n"
        "}\n"
    )


def main() -> None:
    FONTS_DIR.mkdir(parents=True, exist_ok=True)
    blocks = []

    for family, (query, subsets, ofl_dir) in FAMILIES.items():
        for face in faces_for(family, query, subsets):
            (FONTS_DIR / file_name(face)).write_bytes(fetch(face["url"]))
            blocks.append(font_face_css(face))
            print(f"public/fonts/{file_name(face)}")

        licence = fetch(f"https://raw.githubusercontent.com/google/fonts/main/ofl/{ofl_dir}/OFL.txt")
        licence_name = f"OFL-{family.replace(' ', '')}.txt"
        (FONTS_DIR / licence_name).write_bytes(licence)
        print(f"public/fonts/{licence_name}")

    header = (
        "/* Generated by web/scripts/fetch-fonts.py. Do not edit by hand.\n"
        "   Barlow and IBM Plex Mono carry no Greek glyphs. The Greek faces are\n"
        "   scoped by unicode-range and sit next in the stacks in index.css. */\n\n"
    )
    CSS_PATH.write_text(header + "\n".join(blocks))
    print(f"src/{CSS_PATH.name}")


if __name__ == "__main__":
    main()
```

- [ ] **Step 5: Fetch the fonts**

Run: `python3 web/scripts/fetch-fonts.py`
Expected output (seven faces, four licences, one stylesheet — Sofia Sans arrives as one variable file per script):

```
public/fonts/Barlow-400-latin.woff2
public/fonts/Barlow-500-latin.woff2
public/fonts/Barlow-600-latin.woff2
public/fonts/OFL-Barlow.txt
public/fonts/IBMPlexMono-500-latin.woff2
public/fonts/OFL-IBMPlexMono.txt
public/fonts/SofiaSans-variable-greek.woff2
public/fonts/SofiaSans-variable-latin.woff2
public/fonts/OFL-SofiaSans.txt
public/fonts/JetBrainsMono-500-greek.woff2
public/fonts/OFL-JetBrainsMono.txt
src/fonts.css
```

(The order of the two Sofia Sans lines may differ.) Open `web/src/fonts.css` and confirm the Sofia Sans faces declare `font-weight: 400 900`.

- [ ] **Step 6: Point the stylesheet at the bundled faces**

In `web/src/index.css`, replace `@import "@fontsource-variable/inter";` with `@import "./fonts.css";`.

In the `@theme` block, replace

```css
  /* One typeface, no exceptions: Inter Variable is bundled locally
     (no network) and covers numerals via font-variant-numeric. */
  --font-sans: "Inter Variable", ui-sans-serif, system-ui, -apple-system,
    "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
  --font-mono: "Inter Variable", ui-sans-serif, system-ui, sans-serif;
```

with

```css
  /* Barlow and IBM Plex Mono carry no Greek glyphs; the next family in each
     stack does (see fonts.css), so the el locale never drops to a system face. */
  --font-sans: "Barlow", "Sofia Sans", "Helvetica Neue", Arial, sans-serif;
  --font-mono: "IBM Plex Mono", "JetBrains Mono", ui-monospace, Menlo, monospace;
  --font-display: "Sofia Sans", "Arial Black", "Helvetica Neue", sans-serif;
```

Remove the Inter package the screen no longer uses (the PDF export's Inter lives in `src/assets/fonts` and is untouched): `cd web && npm uninstall @fontsource-variable/inter`.

- [ ] **Step 7: Verify**

Run: `cd web && npx vitest run tests/fonts.test.ts`
Expected: PASS, 5 tests.

Run: `cd web && npx tsc -b && npm run lint && npm test && npm run build && ls dist/fonts`
Expected: all green; `dist/fonts` lists the seven woff2 files and four OFL texts.

- [ ] **Step 8: Commit**

```bash
git add web/scripts/fetch-fonts.py web/public/fonts web/src/fonts.css web/src/index.css web/tests/fonts.test.ts web/tsconfig.node.json web/package.json web/package-lock.json
git commit -m "feat(web): bundle Barlow, IBM Plex Mono and Sofia Sans with Greek covered" -m "The Aurora glass type system is Barlow for UI text, IBM Plex Mono for captions and Sofia Sans for display. Barlow and IBM Plex Mono have no Greek glyphs and the app ships an el locale, so each stack falls back to a Greek-capable face (Sofia Sans, JetBrains Mono) scoped by Google's unicode ranges instead of dropping to a system font. The faces are fetched once by a committed script and served from public/fonts with their OFL texts; nothing loads at runtime."
```

---

### Task 4: Aurora glass tokens and glass utilities

**Files:**
- Create: `web/tests/tokens.test.ts`
- Modify: `web/src/index.css`

**Interfaces:**
- Consumes: Task 3's font tokens (repeated verbatim in the new `@theme`).
- Produces, used by every later task:
  - Colour tokens (Tailwind `@theme`): `--color-canvas`, `--color-surface`, `--color-surface-2`, `--color-surface-elevated`, `--color-control`, `--color-border`, `--color-border-strong`, `--color-fg`, `--color-fg-secondary`, `--color-muted`, `--color-dim`, `--color-accent`, `--color-accent-b`, `--color-accent-c`, `--color-accent-hover`, `--color-accent-soft`, `--color-accent-muted`, `--color-on-accent`, `--color-money-in`, `--color-money-in-text`, `--color-money-in-soft`, `--color-money-out`, `--color-money-out-text`, `--color-money-out-soft`, `--color-danger`, `--color-danger-text`, `--color-danger-soft`, `--color-danger-fill-a`, `--color-danger-fill-b`, `--color-success`, `--color-success-soft`, `--color-warning`, `--color-warning-soft`, `--color-info`, `--color-info-soft`, `--radius-control`, `--control-h`, `--sidebar-w`.
  - Plain custom properties: `--glass-fill`, `--glass-edge`, `--glass-highlight`, `--aurora-veil-top`, `--aurora-veil-mid`.
  - Utility classes: `.glass-pane`, `.glass-dialog`, `.glass-scrim`.

- [ ] **Step 1: Write the failing test**

Create `web/tests/tokens.test.ts`:

```ts
import { readFileSync } from 'node:fs'
import { describe, expect, test } from 'vitest'

// Read from disk: Vitest turns .css imports into empty strings.
const css = readFileSync(new URL('../src/index.css', import.meta.url), 'utf8')

/**
 * The lightest backdrop pixel any pane text can sit on: the 99th percentile
 * under every pane, measured across five points of the aurora's drift at a
 * pane fill of 0.5 (see the spec's "Measured, not assumed" note). If the fill
 * or the aurora changes, re-measure before touching this number.
 */
const BRIGHTEST_GLASS: [number, number, number] = [8, 66, 48]

function token(name: string): string {
  const match = new RegExp(`${name}:\\s*([^;]+);`).exec(css)
  expect(match, `${name} is declared`).not.toBeNull()

  return (match?.[1] ?? '').trim().toLowerCase()
}

function rgb(hex: string): [number, number, number] {
  return [1, 3, 5].map((i) => Number.parseInt(hex.slice(i, i + 2), 16)) as [number, number, number]
}

function luminance([r, g, b]: [number, number, number]): number {
  const linear = (channel: number) => {
    const c = channel / 255
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4
  }

  return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

function contrast(a: [number, number, number], b: [number, number, number]): number {
  const [light, dark] = [luminance(a), luminance(b)].sort((x, y) => y - x)
  return (light + 0.05) / (dark + 0.05)
}

describe('Aurora glass tokens', () => {
  test('money wears the validated Ledger pair', () => {
    // Validated with the dataviz six checks against #11141b: deutan dE 13.8.
    // Changing either side means re-running the validator.
    expect(token('--color-money-in')).toBe('#1ba39a')
    expect(token('--color-money-out')).toBe('#e8603f')
  })

  test('chrome wears the brand light', () => {
    expect(token('--color-accent')).toBe('#2ee6a6')
    expect(token('--color-accent-b')).toBe('#37d5ff')
    expect(token('--color-accent-c')).toBe('#7a8cff')
  })

  test('the glass fill is the one the contrast was measured at', () => {
    expect(token('--glass-fill')).toBe('rgba(16, 18, 24, 0.5)')
  })

  test('text on glass clears 4.5:1 over the brightest measured backdrop', () => {
    const textTokens = [
      '--color-fg',
      '--color-fg-secondary',
      '--color-muted',
      '--color-money-in-text',
      '--color-money-out-text',
      '--color-danger',
    ]

    for (const name of textTokens) {
      expect(contrast(rgb(token(name)), BRIGHTEST_GLASS), name).toBeGreaterThanOrEqual(4.5)
    }
  })

  test('the dim tier is for icons: it clears 3:1 and nothing more is promised', () => {
    expect(contrast(rgb(token('--color-dim')), BRIGHTEST_GLASS)).toBeGreaterThanOrEqual(3)
  })

  test('labels on filled buttons clear 4.5:1', () => {
    const onAccent = rgb(token('--color-on-accent'))
    const white: [number, number, number] = [255, 255, 255]

    expect(contrast(onAccent, rgb(token('--color-accent')))).toBeGreaterThanOrEqual(4.5)
    expect(contrast(onAccent, rgb(token('--color-accent-b')))).toBeGreaterThanOrEqual(4.5)
    expect(contrast(white, rgb(token('--color-danger-fill-a')))).toBeGreaterThanOrEqual(4.5)
    expect(contrast(white, rgb(token('--color-danger-fill-b')))).toBeGreaterThanOrEqual(4.5)
  })

  test('the Reports categorical palette is unchanged', () => {
    const expected = ['#3987e5', '#d95926', '#199e70', '#c98500', '#d55181', '#008300', '#9085e9', '#e66767']

    expected.forEach((hex, index) => {
      expect(token(`--viz-${index + 1}`)).toBe(hex)
    })
  })

  test('the stylesheet is dark-only', () => {
    expect(css).not.toMatch(/prefers-color-scheme|html:not\(\.dark\)|@custom-variant dark/)
  })
})
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cd web && npx vitest run tests/tokens.test.ts`
Expected: FAIL — `--color-money-in is declared` (and the other new tokens) not found. The Reports palette and dark-only tests already pass.

- [ ] **Step 3: Replace the theme tokens**

In `web/src/index.css`, replace the entire `@theme { ... }` block (from `@theme {` through its closing `}`) with:

```css
@theme {
  /* Barlow and IBM Plex Mono carry no Greek glyphs; the next family in each
     stack does (see fonts.css), so the el locale never drops to a system face. */
  --font-sans: "Barlow", "Sofia Sans", "Helvetica Neue", Arial, sans-serif;
  --font-mono: "IBM Plex Mono", "JetBrains Mono", ui-monospace, Menlo, monospace;
  --font-display: "Sofia Sans", "Arial Black", "Helvetica Neue", sans-serif;

  /* Surfaces. Glass primitives paint --glass-fill; these solid steps stay for
     popovers and page-local panels that have no backdrop to blur. */
  --color-canvas: #05060a;
  --color-surface: #10131a;
  --color-surface-2: #161a22;
  --color-surface-elevated: #1c212b;
  --color-control: rgba(255, 255, 255, 0.045);
  --color-border: rgba(255, 255, 255, 0.09);
  --color-border-strong: rgba(255, 255, 255, 0.16);

  /* Ink, measured over the brightest backdrop a pane can sit on (see
     web/tests/tokens.test.ts). dim is for icons and inactive controls only. */
  --color-fg: #e7ebf1;
  --color-fg-secondary: #aeb5bf;
  --color-muted: #a3aab5;
  --color-dim: #848b98;

  /* Brand light: chrome only — primary buttons, active navigation, logo,
     focus. Never money. */
  --color-accent: #2ee6a6;
  --color-accent-b: #37d5ff;
  --color-accent-c: #7a8cff;
  --color-accent-hover: #5aefbc;
  --color-accent-soft: rgba(46, 230, 166, 0.14);
  --color-accent-muted: #10382d;
  --color-on-accent: #06110d;

  /* Ledger: anything that is money. The pair is validated; the text tints are
     what amounts are set in. */
  --color-money-in: #1ba39a;
  --color-money-in-text: #6fd9c4;
  --color-money-in-soft: rgba(27, 163, 154, 0.14);
  --color-money-out: #e8603f;
  --color-money-out-text: #ff9b7e;
  --color-money-out-soft: rgba(232, 96, 63, 0.13);

  /* Status. Always shipped with an icon and a label, so coral money-out is
     never read as an error. */
  --color-danger: #ff8295;
  --color-danger-text: #ffd3da;
  --color-danger-soft: rgba(255, 77, 103, 0.12);
  --color-danger-fill-a: #d7304c;
  --color-danger-fill-b: #b8327a;
  --color-success: #2ee6a6;
  --color-success-soft: rgba(46, 230, 166, 0.13);
  --color-warning: #e2f23a;
  --color-warning-soft: rgba(226, 242, 58, 0.12);
  --color-info: #37d5ff;
  --color-info-soft: rgba(55, 213, 255, 0.13);

  --radius-control: 12px;
  --control-h: 2.5rem;
  --sidebar-w: 13.75rem;
}

/* Glass and aurora knobs. The fill and the veil were measured for contrast;
   read the spec's "Measured, not assumed" note before changing either. */
:root {
  --glass-fill: rgba(16, 18, 24, 0.5);
  --glass-edge: rgba(255, 255, 255, 0.09);
  --glass-highlight: rgba(255, 255, 255, 0.08);
  --aurora-veil-top: 0.5;
  --aurora-veil-mid: 0.28;
}
```

Leave the existing `:root { --viz-1 ... --viz-other }` block exactly as it is.

- [ ] **Step 4: Add the glass utilities and the focus ring**

Inside the existing `@layer base { ... }` block, add after the `html { ... }` rule:

```css
  /* One focus ring for everything interactive: the brand cyan, never removed
     without a replacement (controls draw their own glow). */
  :focus-visible {
    outline: 2px solid var(--color-accent-b);
    outline-offset: 2px;
  }
```

After the `@layer base { ... }` block, add:

```css
@layer components {
  /* Every panel. The fill is measured: text on it keeps 4.5:1 wherever the
     aurora drifts. */
  .glass-pane {
    background: var(--glass-fill);
    -webkit-backdrop-filter: blur(28px) saturate(170%);
    backdrop-filter: blur(28px) saturate(170%);
    border: 1px solid var(--glass-edge);
    box-shadow:
      inset 0 1px 0 var(--glass-highlight),
      0 24px 60px rgba(0, 0, 0, 0.35);
  }

  /* Dialogs float above a scrim, so they sit on a darker backdrop than panes
     and can afford a stronger, lifted edge. */
  .glass-dialog {
    background: rgba(20, 22, 30, 0.66);
    -webkit-backdrop-filter: blur(32px) saturate(170%);
    backdrop-filter: blur(32px) saturate(170%);
    box-shadow:
      inset 0 1px 0 rgba(255, 255, 255, 0.12),
      0 0 0 1px rgba(255, 255, 255, 0.1),
      0 40px 120px rgba(0, 0, 0, 0.6),
      0 0 80px rgba(55, 213, 255, 0.08);
  }

  /* Behind a dialog: the page stays visible, softly, so nobody loses their place. */
  .glass-scrim {
    background: rgba(4, 5, 8, 0.42);
    -webkit-backdrop-filter: blur(10px) saturate(120%);
    backdrop-filter: blur(10px) saturate(120%);
  }
}

/* Where WebKit reports Reduce Transparency, glass turns solid. */
@media (prefers-reduced-transparency: reduce) {
  .glass-pane,
  .glass-dialog {
    -webkit-backdrop-filter: none;
    backdrop-filter: none;
    background: var(--color-surface);
  }

  .glass-scrim {
    -webkit-backdrop-filter: none;
    backdrop-filter: none;
    background: rgba(4, 5, 8, 0.78);
  }
}
```

- [ ] **Step 5: Re-validate the Reports palette against the glass surface**

The spec requires it. Load the `dataviz` skill: the base directory it reports on load contains `scripts/validate_palette.js`. From that directory:

Run: `node scripts/validate_palette.js "#3987e5,#d95926,#199e70,#c98500,#d55181,#008300,#9085e9,#e66767" --mode dark --surface "#11141b"`
Expected (recorded 2026-09-14): `ALL CHECKS PASS` — worst adjacent CVD ΔE 8.4 (protan), normal-vision ΔE 19.3. If it does not pass, stop and report; do not change the palette in this phase.

- [ ] **Step 6: Verify**

Run: `cd web && npx vitest run tests/tokens.test.ts`
Expected: PASS, 8 tests.

Run: `cd web && npx tsc -b && npm run lint && npm test && npm run build`
Expected: all green.

- [ ] **Step 7: Commit**

```bash
git add web/src/index.css web/tests/tokens.test.ts
git commit -m "feat(web): Aurora glass tokens, measured for contrast" -m "Money gets the validated Ledger pair and chrome keeps the brand light, as two token families that never mix. The glass fill and text tiers are not the mockup's: measured over the drifting aurora, the mockup's 0.42 fill left secondary text at 4.0:1 and captions at 3.0:1, so the fill is 0.5 and the greys are lighter. The destructive fill deepens for the same reason (white on #FF4D67 was 3.2:1). tokens.test.ts recomputes every ratio from the stylesheet, so a later tweak cannot quietly break it."
```

---

### Task 5: The motion gate and the Aurora layer

**Files:**
- Create: `web/src/lib/motion.ts`, `web/src/lib/motion.test.ts`
- Create: `web/src/components/Aurora.tsx`, `web/src/components/Aurora.test.tsx`
- Modify: `web/src/index.css`

**Interfaces:**
- Consumes: Task 4's `--color-canvas`, `--font-display`, `--aurora-veil-top`, `--aurora-veil-mid`.
- Produces: `useMotionAllowed(): boolean` from `web/src/lib/motion.ts`; `Aurora({ watermark?: boolean })` from `web/src/components/Aurora.tsx`, rendering a root element with `aria-hidden="true"` and `data-moving="true" | "false"`.

- [ ] **Step 1: Write the failing hook test**

Create `web/src/lib/motion.test.ts`:

```ts
/** @vitest-environment jsdom */

import { act, renderHook } from '@testing-library/react'
import { afterEach, describe, expect, test, vi } from 'vitest'

import { useMotionAllowed } from './motion'

function setVisibility(state: DocumentVisibilityState) {
  Object.defineProperty(document, 'visibilityState', { value: state, configurable: true })
}

function mockReducedMotion(matches: boolean) {
  Object.defineProperty(window, 'matchMedia', {
    configurable: true,
    value: vi.fn().mockReturnValue({
      matches,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    }),
  })
}

afterEach(() => {
  vi.restoreAllMocks()
  setVisibility('visible')
  Reflect.deleteProperty(window, 'matchMedia')
})

describe('useMotionAllowed', () => {
  test('allows motion in a visible, focused window with no preference', () => {
    vi.spyOn(document, 'hasFocus').mockReturnValue(true)

    const { result } = renderHook(() => useMotionAllowed())

    expect(result.current).toBe(true)
  })

  test('stops when the window loses focus', () => {
    const hasFocus = vi.spyOn(document, 'hasFocus').mockReturnValue(true)
    const { result } = renderHook(() => useMotionAllowed())

    act(() => {
      hasFocus.mockReturnValue(false)
      window.dispatchEvent(new Event('blur'))
    })

    expect(result.current).toBe(false)
  })

  test('stops when the page is hidden', () => {
    vi.spyOn(document, 'hasFocus').mockReturnValue(true)
    const { result } = renderHook(() => useMotionAllowed())

    act(() => {
      setVisibility('hidden')
      document.dispatchEvent(new Event('visibilitychange'))
    })

    expect(result.current).toBe(false)
  })

  test('never moves when the user asks for reduced motion', () => {
    vi.spyOn(document, 'hasFocus').mockReturnValue(true)
    mockReducedMotion(true)

    const { result } = renderHook(() => useMotionAllowed())

    expect(result.current).toBe(false)
  })

  test('survives a webview without matchMedia', () => {
    vi.spyOn(document, 'hasFocus').mockReturnValue(true)

    const { result } = renderHook(() => useMotionAllowed())

    expect(result.current).toBe(true)
  })
})
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cd web && npx vitest run src/lib/motion.test.ts`
Expected: FAIL — `Failed to resolve import "./motion"`.

- [ ] **Step 3: Implement the hook**

Create `web/src/lib/motion.ts`:

```ts
import { useEffect, useState } from 'react'

const REDUCED_MOTION = '(prefers-reduced-motion: reduce)'

function reducedMotionQuery(): MediaQueryList | null {
  // jsdom, and some embedded webviews, ship without matchMedia. Treat that as
  // "no preference" instead of crashing the shell.
  return typeof window.matchMedia === 'function' ? window.matchMedia(REDUCED_MOTION) : null
}

function readMotionAllowed(): boolean {
  const reduced = reducedMotionQuery()?.matches ?? false
  return !reduced && document.visibilityState === 'visible' && document.hasFocus()
}

/**
 * True while ambient motion may run: the user has not asked for reduced
 * motion, the page is visible and the window has focus. A background window
 * holds still, so the aurora never spends GPU time nobody is watching.
 */
export function useMotionAllowed(): boolean {
  const [allowed, setAllowed] = useState(readMotionAllowed)

  useEffect(() => {
    const update = () => setAllowed(readMotionAllowed())
    const query = reducedMotionQuery()

    query?.addEventListener('change', update)
    document.addEventListener('visibilitychange', update)
    window.addEventListener('focus', update)
    window.addEventListener('blur', update)

    return () => {
      query?.removeEventListener('change', update)
      document.removeEventListener('visibilitychange', update)
      window.removeEventListener('focus', update)
      window.removeEventListener('blur', update)
    }
  }, [])

  return allowed
}
```

Run: `cd web && npx vitest run src/lib/motion.test.ts`
Expected: PASS, 5 tests.

- [ ] **Step 4: Write the failing component test**

Create `web/src/components/Aurora.test.tsx`:

```tsx
/** @vitest-environment jsdom */

import '@testing-library/jest-dom/vitest'
import { cleanup, render } from '@testing-library/react'
import { afterEach, describe, expect, test, vi } from 'vitest'

import { Aurora } from './Aurora'

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('Aurora', () => {
  test('is decorative and hidden from assistive technology', () => {
    const { container } = render(<Aurora />)

    expect(container.firstElementChild).toHaveAttribute('aria-hidden', 'true')
  })

  test('drifts while the window has focus', () => {
    vi.spyOn(document, 'hasFocus').mockReturnValue(true)

    const { container } = render(<Aurora />)

    expect(container.firstElementChild).toHaveAttribute('data-moving', 'true')
  })

  test('holds still in a background window', () => {
    vi.spyOn(document, 'hasFocus').mockReturnValue(false)

    const { container } = render(<Aurora />)

    expect(container.firstElementChild).toHaveAttribute('data-moving', 'false')
  })

  test('shows the Greek wordmark unless asked not to', () => {
    const { container, rerender } = render(<Aurora />)
    expect(container).toHaveTextContent('ΟΙΚΟΝΟΜΙΑ')

    rerender(<Aurora watermark={false} />)
    expect(container).not.toHaveTextContent('ΟΙΚΟΝΟΜΙΑ')
  })
})
```

- [ ] **Step 5: Run it and watch it fail**

Run: `cd web && npx vitest run src/components/Aurora.test.tsx`
Expected: FAIL — `Failed to resolve import "./Aurora"`.

- [ ] **Step 6: Implement the component**

Create `web/src/components/Aurora.tsx`:

```tsx
import { useMotionAllowed } from '../lib/motion'

/**
 * The ambient light behind every glass pane: five slow radial blobs, a veil
 * that keeps text sitting directly on the aurora legible, and the Greek
 * wordmark. Purely decorative, so it is hidden from assistive technology, and
 * it holds still whenever useMotionAllowed says nobody should pay for motion.
 */
export function Aurora({ watermark = true }: { watermark?: boolean }) {
  const moving = useMotionAllowed()

  return (
    <div className="aurora" data-moving={moving} aria-hidden="true">
      <div className="aurora-blobs">
        <i />
        <i />
        <i />
        <i />
        <i />
      </div>

      <div className="aurora-veil" />

      {watermark ? <div className="aurora-watermark">ΟΙΚΟΝΟΜΙΑ</div> : null}
    </div>
  )
}
```

- [ ] **Step 7: Paint the aurora**

In `web/src/index.css`, add after the `@media (prefers-reduced-transparency: reduce)` block from Task 4:

```css
@layer components {
  .aurora {
    position: fixed;
    inset: 0;
    z-index: 0;
    overflow: hidden;
    pointer-events: none;
    background: var(--color-canvas);
  }

  /* One blur over all five blobs; each blob only ever animates transform, so
     the drift stays on the compositor. */
  .aurora-blobs {
    position: absolute;
    inset: -80px;
    filter: blur(70px) saturate(1.35);
  }

  .aurora-blobs i {
    position: absolute;
    border-radius: 50%;
    mix-blend-mode: screen;
    will-change: transform;
  }

  .aurora-blobs i:nth-child(1) {
    left: 14%;
    top: 5%;
    width: 48%;
    height: 52%;
    background: radial-gradient(closest-side, rgba(46, 230, 166, 0.55), transparent);
    animation: aurora-drift-a 22s ease-in-out infinite alternate;
  }

  .aurora-blobs i:nth-child(2) {
    left: 48%;
    top: -5%;
    width: 55%;
    height: 57%;
    background: radial-gradient(closest-side, rgba(55, 213, 255, 0.42), transparent);
    animation: aurora-drift-b 26s ease-in-out infinite alternate;
  }

  .aurora-blobs i:nth-child(3) {
    left: 69%;
    top: 45%;
    width: 44%;
    height: 57%;
    background: radial-gradient(closest-side, rgba(122, 140, 255, 0.5), transparent);
    animation: aurora-drift-a 30s ease-in-out infinite alternate-reverse;
  }

  .aurora-blobs i:nth-child(4) {
    left: 23%;
    top: 70%;
    width: 48%;
    height: 48%;
    background: radial-gradient(closest-side, rgba(255, 79, 163, 0.34), transparent);
    animation: aurora-drift-b 24s ease-in-out infinite alternate-reverse;
  }

  .aurora-blobs i:nth-child(5) {
    left: -3%;
    top: 45%;
    width: 33%;
    height: 52%;
    background: radial-gradient(closest-side, rgba(255, 176, 46, 0.22), transparent);
    animation: aurora-drift-a 28s ease-in-out infinite alternate;
  }

  .aurora[data-moving="false"] .aurora-blobs i {
    animation-play-state: paused;
  }

  /* Page titles and descriptions sit directly on the aurora with no pane to
     darken it. The brightest blob is top-left, so the veil is strongest there. */
  .aurora-veil {
    position: absolute;
    inset: 0;
    background: linear-gradient(
      180deg,
      rgba(5, 6, 10, var(--aurora-veil-top)) 0%,
      rgba(5, 6, 10, var(--aurora-veil-mid)) 28%,
      rgba(5, 6, 10, 0) 55%
    );
  }

  .aurora-watermark {
    position: absolute;
    left: 18%;
    bottom: -7%;
    white-space: nowrap;
    font: 900 clamp(140px, 18vw, 260px) / 1 var(--font-display);
    color: rgba(255, 255, 255, 0.025);
    -webkit-text-stroke: 1px rgba(255, 255, 255, 0.07);
  }
}

@keyframes aurora-drift-a {
  to {
    transform: translate(90px, 40px) scale(1.12);
  }
}

@keyframes aurora-drift-b {
  to {
    transform: translate(-80px, 50px) scale(0.92);
  }
}

@media (prefers-reduced-motion: reduce) {
  .aurora-blobs i {
    animation: none;
  }
}
```

- [ ] **Step 8: Verify**

Run: `cd web && npx vitest run src/lib/motion.test.ts src/components/Aurora.test.tsx`
Expected: PASS, 9 tests.

Run: `cd web && npx tsc -b && npm run lint && npm test`
Expected: all green. (Nothing mounts `Aurora` yet; Task 9 does.)

- [ ] **Step 9: Commit**

```bash
git add web/src/lib/motion.ts web/src/lib/motion.test.ts web/src/components/Aurora.tsx web/src/components/Aurora.test.tsx web/src/index.css
git commit -m "feat(web): the Aurora layer, paused whenever nobody is watching" -m "The aurora is the colour behind every glass pane. It animates transform only so the drift stays on the compositor, and useMotionAllowed stops it in a background or hidden window and never starts it under Reduce Motion, so ambient light never costs battery unseen. A top veil keeps page titles readable where they sit directly on the brightest blob."
```

---

### Task 6: Glass primitives

**Files:**
- Modify: `web/src/components/ui.tsx` (full replacement below)
- Modify: `web/src/components/ui.test.tsx`
- Modify: `web/src/index.css`

**Interfaces:**
- Consumes: Task 4's tokens and `.glass-pane`.
- Produces: every existing export of `ui.tsx` with unchanged names and props: `IconBadge`, `Card`, `Hero`, `Panel`, `CollapsibleSection`, `Button`, `Input`, `Select`, `Label`, `Field`, `PageHeader`, `ErrorBanner`, `EmptyState`, `ChoiceCard`, `MetricCard`, `Segmented`, `FlowBar`, `ListRow`. Two styling hooks later code must keep: text-like controls carry the class `ui-control`; `Field` renders `<label class="field-box ...">`.

- [ ] **Step 1: Write the failing tests**

In `web/src/components/ui.test.tsx`, change the import `import { ErrorBanner } from './ui'` to `import { Button, ErrorBanner, Field, Input, Select } from './ui'`, then add at the end of the file:

```tsx
describe('Field', () => {
  test('still labels the control it wraps', () => {
    render(
      <Field label="Amount">
        <Input defaultValue="25,50" />
      </Field>,
    )

    expect(screen.getByLabelText('Amount')).toHaveValue('25,50')
  })

  test('carries the hooks the glass field box is styled by', () => {
    render(
      <Field label="Account">
        <Select defaultValue="bank" aria-label="Account">
          <option value="bank">Bank</option>
        </Select>
      </Field>,
    )

    // index.css draws one glass box for .field-box:has(.ui-control) and strips
    // the control's own box. Renaming either class silently breaks every form.
    // (Selects are found by aria-label, as in the app: option text would leak
    // into a wrapping label's text.)
    const control = screen.getByRole('combobox', { name: 'Account' })
    expect(control).toHaveClass('ui-control')
    expect(control.closest('label')).toHaveClass('field-box')
  })
})

describe('Button', () => {
  test('a busy button keeps its name and cannot be pressed twice', () => {
    render(<Button busy>Save entry</Button>)

    expect(screen.getByRole('button', { name: 'Save entry' })).toBeDisabled()
  })
})
```

- [ ] **Step 2: Run them and watch the hook test fail**

Run: `cd web && npx vitest run src/components/ui.test.tsx`
Expected: FAIL — `carries the hooks the glass field box is styled by`: the element lacks class `ui-control`. The other new tests pass already; they guard behaviour this task must not break.

- [ ] **Step 3: Replace `web/src/components/ui.tsx`**

Replace the whole file with:

```tsx
import { ChevronDown, CircleAlert, Loader2 } from 'lucide-react'
import { useState } from 'react'
import type {
  ButtonHTMLAttributes,
  InputHTMLAttributes,
  ReactNode,
  SelectHTMLAttributes,
} from 'react'

import { cn } from '../lib/cn'

/**
 * The shared control look. `ui-control` is a styling hook, not decoration:
 * index.css turns a Field around a control into one glass field box and strips
 * the control's own box, so every text-like control must keep the class.
 */
const controlBase =
  'ui-control h-10 w-full rounded-[var(--radius-control)] border border-[var(--color-border)] bg-[var(--color-control)] px-3 text-sm text-[var(--color-fg)] outline-none transition placeholder:text-[var(--color-muted)] focus:border-[var(--color-accent-b)] focus:ring-4 focus:ring-[var(--color-accent-b)]/15 aria-[invalid=true]:border-[var(--color-danger)] disabled:opacity-50'

/** Tinted icon chip used across metric cards and activity rows. */
export function IconBadge({
  children,
  tone = 'accent',
  size = 'md',
  className = '',
}: {
  children: ReactNode
  tone?: 'accent' | 'success' | 'danger' | 'warning' | 'info' | 'muted'
  size?: 'xs' | 'sm' | 'md'
  className?: string
}) {
  const tones = {
    accent: 'bg-[var(--color-accent-soft)] text-[var(--color-accent)]',
    success: 'bg-[var(--color-success-soft)] text-[var(--color-success)]',
    danger: 'bg-[var(--color-danger-soft)] text-[var(--color-danger)]',
    warning: 'bg-[var(--color-warning-soft)] text-[var(--color-warning)]',
    info: 'bg-[var(--color-info-soft)] text-[var(--color-info)]',
    muted: 'bg-white/[0.06] text-[var(--color-dim)]',
  }
  const sizes = {
    xs: 'size-6',
    sm: 'size-8',
    md: 'size-9',
  }

  return (
    <span
      className={cn(
        'inline-flex shrink-0 items-center justify-center rounded-[11px]',
        tones[tone],
        sizes[size],
        className,
      )}
    >
      {children}
    </span>
  )
}

/** The glass panel every block of content sits on. */
export function Card({
  children,
  className = '',
  padding = 'md',
}: {
  children: ReactNode
  className?: string
  padding?: 'none' | 'sm' | 'md' | 'lg'
}) {
  const pad = {
    none: '',
    sm: 'p-4',
    md: 'p-5',
    lg: 'p-6 sm:p-8',
  }[padding]

  return <div className={cn('glass-pane rounded-[22px]', pad, className)}>{children}</div>
}

/**
 * The glass pane for a page's key figure. It carries no colour wash of its
 * own: the aurora shows through, and text keeps its measured contrast.
 * `accent` is still accepted so existing callers compile unchanged.
 */
export function Hero({
  children,
  className = '',
}: {
  children: ReactNode
  className?: string
  accent?: 'accent' | 'success' | 'neutral'
}) {
  return (
    <div className={cn('glass-pane relative overflow-hidden rounded-[24px]', className)}>
      <div className="relative">{children}</div>
    </div>
  )
}

/** Glass panel with a title bar — activity lists, report blocks, etc. */
export function Panel({
  title,
  description,
  whisper,
  icon,
  actions,
  children,
  className = '',
}: {
  title: string
  description?: string
  /** Quieter second line under list meta (export omit reminder). */
  whisper?: string
  icon?: ReactNode
  actions?: ReactNode
  children: ReactNode
  className?: string
}) {
  return (
    <div className={cn('glass-pane overflow-hidden rounded-[22px]', className)}>
      <div className="flex items-center justify-between gap-3 border-b border-[var(--color-border)] px-5 py-4">
        <div className="min-w-0">
          <h3 className="text-[15px] font-semibold text-[var(--color-fg)]">{title}</h3>
          {description ? (
            <p className="text-[13px] text-[var(--color-muted)]">{description}</p>
          ) : null}
          {whisper ? <p className="text-xs text-[var(--color-muted)]">{whisper}</p> : null}
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {actions}
          {icon ? <span className="text-[var(--color-dim)]">{icon}</span> : null}
        </div>
      </div>
      {children}
    </div>
  )
}

/**
 * Collapsible glass section: the header row is always visible with a chevron
 * at the end; the body renders only while expanded. Collapsed by default.
 */
export function CollapsibleSection({
  title,
  description,
  icon,
  tone = 'accent',
  defaultOpen = false,
  flush = false,
  children,
}: {
  title: string
  description?: string
  icon?: ReactNode
  tone?: 'accent' | 'success' | 'danger' | 'warning' | 'info' | 'muted'
  defaultOpen?: boolean
  /** Body without padding, for lists that manage their own edges. */
  flush?: boolean
  children: ReactNode
}) {
  const [open, setOpen] = useState(defaultOpen)

  return (
    <div className="glass-pane overflow-hidden rounded-[22px]">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        className="flex w-full items-center gap-3 px-5 py-4 text-left transition hover:bg-white/[0.04]"
      >
        {icon ? (
          <IconBadge tone={tone} size="sm">
            {icon}
          </IconBadge>
        ) : null}
        <div className="min-w-0 flex-1">
          <h3 className="text-[15px] font-semibold text-[var(--color-fg)]">{title}</h3>
          {description ? (
            <p className="text-[13px] text-[var(--color-muted)]">{description}</p>
          ) : null}
        </div>
        <ChevronDown
          className={cn(
            'size-4 shrink-0 text-[var(--color-dim)] transition-transform duration-200',
            open && 'rotate-180',
          )}
        />
      </button>

      {open ? (
        <div className={cn('border-t border-[var(--color-border)]', !flush && 'p-5 sm:p-6')}>
          {children}
        </div>
      ) : null}
    </div>
  )
}

export function Button({
  variant = 'primary',
  size = 'md',
  className = '',
  busy = false,
  disabled,
  children,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: 'primary' | 'secondary' | 'danger' | 'ghost'
  size?: 'sm' | 'md' | 'icon'
  /** Shows a spinner and disables the button while a slow action runs. */
  busy?: boolean
}) {
  const styles = {
    // Dark ink on the brand light: 11.9:1.
    primary:
      'bg-[linear-gradient(90deg,var(--color-accent),var(--color-accent-b))] font-semibold text-[var(--color-on-accent)] shadow-[0_8px_26px_rgba(46,230,166,0.28),inset_0_1px_0_rgba(255,255,255,0.35)] hover:brightness-110',
    secondary:
      'bg-white/[0.06] text-[var(--color-fg)] shadow-[inset_0_0_0_1px_rgba(255,255,255,0.1),inset_0_1px_0_rgba(255,255,255,0.08)] hover:bg-white/[0.1]',
    // White on the deepened fill: 4.75:1 at its lightest stop. Hover glows
    // instead of brightening, which would drop the label below 4.5:1.
    danger:
      'bg-[linear-gradient(90deg,var(--color-danger-fill-a),var(--color-danger-fill-b))] font-semibold text-white shadow-[0_8px_26px_rgba(215,48,76,0.3),inset_0_1px_0_rgba(255,255,255,0.25)] hover:shadow-[0_10px_34px_rgba(215,48,76,0.5),inset_0_1px_0_rgba(255,255,255,0.25)]',
    ghost: 'text-[var(--color-muted)] hover:bg-white/[0.06] hover:text-[var(--color-fg)]',
  }
  const sizes = {
    sm: 'h-8 gap-1.5 px-3 text-xs',
    md: 'h-10 gap-2 px-4 text-sm',
    icon: 'h-10 w-10 shrink-0 justify-center p-0',
  }

  return (
    <button
      type="button"
      className={cn(
        'inline-flex items-center justify-center rounded-[var(--radius-control)] font-medium transition disabled:cursor-not-allowed disabled:opacity-50 disabled:shadow-none disabled:brightness-100',
        styles[variant],
        sizes[size],
        className,
      )}
      disabled={disabled || busy}
      {...props}
    >
      {busy ? <Loader2 className="size-3.5 shrink-0 animate-spin" /> : null}
      {children}
    </button>
  )
}

export function Input({ className = '', ...props }: InputHTMLAttributes<HTMLInputElement>) {
  return <input className={cn(controlBase, className)} {...props} />
}

export function Select({
  className = '',
  children,
  ...props
}: SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <div className={cn('relative', className)}>
      <select className={cn(controlBase, 'ui-select cursor-pointer pr-9')} {...props}>
        {children}
      </select>
      <ChevronDown
        className="pointer-events-none absolute top-1/2 right-2.5 size-4 -translate-y-1/2 text-[var(--color-dim)]"
        strokeWidth={1.75}
        aria-hidden
      />
    </div>
  )
}

export function Label({ children }: { children: ReactNode }) {
  return (
    <span className="mb-1 block font-mono text-[9.5px] font-medium tracking-[0.14em] text-[var(--color-muted)] uppercase">
      {children}
    </span>
  )
}

/** A labelled control. Around a `ui-control` it renders as one glass field box. */
export function Field({
  label,
  children,
  className = '',
}: {
  label: string
  children: ReactNode
  className?: string
}) {
  return (
    <label className={cn('field-box block min-w-0', className)}>
      <Label>{label}</Label>
      {children}
    </label>
  )
}

/**
 * Page chrome: a mono eyebrow or breadcrumb, a large title, a description and
 * optional actions or meta. It sits directly on the aurora, under the veil.
 */
export function PageHeader({
  eyebrow,
  breadcrumb,
  title,
  description,
  actions,
  meta,
}: {
  eyebrow?: string
  /** Sub-view trail (Transactions / Recurring). Last crumb is the current page. */
  breadcrumb?: Array<{ label: string; onClick?: () => void }>
  title: string
  description?: string
  actions?: ReactNode
  meta?: ReactNode
}) {
  const hasLead = Boolean(eyebrow || (breadcrumb && breadcrumb.length > 0))

  return (
    <div className="mb-6 flex flex-wrap items-end justify-between gap-4">
      <div className="min-w-0">
        {breadcrumb && breadcrumb.length > 0 ? (
          <nav
            className="flex flex-wrap items-center gap-1.5 font-mono text-[10.5px] font-medium tracking-[0.16em] text-[var(--color-muted)] uppercase"
            aria-label={breadcrumb.map((c) => c.label).join(' / ')}
          >
            {breadcrumb.map((crumb, i) => (
              <span key={`${crumb.label}-${i}`} className="inline-flex items-center gap-1.5">
                {i > 0 ? <span aria-hidden>/</span> : null}
                {crumb.onClick ? (
                  <button
                    type="button"
                    onClick={crumb.onClick}
                    className="hover:text-[var(--color-fg)]"
                  >
                    {crumb.label}
                  </button>
                ) : (
                  <span className="text-[var(--color-fg-secondary)]">{crumb.label}</span>
                )}
              </span>
            ))}
          </nav>
        ) : eyebrow ? (
          <p className="font-mono text-[10.5px] font-medium tracking-[0.16em] text-[var(--color-muted)] uppercase">
            {eyebrow}
          </p>
        ) : null}
        <h2
          className={cn(
            'leading-tight font-semibold tracking-tight text-[var(--color-fg)]',
            hasLead ? 'mt-1.5 text-[1.875rem]' : 'text-[1.625rem]',
          )}
        >
          {title}
        </h2>
        {description ? (
          <p className="mt-1 text-sm text-[var(--color-fg-secondary)]">{description}</p>
        ) : null}
      </div>
      <div className="flex shrink-0 flex-wrap items-center gap-2">
        {meta ? <div className="text-xs text-[var(--color-muted)]">{meta}</div> : null}
        {actions}
      </div>
    </div>
  )
}

export function ErrorBanner({
  message,
  title,
  className = 'mb-4',
  id,
}: {
  message: string | null
  title?: string
  className?: string
  id?: string
}) {
  if (!message && !title) return null

  return (
    <div
      id={id}
      role="alert"
      aria-live="assertive"
      className={cn(
        'flex items-start gap-2 rounded-[14px] bg-[var(--color-danger-soft)] px-4 py-3 text-sm text-[var(--color-danger-text)] shadow-[inset_0_0_0_1px_rgba(255,77,103,0.35)]',
        className,
      )}
    >
      {title ? (
        <CircleAlert className="mt-0.5 size-4 shrink-0 text-[var(--color-danger)]" aria-hidden />
      ) : null}
      <div className="min-w-0 flex-1">
        {title ? <p className="font-semibold">{title}</p> : null}
        {message ? <p className={title ? 'mt-0.5' : undefined}>{message}</p> : null}
      </div>
    </div>
  )
}

export function EmptyState({
  icon,
  title,
  body,
  action,
}: {
  icon?: ReactNode
  title: string
  body: string
  action?: ReactNode
}) {
  return (
    <div className="glass-pane flex flex-col items-center rounded-[22px] px-6 py-16 text-center">
      {icon ? (
        <div className="oik-empty-icon mb-4 flex size-12 items-center justify-center rounded-[15px] bg-[var(--color-info-soft)] text-[var(--color-info)] shadow-[inset_0_0_0_1px_rgba(55,213,255,0.25),0_0_26px_rgba(55,213,255,0.15)]">
          {icon}
        </div>
      ) : null}
      <p className="oik-empty-title text-sm font-semibold text-[var(--color-fg)]">{title}</p>
      <p className="oik-empty-body mt-1.5 max-w-sm text-sm text-[var(--color-muted)]">{body}</p>
      {action ? <div className="mt-6">{action}</div> : null}
    </div>
  )
}

/** Selectable card for template / type choices (less typing). */
export function ChoiceCard({
  selected,
  onClick,
  icon,
  title,
  description,
}: {
  selected: boolean
  onClick: () => void
  icon: ReactNode
  title: string
  description?: string
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        'flex h-full min-h-[5.5rem] w-full flex-col items-start gap-2 rounded-[16px] p-4 text-left transition',
        selected
          ? 'bg-[var(--color-accent-soft)] shadow-[inset_0_0_0_1px_rgba(46,230,166,0.45),0_0_24px_rgba(46,230,166,0.12)]'
          : 'bg-white/[0.04] shadow-[inset_0_0_0_1px_rgba(255,255,255,0.08)] hover:bg-white/[0.07]',
      )}
    >
      <span
        className={cn(
          'flex size-8 items-center justify-center rounded-[10px]',
          selected
            ? 'bg-[linear-gradient(135deg,var(--color-accent),var(--color-accent-b))] text-[var(--color-on-accent)]'
            : 'bg-white/[0.06] text-[var(--color-dim)]',
        )}
      >
        {icon}
      </span>
      <span className="text-sm font-medium text-[var(--color-fg)]">{title}</span>
      {description ? (
        <span className="text-xs leading-snug text-[var(--color-muted)]">{description}</span>
      ) : null}
    </button>
  )
}

/** Dashboard-style metric tile. */
export function MetricCard({
  label,
  hint,
  value,
  icon,
  accent,
}: {
  label: string
  hint?: string
  value: string
  icon?: ReactNode
  accent?: 'success' | 'danger' | 'accent'
}) {
  const tone = accent === 'success' ? 'success' : accent === 'danger' ? 'danger' : 'accent'

  return (
    <div className="glass-pane rounded-[20px] p-5">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="font-mono text-[10.5px] font-medium tracking-[0.14em] text-[var(--color-muted)] uppercase">
            {label}
          </div>
          {hint ? <div className="mt-1 text-xs text-[var(--color-muted)]">{hint}</div> : null}
        </div>
        {icon ? (
          <IconBadge tone={tone} size="sm">
            {icon}
          </IconBadge>
        ) : null}
      </div>
      <div
        title={value}
        className={cn(
          'mt-5 truncate text-[26px] font-semibold tracking-tight tabular-nums',
          // "danger" marks money going out (the Expenses tile): it is set in the
          // Ledger out tint, never in the error colour.
          accent === 'danger' ? 'text-[var(--color-money-out-text)]' : 'text-[var(--color-fg)]',
        )}
      >
        {value}
      </div>
    </div>
  )
}

export function Segmented<T extends string>({
  value,
  onChange,
  options,
  className = '',
}: {
  value: T
  onChange: (v: T) => void
  options: Array<{ id: T; label: string; icon?: ReactNode }>
  className?: string
}) {
  return (
    <div
      className={cn(
        'inline-flex h-10 items-stretch gap-0.5 rounded-full bg-white/[0.04] p-1 shadow-[inset_0_0_0_1px_rgba(255,255,255,0.08)]',
        className,
      )}
    >
      {options.map((opt) => {
        const active = value === opt.id

        return (
          <button
            key={opt.id}
            type="button"
            aria-pressed={active}
            onClick={() => onChange(opt.id)}
            className={cn(
              'inline-flex items-center gap-1.5 rounded-full px-3.5 text-[13px] font-medium transition',
              active
                ? 'bg-white/10 text-[var(--color-fg)] shadow-[inset_0_1px_0_rgba(255,255,255,0.12)]'
                : 'text-[var(--color-muted)] hover:text-[var(--color-fg)]',
            )}
          >
            {opt.icon}
            {opt.label}
          </button>
        )
      })}
    </div>
  )
}

/** Horizontal ratio bar used in flow summaries. */
export function FlowBar({
  label,
  value,
  ratio,
  tone,
}: {
  label: string
  value: string
  ratio: number
  tone: 'success' | 'danger' | 'accent'
}) {
  const pct = Math.round(Math.min(1, Math.max(0, ratio)) * 100)

  // Every caller uses success and danger for money in and out, so the bars
  // wear the Ledger gradients rather than the status colours.
  const bar =
    tone === 'success'
      ? 'bg-[linear-gradient(90deg,#27bf93,var(--color-money-in))]'
      : tone === 'danger'
        ? 'bg-[linear-gradient(90deg,#f07a45,var(--color-money-out))]'
        : 'bg-[linear-gradient(90deg,var(--color-accent),var(--color-accent-b))]'

  return (
    <div>
      <div className="mb-1.5 flex items-baseline justify-between gap-3 text-sm">
        <span className="shrink-0 text-[var(--color-muted)]">{label}</span>
        <span
          title={value}
          className="min-w-0 truncate font-semibold tabular-nums text-[var(--color-fg)]"
        >
          {value}
        </span>
      </div>
      <div className="h-2 overflow-hidden rounded-full bg-white/[0.06]">
        <div
          className={cn('h-full rounded-full transition-all duration-500', bar)}
          style={{ width: `${pct}%` }}
        />
      </div>
    </div>
  )
}

/** List row with a hover surface that also shows for keyboard focus inside it. */
export function ListRow({ children, className = '' }: { children: ReactNode; className?: string }) {
  return (
    <li
      className={cn(
        'flex items-center gap-4 px-5 py-3.5 transition hover:bg-white/[0.04] focus-within:bg-white/[0.04]',
        className,
      )}
    >
      {children}
    </li>
  )
}
```

- [ ] **Step 4: Draw the glass field box**

At the end of `web/src/index.css`, add (outside any `@layer`):

```css
/* A Field around a control becomes one glass field box, label inside and above
   the value. Unlayered on purpose: these rules must beat the control's own
   Tailwind utilities, which live in a cascade layer. Every rule requires :has(),
   so an engine without it falls back to the control's own box. */
.field-box:has(.ui-control) {
  display: block;
  padding: 8px 12px 7px;
  border-radius: var(--radius-control);
  background: var(--color-control);
  box-shadow: inset 0 0 0 1px var(--color-border);
  transition:
    box-shadow 150ms ease,
    background-color 150ms ease;
}

.field-box:has(.ui-control) > span:first-child {
  margin-bottom: 2px;
}

.field-box:has(.ui-control):focus-within {
  background: rgba(255, 255, 255, 0.07);
  box-shadow:
    inset 0 0 0 1px rgba(55, 213, 255, 0.7),
    0 0 0 4px rgba(55, 213, 255, 0.14),
    0 0 24px rgba(55, 213, 255, 0.18);
}

.field-box:has(.ui-control[aria-invalid="true"]) {
  box-shadow:
    inset 0 0 0 1px rgba(255, 77, 103, 0.75),
    0 0 0 4px rgba(255, 77, 103, 0.12);
}

/* Inside the box the control is bare. Only the left padding goes: Select and
   DateInput keep right padding for their chevron and calendar button. */
.field-box:has(.ui-control) .ui-control {
  height: 24px;
  padding-left: 0;
  border: 0;
  border-radius: 0;
  background: transparent;
  box-shadow: none;
}
```

- [ ] **Step 5: Verify**

Run: `cd web && npx vitest run src/components/ui.test.tsx`
Expected: PASS, 6 tests.

Run: `cd web && npx tsc -b && npm run lint && npm test`
Expected: all green, no new lint warnings.

- [ ] **Step 6: Commit**

```bash
git add web/src/components/ui.tsx web/src/components/ui.test.tsx web/src/index.css
git commit -m "feat(web): glass treatment for the shared primitives" -m "Every page is built from ui.tsx, so restyling the primitives restyles the app without touching page code: panels become glass, buttons become brand-light and glass plates, fields put their label inside one glass box. The field box is driven by two class hooks rather than a prop, so the 46 existing Field call sites, Select and DateInput included, pick it up unchanged; a test pins both hooks. The Expenses tile and flow bars move from status colours to the Ledger."
```

---

### Task 7: Glass dialogs and popover

**Files:**
- Modify: `web/src/components/Modal.tsx`, `web/src/components/ConfirmDialog.tsx`, `web/src/components/DateInput.tsx`

**Interfaces:**
- Consumes: Task 4's `.glass-scrim` and `.glass-dialog`.
- Produces: no API change.

This task is presentation only: dialog roles, names, focus trapping and Escape handling are unchanged and already covered by the suites that open dialogs (`grep -rln "getByRole('dialog'" web/src` lists them). No new test.

- [ ] **Step 1: Restyle `Modal`**

In `web/src/components/Modal.tsx`:
- Replace `className="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-4"` with `className="glass-scrim fixed inset-0 z-50 flex items-center justify-center p-4"`.
- Replace `'flex max-h-[88vh] w-full flex-col overflow-hidden rounded-2xl border border-[var(--color-border)] bg-[var(--color-surface)] shadow-2xl outline-none',` with `'glass-dialog flex max-h-[88vh] w-full flex-col overflow-hidden rounded-[26px] outline-none',`.
- Replace `<h2 id={titleId} className="text-base font-semibold text-[var(--color-fg)]">` with `<h2 id={titleId} className="text-lg font-semibold text-[var(--color-fg)]">`.

- [ ] **Step 2: Restyle `ConfirmDialog`**

In `web/src/components/ConfirmDialog.tsx`:
- Replace `className="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-4"` with `className="glass-scrim fixed inset-0 z-50 flex items-center justify-center p-4"`.
- Replace `className="w-full max-w-md rounded-2xl border border-[var(--color-border)] bg-[var(--color-surface)] p-6 shadow-2xl outline-none"` with `className="glass-dialog w-full max-w-md rounded-[26px] p-6 outline-none"`.
- In the icon span, replace `flex size-10 shrink-0 items-center justify-center rounded-lg ${iconWrap}` with `flex size-10 shrink-0 items-center justify-center rounded-[12px] ${iconWrap}`.

- [ ] **Step 3: Restyle the `DateInput` calendar popover**

In `web/src/components/DateInput.tsx`, replace `className="absolute top-full left-0 z-30 mt-2 w-64 rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-3 shadow-2xl"` with `className="glass-dialog absolute top-full left-0 z-30 mt-2 w-64 rounded-[16px] p-3"`.

- [ ] **Step 4: Verify**

Run: `cd web && npx tsc -b && npm run lint && npm test`
Expected: all green.

- [ ] **Step 5: Commit**

```bash
git add web/src/components/Modal.tsx web/src/components/ConfirmDialog.tsx web/src/components/DateInput.tsx
git commit -m "feat(web): glass dialogs over a blurred page" -m "Dialogs keep the page visible behind a soft blur, so opening one never loses the reader's place. Roles, names, focus and Escape handling are unchanged."
```

---

### Task 8: Money amounts wear the Ledger

**Files:**
- Modify: `web/src/pages/TransactionsPage.tsx`, `web/src/pages/DashboardPage.tsx`, `web/src/pages/ReportsPage.tsx`, `web/src/pages/RecurringPage.tsx`, `web/src/components/CsvPreviewModal.tsx`

**Interfaces:**
- Consumes: Task 4's `--color-money-in-text`, `--color-money-out-text`, `--color-money-in-soft`.
- Produces: nothing new.

Every use of `--color-success` / `--color-danger` in the web code was classified. **Money** (changed here): the nine sites below. **Status** (left alone, correctly): `QuickAddApp.tsx` saved message, `CsvPreviewModal.tsx` row error, `DocumentDropZone.tsx` error, `ConfirmDialog.tsx` icon tones, `DateInput.tsx` invalid border, `ReportsPage.tsx` trial-balance difference, `QuickAddPage.tsx` error, and `App.tsx`'s error box (Task 9 restyles it). Presentation only; no new test.

- [ ] **Step 1: Transactions**

In `web/src/pages/TransactionsPage.tsx` replace

```tsx
                      kindLabel === 'expense'
                        ? 'text-[var(--color-danger)]'
                        : kindLabel === 'income'
                          ? 'text-[var(--color-success)]'
                          : 'text-[var(--color-fg)]',
```

with

```tsx
                      kindLabel === 'expense'
                        ? 'text-[var(--color-money-out-text)]'
                        : kindLabel === 'income'
                          ? 'text-[var(--color-money-in-text)]'
                          : 'text-[var(--color-fg)]',
```

- [ ] **Step 2: Dashboard (two sites)**

In `web/src/pages/DashboardPage.tsx` replace `net < 0 ? 'text-[var(--color-danger)]' : 'text-[var(--color-fg)]',` with `net < 0 ? 'text-[var(--color-money-out-text)]' : 'text-[var(--color-fg)]',`, and replace

```tsx
                    row.signedMinor < 0
                      ? 'text-[var(--color-danger)]'
                      : row.signedMinor > 0 && row.kind === 'income'
                        ? 'text-[var(--color-success)]'
                        : 'text-[var(--color-fg)]',
```

with

```tsx
                    row.signedMinor < 0
                      ? 'text-[var(--color-money-out-text)]'
                      : row.signedMinor > 0 && row.kind === 'income'
                        ? 'text-[var(--color-money-in-text)]'
                        : 'text-[var(--color-fg)]',
```

- [ ] **Step 3: Reports net result**

In `web/src/pages/ReportsPage.tsx` replace

```tsx
          tone === 'danger'
            ? 'text-[var(--color-danger)]'
            : tone === 'success'
              ? 'text-[var(--color-success)]'
              : 'text-[var(--color-fg)]',
```

with

```tsx
          tone === 'danger'
            ? 'text-[var(--color-money-out-text)]'
            : tone === 'success'
              ? 'text-[var(--color-money-in-text)]'
              : 'text-[var(--color-fg)]',
```

Leave `diff === 0 ? 'text-[var(--color-muted)]' : 'text-[var(--color-danger)]',` untouched: an unbalanced trial balance is an error, not money.

- [ ] **Step 4: Recurring and CSV preview**

In `web/src/pages/RecurringPage.tsx` replace `income ? 'text-[var(--color-success)]' : 'text-[var(--color-fg)]',` with `income ? 'text-[var(--color-money-in-text)]' : 'text-[var(--color-fg)]',`.

In `web/src/components/CsvPreviewModal.tsx` replace `income ? 'text-[var(--color-success)]' : 'text-[var(--color-fg)]',` with `income ? 'text-[var(--color-money-in-text)]' : 'text-[var(--color-fg)]',`, and replace `? 'bg-[var(--color-success-soft)] text-[var(--color-success)]'` with `? 'bg-[var(--color-money-in-soft)] text-[var(--color-money-in-text)]'`. Leave `<span className="text-[var(--color-danger)]">{row.error}</span>` untouched.

- [ ] **Step 5: Verify**

Run: `grep -rn "color-success\|color-danger" web/src --include='*.tsx' | grep -v test`
Expected: only the status sites listed above, plus `ui.tsx`.

Run: `cd web && npx tsc -b && npm run lint && npm test`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add web/src/pages web/src/components/CsvPreviewModal.tsx
git commit -m "feat(web): money amounts wear the Ledger colours" -m "Expenses were set in the error red and income in the success green, so an ordinary expense looked like a failure. Amounts now use the validated Ledger pair; genuine errors, including an unbalanced trial balance, keep the danger colour."
```

---

### Task 9: The glass shell

**Files:**
- Modify: `web/src/App.test.tsx`, `web/src/App.tsx`, `web/src/components/Logo.tsx`

**Interfaces:**
- Consumes: `Aurora` (Task 5), `Button` and tokens (Tasks 4 and 6).
- Produces: the book selector is a list of buttons, each named by the book's name and currency and marked with `aria-pressed`; navigation buttons keep their names and gain `aria-current="page"` when active.

- [ ] **Step 1: Write the failing test**

In `web/src/App.test.tsx`, inside the `describe('App shell', ...)` block added in Task 2, add:

```tsx
  test('switches books from the sidebar list', async () => {
    vi.mocked(api.entityList).mockResolvedValue([entity, { ...entity, id: 'e2', name: 'Household' }])
    render(<App />)

    const household = await screen.findByRole('button', { name: /Household/ })
    expect(household.getAttribute('aria-pressed')).toBe('false')

    await userEvent.click(household)

    expect(household.getAttribute('aria-pressed')).toBe('true')
    expect(screen.getByRole('button', { name: /Personal/ }).getAttribute('aria-pressed')).toBe('false')
  })
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cd web && npx vitest run src/App.test.tsx -t "switches books"`
Expected: FAIL — `Unable to find role="button" and name /Household/` (books are still options in a select).

- [ ] **Step 3: Mount the aurora behind loading and unlock**

In `web/src/App.tsx`:
- Add `import { Aurora } from './components/Aurora'` next to the other component imports.
- Change `import { Button, Select } from './components/ui'` to `import { Button } from './components/ui'`.
- Below the `NAV` constant, add:

  ```ts
  /** Identity dots for books in the sidebar, assigned in order: not money, not status. */
  const BOOK_DOTS = ['#2ee6a6', '#7a8cff', '#37d5ff', '#ffb02e', '#ff4fa3'] as const
  ```

- Replace

  ```tsx
  if (status === null) {
    return (
      <div className="flex h-full items-center justify-center bg-[var(--color-canvas)] text-sm text-[var(--color-muted)]">
        {t('common.loading')}
      </div>
    )
  }

  if (status === 'uninitialized' || status === 'locked') {
    return (
      <UnlockScreen
        status={status}
        supportEmail={info?.support_email}
        onUnlocked={(next) => {
          setStatus(next)
          void refresh()
        }}
      />
    )
  }
  ```

  with

  ```tsx
  if (status === null) {
    return (
      <div className="relative flex h-full items-center justify-center text-sm text-[var(--color-muted)]">
        <Aurora watermark={false} />
        <span className="relative z-10">{t('common.loading')}</span>
      </div>
    )
  }

  if (status === 'uninitialized' || status === 'locked') {
    return (
      <div className="relative h-full">
        <Aurora />
        <div className="relative z-10 h-full">
          <UnlockScreen
            status={status}
            supportEmail={info?.support_email}
            onUnlocked={(next) => {
              setStatus(next)
              void refresh()
            }}
          />
        </div>
      </div>
    )
  }
  ```

- [ ] **Step 4: Replace the unlocked shell chrome**

Still in `App.tsx`, in the final `return (` of the component, replace everything from that `return (` line down to (but **not** including) the line `        <main key={active} className="flex-1 overflow-auto">` with:

```tsx
  return (
    <div className="relative flex h-full min-h-0 text-[var(--color-fg)]">
      <Aurora />

      <aside className="glass-pane relative z-10 my-3 ml-3 flex w-[var(--sidebar-w)] shrink-0 flex-col gap-5 rounded-[22px] px-3 py-4">
        <div className="flex items-center gap-2.5 px-1.5">
          <Logo className="size-8 shrink-0" />
          <div className="min-w-0 leading-tight">
            <div className="truncate text-[15px] font-semibold tracking-tight">Oikonomia</div>
            <div className="truncate text-xs text-[var(--color-muted)]">{t('app.localLedger')}</div>
          </div>
        </div>

        <nav className="flex flex-col gap-0.5">
          {NAV.map((item) => {
            const isActive = active === item.id
            const Icon = item.icon

            return (
              <button
                key={item.id}
                type="button"
                onClick={() => setActive(item.id)}
                aria-current={isActive ? 'page' : undefined}
                className={cn(
                  'flex h-10 items-center gap-2.5 rounded-xl px-3 text-sm font-medium transition',
                  isActive
                    ? 'bg-[linear-gradient(90deg,rgba(46,230,166,0.2),rgba(55,213,255,0.08))] text-[var(--color-fg)] shadow-[inset_0_0_0_1px_rgba(46,230,166,0.35),0_0_24px_rgba(46,230,166,0.12)]'
                    : 'text-[var(--color-fg-secondary)] hover:bg-white/[0.05] hover:text-[var(--color-fg)]',
                )}
              >
                <Icon
                  className={cn(
                    'size-[1.125rem] shrink-0',
                    isActive ? 'text-[var(--color-accent)]' : 'text-[var(--color-dim)]',
                  )}
                  strokeWidth={1.75}
                />
                <span className="truncate">{t(item.labelKey)}</span>
              </button>
            )
          })}
        </nav>

        <section className="flex min-h-0 flex-1 flex-col gap-2" aria-labelledby="sidebar-books">
          <h2
            id="sidebar-books"
            className="px-1.5 font-mono text-[10.5px] font-medium tracking-[0.16em] text-[var(--color-muted)] uppercase"
          >
            {t('app.book')}
          </h2>

          {entities.length === 0 ? (
            <p className="px-1.5 text-sm text-[var(--color-muted)]">{t('app.noEntitiesYet')}</p>
          ) : (
            <ul className="flex min-h-0 flex-col gap-0.5 overflow-y-auto">
              {entities.map((book, index) => {
                const selected = book.id === entity?.id
                const dot = BOOK_DOTS[index % BOOK_DOTS.length]

                return (
                  <li key={book.id}>
                    <button
                      type="button"
                      onClick={() => setEntityId(book.id)}
                      aria-pressed={selected}
                      className={cn(
                        'flex h-9 w-full items-center gap-2.5 rounded-lg px-1.5 text-left text-sm transition',
                        selected
                          ? 'bg-white/[0.06] text-[var(--color-fg)]'
                          : 'text-[var(--color-fg-secondary)] hover:text-[var(--color-fg)]',
                      )}
                    >
                      <span
                        aria-hidden
                        className="size-2 shrink-0 rounded-full"
                        style={{ background: dot, boxShadow: `0 0 10px ${dot}` }}
                      />
                      <span className="min-w-0 flex-1 truncate">{book.name}</span>
                      <span className="font-mono text-[10.5px] text-[var(--color-muted)]">
                        {book.base_currency}
                      </span>
                    </button>
                  </li>
                )
              })}
            </ul>
          )}
        </section>

        <div className="px-1.5 font-mono text-[10.5px] tracking-[0.08em] text-[var(--color-muted)]">
          {info ? t('app.versionEncrypted', { version: info.version }) : 'Oikonomia'}
        </div>
      </aside>

      <div className="relative z-10 flex min-w-0 flex-1 flex-col">
        <TrialBanner license={license} />
        <header className="flex h-16 shrink-0 items-center justify-between gap-4 px-7">
          <div className="flex min-w-0 items-baseline gap-2.5">
            <span className="truncate text-xl font-semibold tracking-tight">
              {entity ? entity.name : t('app.noBookSelected')}
            </span>
            <span className="truncate text-sm text-[var(--color-fg-secondary)]">
              {entity
                ? t('app.entityChart', {
                    currency: entity.base_currency,
                    chart: t(`chart.${entity.chart_template}`),
                  })
                : t('app.createEntityInSettings')}
            </span>
          </div>
          <Button
            variant="secondary"
            onClick={() => void onLock()}
            disabled={locking}
            aria-label={t('app.lockVault')}
          >
            <Lock className="size-4" />
            {locking ? t('app.locking') : t('app.lock')}
          </Button>
        </header>

```

The nesting depth is unchanged, so the closing tags below `<main>` stay as they are. Then:
- Replace `<div className="mx-auto max-w-6xl px-6 py-8">` with `<div className="mx-auto max-w-6xl px-7 pt-2 pb-10">`.
- Replace `className="mb-5 rounded-xl border border-[var(--color-danger)]/30 bg-[var(--color-danger-soft)] px-4 py-3 text-sm text-[var(--color-danger)]"` with `className="mb-5 rounded-[14px] bg-[var(--color-danger-soft)] px-4 py-3 text-sm text-[var(--color-danger-text)] shadow-[inset_0_0_0_1px_rgba(255,77,103,0.35)]"`.

- [ ] **Step 5: The mark in the brand light**

In `web/src/components/Logo.tsx` change the six stop colours:
- `stopColor="#6fd695"` to `stopColor="#7ff3cb"` and `stopColor="#1b7a45"` to `stopColor="#2bbfd8"` (the shield: emerald into cyan).
- `stopColor="#17171d"` to `stopColor="#161a21"` and `stopColor="#09090b"` to `stopColor="#0b0c10"` (the plate).
- Both `stopColor="#35b06b"` to `stopColor="#37d5ff"` (the glow).

- [ ] **Step 6: Verify**

Run: `cd web && npx vitest run src/App.test.tsx`
Expected: PASS, including `switches books from the sidebar list` and `is dark-only`.

Run: `cd web && npx tsc -b && npm run lint && npm test`
Expected: all green.

- [ ] **Step 7: Commit**

```bash
git add web/src/App.tsx web/src/App.test.tsx web/src/components/Logo.tsx
git commit -m "feat(web): the glass shell over the aurora" -m "The sidebar becomes a floating glass pane over the aurora, and the book dropdown becomes a list: switching books is now one click and every book is visible at once. The aurora also sits behind loading and unlock, so the app never flashes a flat screen on its way in."
```

---

### Task 10: Measure, verify and open the pull request

**Files:**
- Create (outside the repository, in a scratch directory): `aurora-qa/package.json`, `aurora-qa/qa.mjs`
- Possibly modify, only if measurements require it: `web/src/index.css` (`--glass-fill`, `--aurora-veil-top`, `--aurora-veil-mid`) and `web/tests/tokens.test.ts` (`BRIGHTEST_GLASS`)

**Interfaces:**
- Consumes: everything above.
- Produces: a pull request with measured contrast, screenshots and performance numbers.

- [ ] **Step 1: Run every CI gate**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo deny check
./scripts/assert-core-offline.sh
cd web && npm ci && npx tsc -b && npm run lint && npm test && npm run build
```

Expected: all green. `cargo test --workspace` takes around ten minutes.

- [ ] **Step 2: Set up the visual QA harness**

Playwright is not a dependency of this repository and must not become one. Work in a scratch directory outside the repo; every later step reuses this variable:

```bash
export SCRATCH="${TMPDIR:-/tmp}/oikonomia-aurora"
mkdir -p "$SCRATCH/aurora-qa" && cd "$SCRATCH/aurora-qa"
npm init -y >/dev/null
npm install playwright@1.60.0
npx playwright install chromium
```

Create `$SCRATCH/aurora-qa/qa.mjs`:

```js
// Renders the real Oikonomia web UI in Chromium against a stubbed Tauri IPC
// layer, then (1) screenshots each screen in English and Greek and (2)
// measures worst-case text contrast over the drifting aurora.
import { chromium } from 'playwright'

const OUT = process.argv[2] ?? '.'
const URL = 'http://localhost:5173/'

function stubs(locale) {
  return `(() => {
    const entity = { id: 'e1', name: 'Ourovoros IKE', base_currency: 'EUR', fiscal_year_start_month: 1, chart_template: 'company' }
    const money = (n) => ({ amount_minor: n })
    const entry = (d, description, n) => ({
      entry: { id: 'j' + d, entity_id: 'e1', entry_date: '2026-08-' + String(d).padStart(2, '0'), description, reference: null, status: 'posted', hidden: false },
      lines: [
        { id: 'a' + d, entry_id: 'j' + d, account_id: 'a1', debit: money(n), credit: money(0), memo: null },
        { id: 'b' + d, entry_id: 'j' + d, account_id: 'a2', debit: money(0), credit: money(n), memo: null },
      ],
      is_voided: false,
    })
    const table = {
      vault_status: 'unlocked',
      app_info: { version: '0.1.0', name: 'Oikonomia', support_email: 'info@ourovoros.io' },
      license_status: { state: 'trial', days_remaining: 21 },
      entity_list: [entity, { ...entity, id: 'e2', name: 'Household', chart_template: 'personal' }],
      account_list: [
        { id: 'a1', entity_id: 'e1', code: '1000', name: 'Bank', account_type: 'asset', parent_id: null, is_active: true, is_system: false, sort_order: 1 },
        { id: 'a2', entity_id: 'e1', code: '4000', name: 'Sales', account_type: 'income', parent_id: null, is_active: true, is_system: false, sort_order: 2 },
      ],
      entry_list: [entry(28, 'Retainer - Thessaloniki', 500000), entry(23, 'Client invoice - Patras', 1500000)],
      dashboard_summary_cmd: { entity_id: 'e1', base_currency: 'EUR', cash_like_assets: 12840022, income: 7450000, expenses: 2310450, net_income: 5139550, recent_entry_count: 16 },
      // A null lock timeout makes the idle watchdog lock instantly.
      settings_get_lock_timeout: 900,
      vault_lock: 'locked',
      vault_touch: null,
      settings_get_locale: '${locale}',
    }
    // Without this, App's vault-locked listener throws on cleanup.
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} }
    window.__TAURI_INTERNALS__ = {
      invoke: (cmd) => {
        if (cmd.startsWith('plugin:event|')) return Promise.resolve(1)
        if (cmd.startsWith('plugin:')) return Promise.resolve(null)
        if (cmd in table) return Promise.resolve(table[cmd])
        if (cmd.endsWith('_list')) return Promise.resolve([])
        return Promise.resolve(null)
      },
      transformCallback: (cb) => cb,
      convertFileSrc: (path) => path,
    }
  })()`
}

const SCREENS = ['Dashboard', 'Transactions', 'Accounts', 'Reports', 'Settings']
const SCREENS_EL = ['Επισκόπηση', 'Κινήσεις', 'Λογαριασμοί', 'Αναφορές', 'Ρυθμίσεις']

async function openApp(browser, locale) {
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 2 })
  const errors = []
  page.on('pageerror', (e) => errors.push(String(e)))
  await page.addInitScript(stubs(locale))
  await page.goto(URL, { waitUntil: 'networkidle' })
  await page.waitForTimeout(1200)
  return { page, errors }
}

async function screenshots(browser) {
  for (const [locale, names] of [['en', SCREENS], ['el', SCREENS_EL]]) {
    const { page, errors } = await openApp(browser, locale)
    for (const [index, name] of names.entries()) {
      const button = page.getByRole('button', { name, exact: true })
      if (await button.count()) {
        await button.click()
        await page.waitForTimeout(700)
      }
      await page.screenshot({ path: `${OUT}/${locale}-${index + 1}-${SCREENS[index].toLowerCase()}.png` })
    }
    console.log(`${locale}: page errors ${JSON.stringify(errors)}`)
    await page.close()
  }
}

// Hide every glyph, icon and filled control, freeze the aurora at a drift
// phase, and read the brightest (99th percentile) backdrop pixel in a region.
async function contrast(browser) {
  const { page } = await openApp(browser, 'en')
  const tiers = { fg: '#e7ebf1', fgSecondary: '#aeb5bf', muted: '#a3aab5' }
  const worst = { pane: { lum: 0, rgb: [0, 0, 0] }, aurora: { lum: 0, rgb: [0, 0, 0] } }

  for (const delay of [0, 6, 12, 18, 24]) {
    await page.addStyleTag({
      content: `
        body * { color: transparent !important; -webkit-text-fill-color: transparent !important; text-shadow: none !important; }
        svg, button, .ui-control, .field-box { visibility: hidden !important; }
        .aurora-blobs i { animation-play-state: paused !important; animation-delay: -${delay}s !important; }`,
    })
    await page.waitForTimeout(200)

    const shot = await page.screenshot()
    const result = await page.evaluate(async (data) => {
      const img = new Image()
      img.src = 'data:image/png;base64,' + data
      await img.decode()
      const canvas = document.createElement('canvas')
      canvas.width = img.width
      canvas.height = img.height
      const ctx = canvas.getContext('2d')
      ctx.drawImage(img, 0, 0)
      const scale = img.width / window.innerWidth

      const lin = (v) => { v /= 255; return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4 }
      const lum = (r, g, b) => 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
      const none = { lum: 0, rgb: [0, 0, 0] }
      const p99 = (x, y, w, h) => {
        if (w < 8 || h < 8) return none
        const d = ctx.getImageData(Math.round(x * scale), Math.round(y * scale), Math.round(w * scale), Math.round(h * scale)).data
        const values = []
        for (let i = 0; i < d.length; i += 16) values.push({ lum: lum(d[i], d[i + 1], d[i + 2]), rgb: [d[i], d[i + 1], d[i + 2]] })
        values.sort((a, b) => a.lum - b.lum)
        return values[Math.floor(values.length * 0.99)]
      }
      const brighter = (a, b) => (b.lum > a.lum ? b : a)

      let pane = none
      for (const el of document.querySelectorAll('.glass-pane')) {
        const r = el.getBoundingClientRect()
        pane = brighter(pane, p99(r.x + 14, r.y + 14, r.width - 28, r.height - 28))
      }

      // Text sitting directly on the aurora: the main column's header band.
      const main = document.querySelector('main')?.getBoundingClientRect()
      const aurora = main ? p99(main.x, 0, main.width, 220) : none
      return { pane, aurora }
    }, shot.toString('base64'))

    worst.pane = result.pane.lum > (worst.pane.lum ?? 0) ? result.pane : worst.pane
    worst.aurora = result.aurora.lum > (worst.aurora.lum ?? 0) ? result.aurora : worst.aurora
  }

  const hex = (h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16))
  const lin = (v) => { v /= 255; return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4 }
  const lum = ([r, g, b]) => 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
  const ratio = (text, bgLum) => (Math.max(lum(text), bgLum) + 0.05) / (Math.min(lum(text), bgLum) + 0.05)

  for (const where of ['pane', 'aurora']) {
    const cells = Object.entries(tiers).map(([name, c]) => `${name} ${ratio(hex(c), worst[where].lum).toFixed(2)}`)
    console.log(`${where}: ${cells.join('  ')}  (brightest backdrop rgb ${worst[where].rgb.join(', ')})`)
  }
  await page.close()
}

const browser = await chromium.launch()
await screenshots(browser)
await contrast(browser)
await browser.close()
```

(`SCREENS_EL` matches `app.nav.*` in `web/src/locales/el.json` as of 2026-09-14. A missing button only skips that click; it does not fail the run.)

- [ ] **Step 3: Capture and measure**

```bash
cd web && npm run dev &
node "$SCRATCH/aurora-qa/qa.mjs" "$SCRATCH/aurora-qa"
```

Expected:
- `en: page errors []` and `el: page errors []`.
- Ten screenshots in `$SCRATCH/aurora-qa`. Open each: glass panes over colour, Barlow in English, **Greek letters rendered in Sofia Sans rather than a system font** in the Greek set, sidebar book list, money amounts in teal and coral.
- Two contrast lines. **Every value on both lines must be at least 4.5.**

If the `pane` line fails, raise the alpha in `--glass-fill` by 0.04, re-run, and repeat until it passes; then update the `--glass-fill` expectation in `web/tests/tokens.test.ts` and set `BRIGHTEST_GLASS` to the `brightest backdrop rgb` the `pane` line prints. If the `aurora` line fails, raise `--aurora-veil-top` by 0.05 (and `--aurora-veil-mid` by 0.03) and re-run. Commit any adjustment as its own commit whose message states the before and after measurements.

Stop the dev server when done.

- [ ] **Step 4: Measure the real bundle**

```bash
make smoke
open target/release/bundle/macos/Oikonomia.app
```

Unlock or create a test vault, leave the Dashboard focused and idle for a minute, and sample CPU:

```bash
ps -axo pid,%cpu,command | grep -i "Oikonomia.app\|com.apple.WebKit" | grep -v grep
```

Take five samples ten seconds apart, then send the window to the background and take five more. Record both averages in the PR. If the focused-idle average exceeds 8% of one core, stop and report before opening the PR: that is the signal to drop the drift to a still aurora, which the spec allows.

- [ ] **Step 5: Adversarial review**

Run the `code-review` skill at `high` effort against this branch versus `main`. Fix every confirmed finding in its own commit; for each finding you decline, note why in the PR description.

- [ ] **Step 6: Open the pull request**

```bash
git push -u origin feat/aurora-glass-foundation
gh pr create --title "feat(ui): Aurora glass foundation" --body-file "$SCRATCH/aurora-qa/pr-body.md"
```

Write `pr-body.md` first, with no emoji and no attribution lines. Include: what changed (typefaces with Greek covered, tokens, aurora, glass primitives, dark-only in Rust and web, Ledger amounts); what deliberately did not (composition, the light, arcs, Unlock, tray); the two measured contrast lines from Step 3 and any adjustment made; the CPU averages from Step 4; the review outcome from Step 5; and a link to the spec.

- [ ] **Step 7: Hand the running app to the user**

Start `make app`, tell the user it is running, attach the English and Greek screenshots, and ask for their review before phase 2 (data) and phase 3 (composition) are planned. Do not merge before the user has seen it.
