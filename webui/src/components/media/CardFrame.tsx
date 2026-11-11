import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from 'react'
import { Link } from 'react-router-dom'
import { DotsThree } from '@phosphor-icons/react'
import { cn } from '@/lib/utils/cn'
import { useUiStore } from '@/lib/store/ui'
import { useLongPress } from '@/lib/hooks/useLongPress'

interface CardFrameProps {
  to: string
  children: ReactNode
  className?: string
  label: string
  /** overlay controls (e.g. the actions menu); rendered as siblings of the link so no
      interactive element nests inside the <a> */
  actions?: ReactNode
  onLongPress?: () => void
}

/** Shared hover/press treatment for all media cards. */
export function CardFrame({ to, children, className, label, actions, onLongPress }: CardFrameProps) {
  const longPress = useLongPress(onLongPress ?? (() => undefined))
  const link = (
    <Link
      to={to}
      aria-label={label}
      title={label}
      {...(onLongPress ? longPress : {})}
      className={cn(
        'group block cursor-pointer rounded-lg outline-none transition-transform duration-200 ease-out',
        onLongPress && '[-webkit-touch-callout:none] select-none',
        'active:scale-[0.98]',
        actions ? undefined : cn('animate-card-in', className),
      )}
    >
      {children}
    </Link>
  )
  if (!actions) return link
  return (
    <div className={cn('group relative animate-card-in rounded-lg', className)}>
      {link}
      {/* the frame also spans the text under the cover; this box is the cover's area */}
      <div className="cover-aspect pointer-events-none absolute inset-x-0 top-0">{actions}</div>
    </div>
  )
}

/** Hover-revealed corner action pinned to the cover's bottom-right (badges live top-right,
    the selection checkbox top-left). Forwards props+ref because Radix Trigger asChild
    injects handlers/state onto the child. Defaults to the menu dots; pass children to use
    it for another card action. */
export const CardMenuButton = forwardRef<HTMLButtonElement, ButtonHTMLAttributes<HTMLButtonElement>>(
  function CardMenuButton({ className, children, ...rest }, ref) {
    return (
      <button
        ref={ref}
        type="button"
        className={cn(
          'pointer-events-auto absolute right-1 bottom-1 z-10 inline-flex size-7 cursor-pointer items-center justify-center rounded-full bg-black/60 text-white shadow-sm backdrop-blur-sm transition-[opacity,background-color] duration-300 ease-out hover:bg-black/80 pointer-coarse:size-9',
          'opacity-0 group-hover:opacity-100 focus-visible:opacity-100 data-[state=open]:opacity-100',
          className,
        )}
        {...rest}
      >
        {children ?? <DotsThree className="size-4" weight="bold" />}
      </button>
    )
  },
)

interface CardTextProps {
  title: string
  /** caption line above the title (e.g. series title) */
  overline?: ReactNode
  secondary?: ReactNode
  /** title clamps to this many lines (default 1) */
  titleLines?: 1 | 2
  center?: boolean
}

interface CardOverlayTextProps extends CardTextProps {
  /** keeps the text clear of the CardMenuButton sharing the cover's bottom-right */
  menuInset?: boolean
}

/** Title block under a cover; honors the global card-style preference. */
export function CardText({ title, overline, secondary, titleLines = 1 }: CardTextProps) {
  const cardStyle = useUiStore((s) => s.cardStyle)
  if (cardStyle !== 'standard') return null
  return (
    <div className="mt-1 min-w-0 px-1.5">
      {overline && <p className="mb-0.5 truncate text-[11px] leading-snug text-ink-3">{overline}</p>}
      <p
        className={cn('text-[13px] leading-snug font-medium text-ink', titleLines === 2 ? 'line-clamp-2' : 'truncate')}
        title={title}
      >
        {title}
      </p>
      {secondary && <p className="mt-0.5 truncate text-xs text-ink-3">{secondary}</p>}
    </div>
  )
}

/** Overlay variant: gradient on the cover itself, white text. */
export function CardOverlayText({ title, overline, secondary, titleLines = 1, menuInset }: CardOverlayTextProps) {
  const cardStyle = useUiStore((s) => s.cardStyle)
  if (cardStyle !== 'overlay') return null
  return (
    <div
      className={cn(
        'pointer-events-none absolute inset-x-0 bottom-0 rounded-b-lg bg-gradient-to-t from-black/85 via-black/40 to-transparent px-2.5 pt-8 pb-2',
        menuInset && 'pr-9',
      )}
    >
      {overline && <p className="mb-0.5 truncate text-[11px] text-white/70">{overline}</p>}
      <p
        className={cn('text-[12.5px] leading-snug font-medium text-white', titleLines === 2 ? 'line-clamp-2' : 'truncate')}
        title={title}
      >
        {title}
      </p>
      {secondary && <p className="mt-0.5 truncate text-[11px] text-white/70">{secondary}</p>}
    </div>
  )
}
