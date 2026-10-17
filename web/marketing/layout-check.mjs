// Checks that translated labels fit, in Chromium against the marketing
// fixture. jsdom does no layout, so the unit tests cannot. See
// marketing/README.md.
import path from 'node:path'
import { chromium } from 'playwright'
import { createServer } from 'vite'
import { waitForDomSettled } from './settle.mjs'

const LANGS = ['en', 'el', 'fr', 'de']
// 960 is the main window's minimum width (tauri.conf.json).
const WIDTHS = [960, 1024, 1280, 1440, 1600]
// Sidebar order, as in capture.mjs. The dashboard and accounts screens get
// their own checks on top of the Button check every screen gets.
const SCREENS = [
  { name: 'dashboard', nav: 0 },
  { name: 'transactions', nav: 1 },
  { name: 'documents', nav: 2 },
  { name: 'accounts', nav: 3 },
  { name: 'reports', nav: 4 },
  { name: 'settings', nav: 5 },
]
const FROZEN_NOW = new Date('2026-09-24T12:00:00')
// Half a pixel of slack: Chromium rounds fractional boxes either way.
const SLACK = 0.5

/**
 * Every rendered base Button (the only element with `data-variant`) keeps
 * its label on one line and inside its box. `checkVisibility()` without
 * options keeps `visibility: hidden` buttons on purpose: an invisible
 * placeholder still takes its space in the layout. A label that truncates
 * inside its own element clips its text, so it passes as long as the button
 * itself does not overflow.
 */
function buttonProblems(slack) {
  const lineCount = (element) => {
    const range = document.createRange()
    const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT)
    const tops = new Set()
    while (walker.nextNode()) {
      range.selectNodeContents(walker.currentNode)
      for (const rect of range.getClientRects()) {
        if (rect.width > 0) tops.add(Math.round(rect.top))
      }
    }
    return tops.size
  }

  return [...document.querySelectorAll('button[data-variant]')]
    .filter((button) => button.checkVisibility() && button.textContent.trim() !== '')
    .flatMap((button) => {
      const label = button.textContent.trim()
      const problems = []
      if (lineCount(button) > 1) problems.push(`Button "${label}" wraps its label`)
      if (button.scrollWidth > button.clientWidth + slack) {
        problems.push(`Button "${label}" is narrower than its label`)
      }
      return problems
    })
}

/**
 * No arc tile label or hint is cut off or runs past its tile. The figure above
 * them is skipped: it is set leading-none, so its glyphs overhang its box by
 * design, and it does not change with the language.
 */
function arcProblems(slack) {
  const tiles = [...document.querySelectorAll('[role="meter"]')]
  if (tiles.length === 0) return ['no arc tiles found on the dashboard']

  return tiles.flatMap((tile) => {
    const box = tile.getBoundingClientRect()
    const texts = [...tile.querySelectorAll(':scope > div > div + div')]
    if (texts.length === 0) return [`arc tile "${tile.getAttribute('aria-label')}" has no label to check`]

    return texts.flatMap((text) => {
      const rect = text.getBoundingClientRect()
      const name = `arc label or hint "${text.textContent.trim()}"`
      const problems = []
      if (text.scrollHeight > text.clientHeight + slack || text.scrollWidth > text.clientWidth + slack) {
        problems.push(`${name} is cut off`)
      }
      if (rect.bottom > box.bottom + slack || rect.right > box.right + slack) {
        problems.push(`${name} runs past its tile`)
      }
      return problems
    })
  })
}

/**
 * Each account row's actions sit in columns: the n-th action button has the
 * same x position in every row. The Set balance label stays on one line.
 */
