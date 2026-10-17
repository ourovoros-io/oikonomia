// Shared by the marketing capture and the layout check.

/**
 * Waits for the page's DOM to stop mutating instead of guessing a fixed
 * delay: React's post-click render plus its data-fetch waterfall keep
 * mutating the tree (attributes, text, children) until every read lands, so
 * "no mutation for `quietMs`" is a real readiness signal. Resolves `true`
 * once settled, `false` if `timeoutMs` passes without a quiet window, so a
 * stuck render fails the run loudly instead of passing on a half-drawn
 * screen.
 */
export async function waitForDomSettled(page, { quietMs = 500, timeoutMs = 10_000 } = {}) {
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
