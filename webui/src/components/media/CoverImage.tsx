import { useState, type ComponentProps } from 'react'
import { BookOpen, MagnifyingGlassPlus } from '@phosphor-icons/react'
import { cn } from '@/lib/utils/cn'
import { useImageCrossfade } from '@/lib/hooks/useImageCrossfade'
import { CoverLightbox } from './CoverLightbox'

interface CoverImageProps {
  src: string
  alt: string
  className?: string
  /** spoiler blur (unread blur preference) */
  blurred?: boolean
  eager?: boolean
  /** pass-through to the underlying <img>; 'no-referrer' makes provider CDNs serve real covers */
  referrerPolicy?: ComponentProps<'img'>['referrerPolicy']
  /** allow clicking on cover to open full-screen lightbox */
  previewable?: boolean
  /** custom click callback */
  onClick?: () => void
}

/** √2 cover with shimmer placeholder, fade-in on load and icon fallback. */
export function CoverImage({ src, alt, className, blurred, eager, referrerPolicy, previewable, onClick }: CoverImageProps) {
  const xf = useImageCrossfade(src)
  const [viewerOpen, setViewerOpen] = useState(false)

  const handleClick = () => {
    if (onClick) {
      onClick()
    } else if (previewable) {
      setViewerOpen(true)
    }
  }

  const isClickable = !!onClick || previewable

  return (
    <>
      <div
        onClick={handleClick}
        className={cn(
          'cover-aspect relative w-full overflow-hidden rounded-lg bg-raised shadow-card-rest transition-[box-shadow,filter,transform] duration-300 ease-out group-hover:shadow-card-hover group-hover:brightness-110',
          isClickable && 'cursor-zoom-in',
          className,
        )}
      >
        {!xf.loaded && !xf.error && !xf.base && <div className="shimmer absolute inset-0" />}
        {xf.base && (
          <img
            src={xf.base}
            alt=""
            aria-hidden
            draggable={false}
            referrerPolicy={referrerPolicy}
            className={cn('absolute inset-0 size-full object-cover', blurred && 'blur-[5px] scale-[1.04]')}
          />
        )}
        {xf.error ? (
          <div className="absolute inset-0 flex items-center justify-center text-ink-3">
            <BookOpen className="size-8" weight="duotone" />
          </div>
        ) : (
          <img
            src={src}
            alt={alt}
            loading={eager ? 'eager' : 'lazy'}
            draggable={false}
            referrerPolicy={referrerPolicy}
            onLoad={xf.onLoad}
            onError={xf.onError}
            onTransitionEnd={xf.onTransitionEnd}
            className={cn(
              'absolute inset-0 size-full object-cover transition-opacity duration-200',
              xf.loaded ? 'opacity-100' : 'opacity-0',
              blurred && 'blur-[5px] scale-[1.04]',
            )}
          />
        )}
        {/* hairline over the cover so dark covers stay distinguishable from the page background */}
        <div aria-hidden className="pointer-events-none absolute inset-0 rounded-[inherit] inset-ring-1 inset-ring-line" />

        {/* Subtle zoom indicator on hover when previewable */}
        {previewable && (
          <div className="pointer-events-none absolute inset-0 flex items-center justify-center bg-black/30 opacity-0 transition-opacity duration-200 group-hover:opacity-100">
            <div className="rounded-full bg-black/60 p-2 text-white backdrop-blur-sm shadow-md">
              <MagnifyingGlassPlus className="size-5" />
            </div>
          </div>
        )}
      </div>

      {previewable && (
        <CoverLightbox open={viewerOpen} onClose={() => setViewerOpen(false)} src={src} title={alt} />
      )}
    </>
  )
}

