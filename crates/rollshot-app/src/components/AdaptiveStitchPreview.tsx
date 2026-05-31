import type { PreviewPlacement } from '../overlay/placement'

type AdaptiveStitchPreviewProps = {
  imageUrl: string | null
  status: string
  placement: PreviewPlacement
  processing?: boolean
}

export function AdaptiveStitchPreview({
  imageUrl,
  status,
  placement,
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
      {processing ? <div className="preview-processing-indicator" aria-label="Stitching" /> : null}
    </div>
  )
}
