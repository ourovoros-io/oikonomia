import { readdirSync, readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, test } from 'vitest'
import { stripComments } from './stripComments.testutil'

/**
 * Guard: screens must not put raw backend error text on screen.
 *
 * A command error's `message` is English text from Rust, with operating-system
 * detail in it. Screens show localized copy by code through
 * `commandErrorMessage`, and anything that needs the error's shape goes
 * through `asCommandError`.
 *
 * This is a tripwire, not a proof. It scans source text for the common ways of
 * reaching the message (a `.message` read, destructuring, bracket access,
 * stringifying an error variable) and cannot see an alias or a helper that
 * does the same. It catches the careless slip; code review catches the rest.
 *
 * Known forms it does not catch:
 * - `JSON.stringify(err)`
 * - string concatenation with `+`
 * - `err.stack`
 * - a computed key (`err[key]`)
 * - `Object.values(err)`
 * - an error held in a variable whose name is not one of `ERROR_NAMES` below
 *
 * `instanceof Error` alone is not flagged: a branch that never reads the
 * message (`e instanceof Error && e.name === 'AbortError'`) leaks nothing, and
 * one that does read it is caught by the `.message`, `String(...)`, template
 * and `toString()` patterns.
 *
 * When it fires on a legitimate field that merely happens to be named
 * `message`, the right fix is to rename that field, not to add an exemption.
 * The allow-list below is for the code that owns the normalisation, and must
 * stay tiny.
 */

const SRC_ROOT = fileURLToPath(new URL('../', import.meta.url))

/** The only production files allowed to touch an error's message. */
const RAW_MESSAGE_ALLOWED: Record<string, string> = {
  'lib/commandError.ts':
    'asCommandError normalises what invoke threw and the raw message is logged, never shown; it also matches the IPC "command not found" text to detect an unregistered command',
}

/** Variable names that, by convention, hold a caught error. */
const ERROR_NAMES = '(?:err|e|error|cause|reason)'

