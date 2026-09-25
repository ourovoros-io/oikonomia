// Captures the real app UI for the marketing site. See marketing/README.md.
import { mkdir, stat } from 'node:fs/promises'
import path from 'node:path'
import { parseArgs } from 'node:util'
import { chromium } from 'playwright'
import { createServer } from 'vite'

const PAGES = [
  { name: 'dashboard', nav: 0 },
  { name: 'transactions', nav: 1 },
  { name: 'documents', nav: 2 },
  { name: 'reports', nav: 4 },
]
const LANGS = ['en', 'el']
const FROZEN_NOW = new Date('2026-09-24T12:00:00')

const { values } = parseArgs({ options: { out: { type: 'string' } } })
if (!values.out) {
  console.error('usage: npm run capture:marketing -- --out <dir>')
  process.exit(2)
}
const outDir = path.resolve(values.out)

/**
 * Waits for the page's DOM to stop mutating instead of guessing a fixed
 * delay: React's post-click render plus its data-fetch waterfall keep
 * mutating the tree (attributes, text, children) until every read lands, so
 * "no mutation for `quietMs`" is a real readiness signal. None of the four
 * captured screens ever render `.animate-spin`, so that was never a signal
 * at all -- a slower render would still screenshot on schedule, half-drawn,
 * with exit 0. Resolves `true` once settled, `false` if `timeoutMs` passes
 * without a quiet window, so a stuck render fails the capture loudly instead
 * of shipping a broken shot.
 */
async function waitForDomSettled(page, { quietMs = 500, timeoutMs = 10_000 } = {}) {
  return page.evaluate(
    ({ quietMs, timeoutMs }) =>
      new Promise((resolve) => {
        let quietTimer
        const finish = (settled) => {
          observer.disconnect()
          clearTimeout(quietTimer)
          clearTimeout(hardTimer)
          resolve(settled)
        }
        const observer = new MutationObserver(() => {
          clearTimeout(quietTimer)
          quietTimer = setTimeout(() => finish(true), quietMs)
        })
        observer.observe(document.body, {
          subtree: true,
          childList: true,
          characterData: true,
          attributes: true,
        })
        quietTimer = setTimeout(() => finish(true), quietMs)
        const hardTimer = setTimeout(() => finish(false), timeoutMs)
      }),
    { quietMs, timeoutMs },
  )
}

const server = await createServer({
  root: path.resolve(import.meta.dirname, '..'),
  server: { port: 5174, strictPort: false },
  logLevel: 'error',
})
await server.listen()
const base = server.resolvedUrls.local[0]

const browser = await chromium.launch()
const failures = []

try {
  for (const lang of LANGS) {
    const context = await browser.newContext({
      viewport: { width: 1440, height: 900 },
      deviceScaleFactor: 2,
      reducedMotion: 'reduce',
      colorScheme: 'dark',
    })
    const page = await context.newPage()

    page.on('console', (msg) => {
      if (msg.type() === 'error') failures.push(`${lang}: console: ${msg.text()}`)
    })
    page.on('pageerror', (err) => failures.push(`${lang}: pageerror: ${err.message}`))

    await page.clock.setFixedTime(FROZEN_NOW)
    await page.goto(`${base}marketing.html?lang=${lang}`)
    await page.locator('nav button[aria-current="page"]').waitFor()

    for (const { name, nav } of PAGES) {
      await page.locator('nav button').nth(nav).click()
      const settled = await waitForDomSettled(page)
      if (!settled) {
        failures.push(`${lang}: ${name}: DOM never settled within 10s -- a render may still be in flight`)
      }
      await page.evaluate(() => document.fonts.ready)
      // No fixed post-settle wait: the context's reducedMotion: 'reduce'
      // trips the app's own `prefers-reduced-motion` CSS, which turns off
      // every one-shot/looping animation (pulse bars, aurora drift, donut
      // sweep, logo draw-in) outright, so there is nothing left to finish
      // drawing once the DOM and fonts are settled.

      const file = path.join(outDir, lang, `${name}.png`)
      await mkdir(path.dirname(file), { recursive: true })
      await page.screenshot({ path: file, omitBackground: true })
      await stat(file)
      console.log(`captured ${path.relative(process.cwd(), file)}`)
    }

    await context.close()
  }
} catch (err) {
  failures.push(String(err))
} finally {
  await browser.close()
  await server.close()
}

if (failures.length > 0) {
  console.error(`capture failed:\n  ${failures.join('\n  ')}`)
  process.exit(1)
}
