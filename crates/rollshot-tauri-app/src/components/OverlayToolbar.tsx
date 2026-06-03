import { Save, Square, X } from 'lucide-react'
import { Button } from '@/components/ui/button'

type OverlayToolbarProps = {
  mode: 'stitching' | 'done' | 'failed'
  message: string
  onStop: () => void
  onSave: () => void
  onClose: () => void
}

export function OverlayToolbar({ mode, message, onStop, onSave, onClose }: OverlayToolbarProps) {
  return (
    <div className={`overlay-toolbar overlay-toolbar-${mode}`}>
      <span className="overlay-toolbar-message">{message}</span>
      {mode === 'stitching' ? (
        <Button type="button" size="sm" variant="outline" onClick={onStop}>
          <Square className="size-4" aria-hidden="true" />
          Stop
        </Button>
      ) : null}
      {mode === 'done' ? (
        <Button type="button" size="sm" onClick={onSave}>
          <Save className="size-4" aria-hidden="true" />
          Save
        </Button>
      ) : null}
      <Button type="button" size="sm" variant="ghost" onClick={onClose}>
        <X className="size-4" aria-hidden="true" />
        Close
      </Button>
    </div>
  )
}
