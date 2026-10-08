import { api } from './api'

/**
 * Sends errors nothing caught, and promise rejections nothing handled, to the
 * desktop log. Page render errors are reported by `PageErrorBoundary`
 * instead, which knows the page. Returns the function that removes the
 * listeners.
 */
export function reportUncaughtErrors(target: Window = window): () => void {
  const onError = (event: ErrorEvent) => api.logFrontendError('window', event.error)
  const onRejection = (event: PromiseRejectionEvent) => api.logFrontendError('window', event.reason)

  target.addEventListener('error', onError)
  target.addEventListener('unhandledrejection', onRejection)
  return () => {
    target.removeEventListener('error', onError)
    target.removeEventListener('unhandledrejection', onRejection)
  }
}