function accountRowProblems(slack) {
  const rows = [...document.querySelectorAll('main li')].filter((row) =>
    row.querySelector('button[data-variant]'),
  )
  if (rows.length < 2) return ['fewer than two account rows found']

  const problems = []
  const columns = rows.map((row) =>
    [...row.querySelectorAll('button[data-variant]')].map((button) => button.getBoundingClientRect().left),
  )
  const width = Math.min(...columns.map((lefts) => lefts.length))
  for (let column = 0; column < width; column++) {
    const lefts = columns.map((row) => row[column])
    const spread = Math.max(...lefts) - Math.min(...lefts)
    if (spread > slack) problems.push(`row action ${column + 1} is out of line by ${spread.toFixed(1)} px`)
  }

  const setBalance = rows
    .flatMap((row) => [...row.querySelectorAll('button[data-variant]')])
    .filter((button) => button.textContent.trim() !== '')
  if (setBalance.length === 0) problems.push('no Set balance button found')
  for (const button of setBalance) {
    const tops = new Set(
      [...button.childNodes]
        .filter((node) => node.nodeType === Node.TEXT_NODE)
        .flatMap((node) => {
          const range = document.createRange()
          range.selectNodeContents(node)
          return [...range.getClientRects()].map((rect) => Math.round(rect.top))
        }),
    )
    if (tops.size !== 1) problems.push(`Set balance "${button.textContent.trim()}" takes ${tops.size} lines`)
  }
  return problems
}

const EXTRA_CHECKS = { dashboard: arcProblems, accounts: accountRowProblems }

/** Runs every check for one screen at every width; returns the failures. */
async function checkScreen(page, screen) {
  const failures = []
  for (const width of WIDTHS) {
    await page.setViewportSize({ width, height: 900 })
    // A layout effect may re-render after the resize; measure only once it is done.
    if (!(await waitForDomSettled(page, { quietMs: 150 }))) {
      failures.push(`${width}px ${screen.name}: DOM never settled after the resize`)
    }

    const problems = await page.evaluate(buttonProblems, SLACK)
    const extra = EXTRA_CHECKS[screen.name]
    if (extra) problems.push(...(await page.evaluate(extra, SLACK)))
    failures.push(...problems.map((problem) => `${width}px ${screen.name}: ${problem}`))
  }
  return failures
}

async function checkLanguage(browser, base, lang) {
  const failures = []
  const context = await browser.newContext({ reducedMotion: 'reduce', colorScheme: 'dark' })
  const page = await context.newPage()
  page.on('console', (msg) => {
    if (msg.type() === 'error') failures.push(`console: ${msg.text()}`)
  })
  page.on('pageerror', (err) => failures.push(`pageerror: ${err.message}`))

  await page.clock.setFixedTime(FROZEN_NOW)
  await page.goto(`${base}marketing.html?lang=${lang}`)
  await page.locator('nav button[aria-current="page"]').waitFor()
  const htmlLang = await page.evaluate(() => document.documentElement.lang)
  if (htmlLang !== lang) failures.push(`asked for ${lang}, the app shows ${htmlLang}`)

  for (const screen of SCREENS) {
    await page.locator('nav button').nth(screen.nav).click()
    if (!(await waitForDomSettled(page))) failures.push(`${screen.name}: DOM never settled`)
    await page.evaluate(() => document.fonts.ready)
    failures.push(...(await checkScreen(page, screen)))
  }

  await context.close()
  return failures.map((failure) => `${lang} ${failure}`)
}

const server = await createServer({
  root: path.resolve(import.meta.dirname, '..'),
  server: { port: 5175, strictPort: false },
  logLevel: 'error',
})
await server.listen()
const base = server.resolvedUrls.local[0]
const browser = await chromium.launch()

const failures = []
try {
  for (const lang of LANGS) failures.push(...(await checkLanguage(browser, base, lang)))
} catch (err) {
  failures.push(String(err))
} finally {
  await browser.close()
  await server.close()
}

if (failures.length > 0) {
  console.error(`layout check failed:\n  ${failures.join('\n  ')}`)
  process.exit(1)
}
console.log(`layout check passed: ${LANGS.join(', ')} at ${WIDTHS.join(', ')} px`)
