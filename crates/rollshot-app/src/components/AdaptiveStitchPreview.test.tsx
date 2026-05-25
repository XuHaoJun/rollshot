import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import { AdaptiveStitchPreview } from './AdaptiveStitchPreview'

describe('AdaptiveStitchPreview', () => {
  it('renders an image when placement allows image preview', () => {
    const html = renderToStaticMarkup(
      <AdaptiveStitchPreview
        imageUrl="blob:stitch"
        status="3 frames"
        placement={{
          mode: 'image',
          side: 'right',
          rect: { left: 120, top: 20, width: 180, height: 260 },
        }}
      />,
    )

    expect(html).toContain('blob:stitch')
    expect(html).toContain('adaptive-stitch-preview')
  })

  it('renders status-only when placement is status', () => {
    const html = renderToStaticMarkup(
      <AdaptiveStitchPreview imageUrl="blob:stitch" status="Stitching live" placement={{ mode: 'status' }} />,
    )

    expect(html).toContain('Stitching live')
    expect(html).not.toContain('blob:stitch')
  })
})
