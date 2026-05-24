import { Check, Play, Square } from 'lucide-react'
import { Button } from '@/components/ui/button'

export default function App() {
  return (
    <main className="app-shell">
      <section className="capture-surface">
        <div className="empty-preview">No preview yet</div>
      </section>
      <aside className="control-panel" aria-label="Capture controls">
        <h1>rollshot</h1>
        <p className="status-text">Ready to start capture</p>
        <Button type="button">
          <Play className="size-4" aria-hidden="true" />
          Start
        </Button>
        <Button type="button" variant="secondary" disabled>
          <Check className="size-4" aria-hidden="true" />
          Confirm Region
        </Button>
        <Button type="button" variant="outline" disabled>
          <Square className="size-4" aria-hidden="true" />
          Stop
        </Button>
      </aside>
    </main>
  )
}
