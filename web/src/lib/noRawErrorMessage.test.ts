import { readdirSync, readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, test } from 'vitest'

const SRC_ROOT = fileURLToPath(new URL('../', import.meta.url))

/**
 * The only production files allowed to touch an error's `.message`. A command
 * error's message is English text from Rust, with operating-system detail in
 * it. Screens must show localized copy by code (`commandErrorMessage`), so
 * every other file is forbidden from reading it. Keep this list short.
 */
const RAW_MESSAGE_ALLOWED: Record<string, string> = {
  'lib/commandError.ts': 'asCommandError normalises what invoke threw; the raw message is logged, never shown',
  'lib/updateCheck.ts': 'matches the IPC "command not found" text to detect an unregistered command',
}

/** Production sources only: no tests, no type stubs, no catalogs. */
function productionSources(dir: string = SRC_ROOT): string[] {
  const files: string[] = []

  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = `${dir}${entry.name}`

    if (entry.isDirectory()) {
      if (entry.name === 'locales') continue
      files.push(...productionSources(`${path}/`))
      continue
    }

    if (!/\.tsx?$/.test(entry.name)) continue
    if (/\.test\.tsx?$/.test(entry.name) || entry.name.endsWith('.d.ts')) continue

    files.push(path)
  }

  return files
}

/** Lines of code that read `.message`, ignoring whole-line comments. */
function rawMessageReads(file: string): string[] {
  return readFileSync(file, 'utf8')
    .split('\n')
    .map((text, index) => ({ text: text.trim(), line: index + 1 }))
    .filter(({ text }) => !/^(\/\/|\/\*|\*)/.test(text))
    .filter(({ text }) => /\.message\b/.test(text))
    .map(({ text, line }) => `${line}: ${text}`)
}

describe('no raw backend message in the UI', () => {
  test('production code does not read an error message outside the allow-list', () => {
    const offenders: string[] = []

    for (const file of productionSources()) {
      const relative = file.slice(SRC_ROOT.length)
      if (relative in RAW_MESSAGE_ALLOWED) continue

      for (const read of rawMessageReads(file)) offenders.push(`${relative}:${read}`)
    }

    expect(offenders).toEqual([])
  })

  test('every allow-list entry still reads a message', () => {
    const stale = Object.keys(RAW_MESSAGE_ALLOWED).filter(
      (relative) => rawMessageReads(`${SRC_ROOT}${relative}`).length === 0,
    )

    expect(stale).toEqual([])
  })
})
