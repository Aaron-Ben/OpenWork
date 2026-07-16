import { Component, type ErrorInfo, type ReactNode } from 'react'

import i18n from '../i18n'

interface Props {
  children: ReactNode
}

interface State {
  error: Error | null
}

/// 全局错误边界:捕获子树渲染期错误,避免整个应用白屏。
/// 显示错误堆栈,方便在打包版(无 devtools)也能看到运行时错误。
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null }

  static getDerivedStateFromError(error: Error): State {
    return { error }
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error('UI error:', error, info)
  }

  render(): ReactNode {
    if (this.state.error) {
      return (
        <div className="grid min-h-full place-items-center bg-paper p-8">
          <div className="max-w-2xl rounded-xl border border-status-danger-border bg-paper p-6 shadow-sm">
            <h2 className="mb-2 text-lg font-semibold text-status-danger-ink">{i18n.t('errorBoundary.title')}</h2>
            <p className="mb-4 text-sm text-ink-soft">
              {i18n.t('errorBoundary.description')}
            </p>
            <pre className="max-h-80 overflow-auto rounded-lg bg-ink p-3 text-[11px] leading-relaxed text-paper">
              {this.state.error.message}
              {'\n\n'}
              {this.state.error.stack}
            </pre>
            <button
              type="button"
              className="mt-4 rounded-lg bg-ink px-4 py-2 text-sm font-medium text-paper hover:bg-ink-soft"
              onClick={() => this.setState({ error: null })}
            >
              {i18n.t('errorBoundary.retry')}
            </button>
          </div>
        </div>
      )
    }
    return this.props.children
  }
}
