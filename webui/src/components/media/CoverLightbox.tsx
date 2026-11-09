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
import { IconButton } from '@/components/ui/IconButton'

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
  const imgRef = useRef<HTMLImageElement>(null)

  // Reset transform when opened or image changes
  useEffect(() => {
    if (open) {
      setZoom(1)
      setRotation(0)
      setOffset({ x: 0, y: 0 })
      setIsDragging(false)
    }
  }, [open, src])

  const handleZoomIn = useCallback(() => {
    setZoom((z) => Math.min(z + 0.25, 4))
  }, [])

  const handleZoomOut = useCallback(() => {
    setZoom((z) => {
      const next = Math.max(z - 0.25, 0.5)
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
      else if (e.key === '-') handleZoomOut()
      else if (e.key === '0') handleReset()
      else if (e.key === 'r' || e.key === 'R') handleRotate()
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [open, onClose, handleZoomIn, handleZoomOut, handleReset, handleRotate])

  // Mouse wheel zoom
  const handleWheel = (e: React.WheelEvent) => {
    e.preventDefault()
    if (e.deltaY < 0) {
      handleZoomIn()
    } else {
      handleZoomOut()
    }
  }

  // Drag pan handlers
  const handleMouseDown = (e: React.MouseEvent) => {
    if (zoom <= 1) return
    setIsDragging(true)
    setDragStart({ x: e.clientX - offset.x, y: e.clientY - offset.y })
  }

  const handleMouseMove = (e: React.MouseEvent) => {
    if (!isDragging) return
    setOffset({
      x: e.clientX - dragStart.x,
      y: e.clientY - dragStart.y,
    })
  }

  const handleMouseUp = () => {
    setIsDragging(false)
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
                transition={{ duration: 0.18 }}
              />
            </RadixDialog.Overlay>

            <RadixDialog.Content asChild>
              <div
                className="fixed inset-0 z-50 flex flex-col items-center justify-between p-3 sm:p-5"
                onMouseMove={handleMouseMove}
                onMouseUp={handleMouseUp}
                onWheel={handleWheel}
              >
                {/* Header Toolbar */}
                <motion.header
                  className="z-10 flex w-full max-w-5xl items-center justify-between rounded-xl border border-white/10 bg-black/60 px-4 py-2 text-white/90 backdrop-blur-md shadow-lg"
                  initial={{ opacity: 0, y: -16 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, y: -16 }}
                >
                  <div className="flex min-w-0 items-center gap-3">
                    <p className="truncate text-sm font-medium">{title || 'Cover Viewer'}</p>
                    {imgSize && (
                      <span className="hidden rounded bg-white/10 px-2 py-0.5 text-xs font-mono text-white/70 sm:inline-block">
                        {imgSize.width} × {imgSize.height}
                      </span>
                    )}
                    <span className="rounded bg-white/10 px-2 py-0.5 text-xs font-mono text-white/70">
                      {Math.round(zoom * 100)}%
                    </span>
                  </div>

                  {/* Actions */}
                  <div className="flex items-center gap-1">
                    <IconButton label="Zoom Out" onClick={handleZoomOut} className="text-white/80 hover:text-white">
                      <MagnifyingGlassMinus className="size-4" />
                    </IconButton>
                    <IconButton label="Zoom In" onClick={handleZoomIn} className="text-white/80 hover:text-white">
                      <MagnifyingGlassPlus className="size-4" />
                    </IconButton>
                    <IconButton
                      label={zoom === 1 ? 'Zoom 200%' : 'Fit to Screen'}
                      onClick={handleToggleFit}
                      className="text-white/80 hover:text-white"
                    >
                      {zoom === 1 ? <ArrowsOutSimple className="size-4" /> : <ArrowsInSimple className="size-4" />}
                    </IconButton>
                    <IconButton label="Rotate" onClick={handleRotate} className="text-white/80 hover:text-white">
                      <ArrowClockwise className="size-4" />
                    </IconButton>
                    <IconButton
                      label="Open Original"
                      onClick={() => window.open(src, '_blank')}
                      className="text-white/80 hover:text-white"
                    >
                      <ArrowSquareOut className="size-4" />
                    </IconButton>
                    <div className="mx-1 h-4 w-px bg-white/20" />
                    <IconButton label="Close" onClick={onClose} className="text-white/80 hover:text-white hover:bg-white/10">
                      <X className="size-4" />
                    </IconButton>
                  </div>
                </motion.header>

                {/* Viewport Area */}
                <div
                  className="relative flex flex-1 w-full items-center justify-center overflow-hidden cursor-default"
                  onClick={(e) => {
                    // Click on background closes viewer
                    if (e.target === e.currentTarget) onClose()
                  }}
                >
                  <motion.div
                    className="relative flex items-center justify-center touch-none"
                    initial={{ opacity: 0, scale: 0.92 }}
                    animate={{ opacity: 1, scale: 1 }}
                    exit={{ opacity: 0, scale: 0.92 }}
                    transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
                    style={{
                      transform: `translate3d(${offset.x}px, ${offset.y}px, 0) scale(${zoom}) rotate(${rotation}deg)`,
                      cursor: zoom > 1 ? (isDragging ? 'grabbing' : 'grab') : 'zoom-in',
                    }}
                    onMouseDown={handleMouseDown}
                    onDoubleClick={handleToggleFit}
                  >
                    <img
                      ref={imgRef}
                      src={src}
                      alt={title || 'Cover'}
                      draggable={false}
                      onLoad={handleImageLoad}
                      className="max-h-[82vh] max-w-[88vw] rounded-lg object-contain shadow-2xl ring-1 ring-white/10"
                    />
                  </motion.div>
                </div>

                {/* Bottom hint bar */}
                <motion.div
                  className="hidden text-xs text-white/50 sm:flex items-center gap-4 py-1"
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                >
                  <span>滚轮缩放</span>
                  <span>·</span>
                  <span>双击放大 / 还原</span>
                  <span>·</span>
                  <span>拖拽平移</span>
                  <span>·</span>
                  <span>Esc 退出</span>
                </motion.div>
              </div>
            </RadixDialog.Content>
          </RadixDialog.Portal>
        )}
      </AnimatePresence>
    </RadixDialog.Root>
  )
}
