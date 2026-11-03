import { X } from '@phosphor-icons/react'
import { useTranslation } from 'react-i18next'
import type { BookDto } from '@/lib/api/types'
import { useReaderBackground } from '@/lib/store/readerSettings'
import { cn } from '@/lib/utils/cn'
import { urls } from '@/lib/utils/urls'

interface BookTransitionProps {
  direction: 'previous' | 'next'
  book: BookDto
  target: BookDto | null
  /** right-to-left reading keeps the later book on the reading-forward side */
  flip: boolean
  onLeave: () => void
}

function BookSection({
  badge,
  book,
  muted,
  hidePortraitCover = false,
}: {
  badge: string
  book: BookDto
  muted: string
  hidePortraitCover?: boolean
}) {
  return (
    <div className="flex w-full max-w-64 min-w-0 flex-col items-center gap-3">
      <p className={cn('text-[11px] font-semibold tracking-wider uppercase', muted)}>{badge}</p>
      <img
        src={urls.bookThumbnail(book.id)}
        alt=""
        draggable={false}
        className={cn(
          'h-[min(26dvh,14rem)] max-w-full rounded-lg object-contain shadow-pop landscape:h-[min(40dvh,18rem)]',
          hidePortraitCover && 'hidden landscape:block',
        )}
      />
      <div className="w-full space-y-0.5">
        <p className="line-clamp-2 text-[15px] font-semibold">{book.metadata.title || book.name}</p>
        {book.metadata.number && <p className={cn('text-xs', muted)}>#{book.metadata.number}</p>}
      </div>
    </div>
  )
}

/**
 * A page past either end of a book. It sits in the page flow instead of over it: one more
 * turn in the same direction moves on and the opposite turn returns to the book.
 */
export function BookTransition({ direction, book, target, flip, onLeave }: BookTransitionProps) {
  const { t } = useTranslation('reader')
  const light = useReaderBackground() === 'WHITE'
  const muted = light ? 'text-black/55' : 'text-white/55'
  const next = direction === 'next'

  const current = (
    <BookSection
      badge={t(next ? 'transition.finished' : 'transition.atStart')}
      book={book}
      muted={muted}
      hidePortraitCover
    />
  )
  const other = target ? (
    <BookSection
      badge={t(next ? 'transition.upNext' : 'transition.previousBook')}
      book={target}
      muted={muted}
    />
  ) : (
    <div className="flex w-full max-w-64 flex-col items-center gap-4">
      <p className="text-[15px] font-semibold">{t(next ? 'transition.noNext' : 'transition.noPrevious')}</p>
      <button
        type="button"
        className="inline-flex cursor-pointer items-center gap-1.5 rounded-full border border-current/25 px-4 py-1.5 text-[13px] font-medium transition-colors hover:bg-current/10"
        onClick={(e) => {
          e.stopPropagation()
          onLeave()
        }}
        onPointerDown={(e) => e.stopPropagation()}
        onPointerUp={(e) => e.stopPropagation()}
        onPointerCancel={(e) => e.stopPropagation()}
      >
        <X aria-hidden className="size-3.5" />
        {t('common:action.close')}
      </button>
    </div>
  )

  return (
    <div
      className={cn(
        'flex h-full w-full flex-col items-center justify-center gap-5 px-6 text-center',
        light ? 'text-black' : 'text-white',
      )}
    >
      {book.seriesTitle && <p className={cn('max-w-lg truncate text-sm', muted)}>{book.seriesTitle}</p>}
      <div
        className={cn(
          'flex w-full flex-col items-center justify-center gap-5 landscape:gap-10',
          flip ? 'landscape:flex-row-reverse' : 'landscape:flex-row',
        )}
      >
        {next ? current : other}
        <div className="h-px w-3/4 max-w-sm shrink-0 bg-current opacity-15 landscape:h-[min(32dvh,20rem)] landscape:w-px" />
        {next ? other : current}
      </div>
      {target && <p className={cn('text-xs', muted)}>{t('transition.turnToContinue')}</p>}
    </div>
  )
}
