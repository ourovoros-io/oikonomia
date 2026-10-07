import { readdirSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

/**
 * Shared by the source-scanning guard tests (`i18n.catalog.test.ts`,
 * `noRawErrorMessage.test.ts` and `moneyDecimals.test.ts`), so the three
 * agree on what production code is.
 *
 * It lives in `src/test`, which the app build does not compile, because it
 * needs Node's file system.
 */

/** The `src` directory, with a trailing slash. */
export const SRC_ROOT = fileURLToPath(new URL('../', import.meta.url))

/** Directories that hold no production code: the catalogs and the test helpers. */
const SKIPPED_DIRECTORIES: ReadonlySet<string> = new Set(['locales', 'test'])

/** Production sources only: no tests or test helpers, no type stubs, no catalogs. */
export function productionSources(dir: string = SRC_ROOT): string[] {
  const files: string[] = []

  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = `${dir}${entry.name}`

    if (entry.isDirectory()) {
      if (SKIPPED_DIRECTORIES.has(entry.name)) continue
      files.push(...productionSources(`${path}/`))
      continue
    }

    if (!/\.tsx?$/.test(entry.name)) continue
    if (/\.(test|testutil)\.tsx?$/.test(entry.name) || entry.name.endsWith('.d.ts')) continue

    files.push(path)
  }

  return files
}
