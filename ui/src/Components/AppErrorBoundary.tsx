import { Component, type ErrorInfo, type ReactNode } from "react";

interface State {
  error: Error | null;
}

export default class AppErrorBoundary extends Component<
  { children: ReactNode },
  State
> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("Dim route failed", {
      error,
      componentStack: info.componentStack,
    });
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <main className="appLoad error" role="alert">
        <h1>Something went wrong</h1>
        <p>
          This page could not be displayed. Your library data has not been
          changed.
        </p>
        <button onClick={() => window.location.reload()}>Reload Dim</button>
      </main>
    );
  }
}
