import type { PreviewPlacement } from '../overlay/placement'

type AdaptiveStitchPreviewProps = {
  imageUrl: string | null
  status: string
  placement: PreviewPlacement
}

export function AdaptiveStitchPreview({ imageUrl, status, placement }: AdaptiveStitchPreviewProps) {
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
    </div>
  )
}
