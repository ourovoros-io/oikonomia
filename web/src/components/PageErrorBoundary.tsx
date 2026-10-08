import { Component, type ErrorInfo, type ReactNode } from 'react'
import { t } from '../lib/i18n'
import { Button, ErrorBanner } from './ui'

type Props = {
  /** Changes whenever the page or book changes, which clears a caught error. */
  resetKey: string
  children: ReactNode
}

type State = { failed: boolean; resetKey: string }

/**
 * Keeps a render error on one page from blanking the whole window. The shell
 * and sidebar stay usable, and the user can retry or switch page.
 */
export class PageErrorBoundary extends Component<Props, State> {
  state: State = { failed: false, resetKey: this.props.resetKey }

  static getDerivedStateFromError(): Partial<State> {
    return { failed: true }
  }

  static getDerivedStateFromProps(props: Props, state: State): State | null {
    return props.resetKey === state.resetKey ? null : { failed: false, resetKey: props.resetKey }
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error('Page failed to render', error, info.componentStack)
  }

  render() {
    if (!this.state.failed) return this.props.children

    return (
      <div className="space-y-4">
        <ErrorBanner message={t('error.pageRender')} />
        <Button onClick={() => this.setState({ failed: false })}>{t('error.pageRenderRetry')}</Button>
      </div>
    )
  }
}
