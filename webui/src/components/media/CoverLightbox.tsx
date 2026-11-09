import { useState, useEffect, useRef, useCallback } from 'react'
import * as RadixDialog from '@radix-ui/react-dialog'
import { motion, AnimatePresence } from 'motion/react'
import {
  X,
  MagnifyingGlassPlus,
  MagnifyingGlassMinus,
  ArrowClockwise,
  ArrowSquareOut,
  ArrowsInSimple,
  ArrowsOutSimple,
} from '@phosphor-icons/react'

export interface CoverLightboxProps {
  open: boolean
  onClose: () => void
  src: string
  title?: string
}

export function CoverLightbox({ open, onClose, src, title }: CoverLightboxProps) {
  const [zoom, setZoom] = useState(1)
  const [rotation, setRotation] = useState(0)
  const [offset, setOffset] = useState({ x: 0, y: 0 })
  const [isDragging, setIsDragging] = useState(false)
  const [dragStart, setDragStart] = useState({ x: 0, y: 0 })
  const [imgSize, setImgSize] = useState<{ width: number; height: number } | null>(null)

  const viewportRef = useRef<HTMLDivElement>(null)
  const imgRef = useRef<HTMLImageElement>(null)

  // Reset transforms whenever modal opens or image src changes
  useEffect(() => {
    if (open) {
      setZoom(1)
      setRotation(0)
      setOffset({ x: 0, y: 0 })
      setIsDragging(false)
    }
  }, [open, src])

  const handleZoomIn = useCallback(() => {
    setZoom((z) => Math.min(Number((z + 0.25).toFixed(2)), 5))
  }, [])

  const handleZoomOut = useCallback(() => {
    setZoom((z) => {
      const next = Math.max(Number((z - 0.25).toFixed(2)), 0.5)
      if (next <= 1) setOffset({ x: 0, y: 0 })
      return next
    })
  }, [])

  const handleRotate = useCallback(() => {
    setRotation((r) => (r + 90) % 360)
  }, [])

  const handleToggleFit = useCallback(() => {
    if (zoom !== 1) {
      setZoom(1)
      setOffset({ x: 0, y: 0 })
    } else {
      setZoom(2)
    }
  }, [zoom])

  const handleReset = useCallback(() => {
    setZoom(1)
    setRotation(0)
    setOffset({ x: 0, y: 0 })
  }, [])

  // Keyboard navigation
  useEffect(() => {
    if (!open) return
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
      else if (e.key === '+' || e.key === '=') handleZoomIn()
      else if (e.key === '-' || e.key === '_') handleZoomOut()
      else if (e.key === '0') handleReset()
      else if (e.key === 'r' || e.key === 'R') handleRotate()
      else if (e.key === 'ArrowLeft' && zoom > 1) setOffset((o) => ({ ...o, x: o.x + 40 }))
      else if (e.key === 'ArrowRight' && zoom > 1) setOffset((o) => ({ ...o, x: o.x - 40 }))
      else if (e.key === 'ArrowUp' && zoom > 1) setOffset((o) => ({ ...o, y: o.y + 40 }))
      else if (e.key === 'ArrowDown' && zoom > 1) setOffset((o) => ({ ...o, y: o.y - 40 }))
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [open, onClose, handleZoomIn, handleZoomOut, handleReset, handleRotate, zoom])

  // Non-passive wheel listener for smooth and reliable wheel zoom
  useEffect(() => {
    if (!open) return
    const el = viewportRef.current
    if (!el) return

    const onWheel = (e: WheelEvent) => {
      e.preventDefault()
      e.stopPropagation()
      const factor = e.deltaY < 0 ? 1.15 : 0.87
      setZoom((z) => {
        const next = Math.min(Math.max(Number((z * factor).toFixed(2)), 0.5), 5.0)
        if (next <= 1) setOffset({ x: 0, y: 0 })
        return next
      })
    }

    el.addEventListener('wheel', onWheel, { passive: false })
    return () => el.removeEventListener('wheel', onWheel)
  }, [open])

  // Drag pan handlers using Pointer Events with PointerCapture
  const handlePointerDown = (e: React.PointerEvent<HTMLImageElement>) => {
    if (zoom <= 1) return
    if (e.button !== 0) return
    e.preventDefault()
    e.stopPropagation()
    setIsDragging(true)
    setDragStart({ x: e.clientX - offset.x, y: e.clientY - offset.y })
    e.currentTarget.setPointerCapture(e.pointerId)
  }

  const handlePointerMove = (e: React.PointerEvent<HTMLImageElement>) => {
    if (!isDragging) return
    e.preventDefault()
    e.stopPropagation()
    setOffset({
      x: e.clientX - dragStart.x,
      y: e.clientY - dragStart.y,
    })
  }

  const handlePointerUp = (e: React.PointerEvent<HTMLImageElement>) => {
    if (isDragging) {
      setIsDragging(false)
      try {
        e.currentTarget.releasePointerCapture(e.pointerId)
      } catch {}
    }
  }

  const handleImageLoad = (e: React.SyntheticEvent<HTMLImageElement>) => {
    const target = e.currentTarget
    setImgSize({ width: target.naturalWidth, height: target.naturalHeight })
  }

  return (
    <RadixDialog.Root open={open} onOpenChange={(o) => !o && onClose()}>
      <AnimatePresence>
        {open && (
          <RadixDialog.Portal forceMount>
            {/* Backdrop */}
            <RadixDialog.Overlay asChild>
              <motion.div
                className="fixed inset-0 z-50 bg-black/90 backdrop-blur-md select-none"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 0.15 }}
                onClick={onClose}
              />
            </RadixDialog.Overlay>

            <RadixDialog.Content asChild>
              <motion.div
                className="fixed inset-0 z-50 flex flex-col items-center justify-between pointer-events-auto p-3 sm:p-5"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 0.15 }}
              >
                <RadixDialog.Title className="sr-only">{title || 'Cover Viewer'}</RadixDialog.Title>
                <RadixDialog.Description className="sr-only">Full-screen cover lightbox</RadixDialog.Description>

                {/* Top Header */}
                <div
                  className="z-20 flex w-full max-w-4xl items-center justify-between rounded-xl border border-white/10 bg-black/60 px-4 py-2.5 text-white/90 backdrop-blur-md shadow-xl"
                  onClick={(e) => e.stopPropagation()}
                >
                  <div className="flex min-w-0 items-center gap-3">
                    <p className="truncate text-sm font-medium">{title || 'Cover'}</p>
                    {imgSize && (
                      <span className="hidden rounded bg-white/10 px-2 py-0.5 text-xs font-mono text-white/70 sm:inline-block">
                        {imgSize.width} × {imgSize.height}
                      </span>
                    )}
                  </div>

                  <div className="flex items-center gap-2">
                    <button
                      type="button"
                      title="Open Original Image"
                      onClick={() => window.open(src, '_blank')}
                      className="inline-flex size-8 cursor-pointer items-center justify-center rounded-lg text-white/80 transition hover:bg-white/15 hover:text-white"
                    >
                      <ArrowSquareOut className="size-4" />
                    </button>
                    <RadixDialog.Close asChild>
                      <button
                        type="button"
                        title="Close (Esc)"
                        onClick={onClose}
                        className="inline-flex size-8 cursor-pointer items-center justify-center rounded-lg text-white/80 transition hover:bg-white/15 hover:text-white"
                      >
                        <X className="size-4" />
                      </button>
                    </RadixDialog.Close>
                  </div>
                </div>

                {/* Viewport Area */}
                <div
                  ref={viewportRef}
                  className="relative flex flex-1 w-full items-center justify-center overflow-hidden cursor-default"
                  onClick={(e) => {
                    if (e.target === e.currentTarget) onClose()
                  }}
                >
                  {/* Clean native img with zero Framer-Motion transform conflict */}
                  <img
                    ref={imgRef}
                    src={src}
                    alt={title || 'Cover'}
                    draggable={false}
                    onLoad={handleImageLoad}
                    style={{
                      transform: `translate3d(${offset.x}px, ${offset.y}px, 0) scale(${zoom}) rotate(${rotation}deg)`,
                      transformOrigin: 'center center',
                      transition: isDragging ? 'none' : 'transform 0.15s cubic-bezier(0.2, 0, 0, 1)',
                      cursor: zoom > 1 ? (isDragging ? 'grabbing' : 'grab') : 'zoom-in',
                      userSelect: 'none',
                      WebkitUserSelect: 'none',
                    }}
                    className="max-h-[78vh] max-w-[85vw] rounded-lg object-contain shadow-2xl ring-1 ring-white/10 select-none touch-none"
                    onPointerDown={handlePointerDown}
                    onPointerMove={handlePointerMove}
                    onPointerUp={handlePointerUp}
                    onPointerCancel={handlePointerUp}
                    onDoubleClick={handleToggleFit}
                  />
                </div>

                {/* Floating Bottom Control Pill Toolbar */}
                <div
                  className="z-20 flex items-center gap-1 sm:gap-2 rounded-full border border-white/15 bg-black/75 px-3 py-1.5 text-white/90 backdrop-blur-md shadow-2xl"
                  onClick={(e) => e.stopPropagation()}
                >
                  <button
                    type="button"
                    title="Zoom Out (-)"
                    onClick={handleZoomOut}
                    className="inline-flex size-8 cursor-pointer items-center justify-center rounded-full text-white/80 transition hover:bg-white/20 hover:text-white active:scale-95"
                  >
                    <MagnifyingGlassMinus className="size-4" />
                  </button>

                  <button
                    type="button"
                    title="Reset to 100% (0)"
                    onClick={handleReset}
                    className="px-2 py-0.5 text-xs font-mono font-medium rounded-full text-white/90 hover:bg-white/15 transition cursor-pointer"
                  >
                    {Math.round(zoom * 100)}%
                  </button>

                  <button
                    type="button"
                    title="Zoom In (+)"
                    onClick={handleZoomIn}
                    className="inline-flex size-8 cursor-pointer items-center justify-center rounded-full text-white/80 transition hover:bg-white/20 hover:text-white active:scale-95"
                  >
                    <MagnifyingGlassPlus className="size-4" />
                  </button>

                  <div className="h-4 w-px bg-white/20 mx-0.5" />

                  <button
                    type="button"
                    title={zoom === 1 ? 'Zoom 200%' : 'Fit to Screen'}
                    onClick={handleToggleFit}
                    className="inline-flex size-8 cursor-pointer items-center justify-center rounded-full text-white/80 transition hover:bg-white/20 hover:text-white active:scale-95"
                  >
                    {zoom === 1 ? <ArrowsOutSimple className="size-4" /> : <ArrowsInSimple className="size-4" />}
                  </button>

                  <button
                    type="button"
                    title="Rotate 90° (r)"
                    onClick={handleRotate}
                    className="inline-flex size-8 cursor-pointer items-center justify-center rounded-full text-white/80 transition hover:bg-white/20 hover:text-white active:scale-95"
                  >
                    <ArrowClockwise className="size-4" />
                  </button>
                </div>
              </motion.div>
            </RadixDialog.Content>
          </RadixDialog.Portal>
        )}
      </AnimatePresence>
    </RadixDialog.Root>
  )
}