/** One way of reaching raw error text. Each runs over whole-file text, so a split line is still seen. */
const RAW_MESSAGE_PATTERNS: Array<{ name: string; pattern: RegExp }> = [
  // err.message, err?.message, and err\n  .message split over lines.
  { name: '.message read', pattern: /\.\s*message\b/g },
  // const { message } = err, const { message: text } = err.
  { name: 'message destructuring', pattern: /\{[^{}]*\bmessage\b[^{}]*\}\s*=(?!=)/g },
  // catch ({ message }) { ... }
  { name: 'message destructuring', pattern: /\bcatch\s*\(\s*\{[^{}]*\bmessage\b/g },
  // err['message']
  { name: 'bracket access', pattern: /\[\s*['"`]message['"`]\s*\]/g },
  // String(err)
  { name: 'String(error)', pattern: new RegExp(`\\bString\\(\\s*${ERROR_NAMES}\\s*\\)`, 'g') },
  // `${err}`
  { name: 'template of error', pattern: new RegExp(`\\$\\{\\s*${ERROR_NAMES}\\s*\\}`, 'g') },
  // err.toString()
  { name: 'error.toString()', pattern: new RegExp(`\\b${ERROR_NAMES}\\.toString\\(\\)`, 'g') },
]

/**
 * Every place `source` could put raw error text on screen, as "line: pattern".
 * Pure, so the tests below exercise the same code as the repository scan.
 */
function rawMessageReads(source: string): string[] {
  const code = stripComments(source)
  const hits: Array<{ index: number; text: string }> = []

  for (const { name, pattern } of RAW_MESSAGE_PATTERNS) {
    for (const match of code.matchAll(pattern)) {
      const line = code.slice(0, match.index).split('\n').length
      hits.push({ index: match.index, text: `${line}: ${name}` })
    }
  }

  return hits.sort((a, b) => a.index - b.index).map((hit) => hit.text)
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
    if (/\.(test|testutil)\.tsx?$/.test(entry.name) || entry.name.endsWith('.d.ts')) continue

    files.push(path)
  }

  return files
}

/** Just the names of the patterns that fired, in source order. */
function firedPatterns(source: string): string[] {
  return rawMessageReads(source).map((hit) => hit.slice(hit.indexOf(': ') + 2))
}

describe('rawMessageReads', () => {
  // [description, source, the one pattern that must fire]
  const flagged: Array<[string, string, string]> = [
    ['a .message read', "setError(err.message)", '.message read'],
    ['an optional .message read', "setError(err?.message)", '.message read'],
    ['a .message read split over lines', "const text = error\n    .message", '.message read'],
    // A UI prop or event field named `message` is flagged on purpose: the scan
    // cannot tell it from an error's message. The fix is to rename the field
    // (`text`, `body`, `notice`), not to add an exemption.
    ['a message prop read', "return <p>{props.message}</p>", '.message read'],
    ['a nested event field read', "show(event.data.message)", '.message read'],
    ['message destructuring', "const { message } = err", 'message destructuring'],
    ['renamed message destructuring', "const { message: text } = error", 'message destructuring'],
    [
      'multi-field destructuring',
      "const { code, message } = asCommandError(err)",
      'message destructuring',
    ],
    ['multi-line destructuring', "const {\n  code,\n  message,\n} = caught", 'message destructuring'],
    [
      'catch parameter destructuring',
      "try { run() } catch ({ message }) { show(message) }",
      'message destructuring',
    ],
    ['bracket access', "setError(err['message'])", 'bracket access'],
    ['double-quoted bracket access', 'setError(error["message"])', 'bracket access'],
    ['String(err)', "setError(String(err))", 'String(error)'],
    ['String(e)', "setError(String(e))", 'String(error)'],
    ['String(cause)', "setError(String( cause ))", 'String(error)'],
    ['template of error', "setError(`Failed: ${error}`)", 'template of error'],
    ['template of reason', "setError(`${reason}`)", 'template of error'],
    ['err.toString()', "setError(err.toString())", 'error.toString()'],
    ['cause.toString()', "setError(cause.toString())", 'error.toString()'],
    [
      'an instanceof branch that reads the message',
      "setError(err instanceof Error ? err.message : 'x')",
      '.message read',
    ],
    [
      'an instanceof branch that stringifies the error',
      "setError(err instanceof Error ? 'x' : String(err))",
      'String(error)',
    ],
    [
      'a read after a comment-looking string',
      'const url = "https://example.com"; show(err.message)',
      '.message read',
    ],
    [
      'a read after a double slash inside a string',
      'const a = "x // y"; show(err.message)',
      '.message read',
    ],
    [
      'a read after a double slash inside a single-quoted string',
      "const a = 'x // y'; show(err.message)",
      '.message read',
    ],
    [
      'a read after a double slash inside a template',
      'const a = `x // ${n}`; show(err.message)',
      '.message read',
    ],
  ]

  test.each(flagged)('flags %s', (_name, source, pattern) => {
    expect(firedPatterns(source)).toEqual([pattern])
  })

  const allowed: Array<[string, string]> = [
    ['a local named message', "const message = t('x.y')"],
    ['passing a message along', "setMessage(t('x.y'))"],
    ['an object literal with a message field', "const toast = { message: 'Saved' }"],
    ['a multi-line object literal', "const toast = {\n  message: 'Saved',\n  kind: 'ok',\n}"],
    ['destructuring other fields', "const { code, params } = asCommandError(err)"],
    ['the shared helper', "setError(commandErrorMessage(err, 'docs.deleteFailed'))"],
    ['String() of a non-error', "const label = String(count)"],
    ['a template of a non-error', "const label = `${name} (${currency})`"],
    ['toString() of a non-error', "const label = value.toString()"],
    ['instanceof of another class', "if (value instanceof Date) return value"],
    [
      'an instanceof Error check that never reads the message',
      "if (e instanceof Error && e.name === 'AbortError') return",
    ],
    ['a bare instanceof Error branch', "const ok = err instanceof Error ? 'x' : 'y'"],
    ['a type with a message field', "type Toast = { message: string }"],
    ['a read inside a line comment', "// err.message is raw backend text"],
    ['a read inside a trailing comment', "run() // never show err.message"],
    ['a read inside a block comment', "/**\n * Never read err.message here.\n */\nrun()"],
    ['a URL in a string', "const url = 'https://example.com/a'"],
  ]

  test.each(allowed)('does not flag %s', (_name, source) => {
    expect(rawMessageReads(source)).toEqual([])
  })

  test('reports the line of the hit', () => {
    expect(rawMessageReads("const a = 1\nconst b = 2\nshow(err.message)")).toEqual(['3: .message read'])
  })

  test('reports the line of a hit that follows a block comment', () => {
    expect(rawMessageReads("/* one\n two */\nshow(String(err))")).toEqual(['3: String(error)'])
  })
})

describe('no raw backend message in the UI', () => {
  test('production code does not reach an error message outside the allow-list', () => {
    const offenders: string[] = []

    for (const file of productionSources()) {
      const relative = file.slice(SRC_ROOT.length)
      if (relative in RAW_MESSAGE_ALLOWED) continue

      for (const read of rawMessageReads(readFileSync(file, 'utf8'))) offenders.push(`${relative}:${read}`)
    }

    expect(offenders).toEqual([])
  })

  test('every allow-list entry still reads a message', () => {
    const stale = Object.keys(RAW_MESSAGE_ALLOWED).filter(
      (relative) => rawMessageReads(readFileSync(`${SRC_ROOT}${relative}`, 'utf8')).length === 0,
    )

    expect(stale).toEqual([])
  })
})
