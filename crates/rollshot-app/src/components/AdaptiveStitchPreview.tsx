import type { CapturedEdge } from '../api/capture'
import type { PreviewPlacement } from '../overlay/placement'

type AdaptiveStitchPreviewProps = {
  imageUrl: string | null
  status: string
  placement: PreviewPlacement
  captureMiss?: boolean
  capturedEdge?: CapturedEdge
  processing?: boolean
}

export function AdaptiveStitchPreview({
  imageUrl,
  status,
  placement,
  captureMiss,
  capturedEdge,
  processing,
}: AdaptiveStitchPreviewProps) {
  if (placement.mode === 'status' || !imageUrl) {
    return <div className="capture-status">{status}</div>
  }

  return (
    <div
      className={`adaptive-stitch-preview adaptive-stitch-preview-${placement.side}`}
      style={{
        left: `${placement.rect.left}px`,
        top: `${placement.rect.top}px`,
        width: `${placement.rect.width}px`,
        height: `${placement.rect.height}px`,
      }}
    >
      <img src={imageUrl} alt="Stitching preview" draggable={false} />
      {captureMiss ? (
        <div className={`preview-recovery-mask preview-recovery-mask-${capturedEdge ?? 'unknown'}`}>
          <span>Scroll back to the captured edge</span>
        </div>
      ) : null}
      {processing ? <div className="preview-processing-indicator" aria-label="Stitching" /> : null}
    </div>
  )
}
