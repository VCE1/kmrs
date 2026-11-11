import { useState } from 'react'
import { useSearchParams } from 'react-router-dom'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import {
  Books,
  CaretLeft,
  CaretRight,
  FolderSimple,
  House,
  WarningCircle,
} from '@phosphor-icons/react'
import { librariesApi } from '@/lib/api/libraries'
import type { DirectoryDto } from '@/lib/api/types'
import { urls } from '@/lib/utils/urls'
import { MediaGrid } from '@/components/media/MediaGrid'
import { SeriesCard } from '@/components/media/SeriesCard'
import { CardFrame, CardOverlayText, CardText } from '@/components/media/CardFrame'
import { GridSkeleton } from '@/components/ui/Skeleton'
import { EmptyState } from '@/components/ui/EmptyState'
import { Button } from '@/components/ui/Button'

interface DirectoryBrowserProps {
  libraryId: string
}

function DirectoryCard({
  libraryId,
  dir,
  onNavigate,
}: {
  libraryId: string
  dir: DirectoryDto
  onNavigate: (path: string) => void
}) {
  const { t } = useTranslation('browse')
  const [imgError, setImgError] = useState(false)
  const secondary =
    dir.subDirectoryCount > 0
      ? `${t('directory.bookCount', { count: dir.bookCount })} · ${t('directory.subfolders', { count: dir.subDirectoryCount })}`
      : t('directory.bookCount', { count: dir.bookCount })
  const showCover = !!dir.sampleSeriesId && !imgError

  return (
    <CardFrame
      to={`/libraries/${libraryId}/series?path=${encodeURIComponent(dir.path)}`}
      label={dir.name}
    >
      <div
        className="relative"
        onClick={(e) => {
          // If normal primary click without modifiers, handle in-place navigation
          if (e.button === 0 && !e.metaKey && !e.ctrlKey && !e.shiftKey && !e.altKey) {
            e.preventDefault()
            onNavigate(dir.path)
          }
        }}
      >
        <div className="cover-aspect relative w-full overflow-hidden rounded-lg bg-surface border border-line/40 transition-all duration-300 group-hover:border-accent/40 group-hover:shadow-card-hover group-hover:brightness-105">
          {showCover ? (
            <>
              <img
                src={urls.seriesThumbnail(dir.sampleSeriesId!)}
                alt={dir.name}
                loading="lazy"
                onError={() => setImgError(true)}
                className="size-full object-cover transition-transform duration-300 group-hover:scale-105"
              />
              <div className="absolute inset-0 bg-gradient-to-t from-black/85 via-black/35 to-black/20" />
              <div className="absolute top-2 left-2 flex items-center justify-center rounded-md bg-black/60 p-1.5 text-accent shadow-sm backdrop-blur-md">
                <FolderSimple size={20} weight="fill" />
              </div>
            </>
          ) : (
            <div className="flex size-full flex-col items-center justify-center gap-2 bg-gradient-to-br from-raised to-surface text-ink-3">
              <div className="flex size-14 items-center justify-center rounded-2xl bg-accent/10 text-accent transition-transform duration-300 group-hover:scale-110 shadow-inner">
                <FolderSimple size={36} weight="duotone" />
              </div>
            </div>
          )}

          {/* Badges in top-right corner */}
          <div className="absolute top-2 right-2 flex flex-col items-end gap-1">
            {dir.subDirectoryCount > 0 && (
              <span className="inline-flex items-center gap-1 rounded-full bg-black/65 px-2 py-0.5 text-[10px] font-medium text-white/90 backdrop-blur-sm">
                <FolderSimple size={11} weight="bold" />
                {dir.subDirectoryCount}
              </span>
            )}
            <span className="inline-flex items-center gap-1 rounded-full bg-accent/90 px-2 py-0.5 text-[10px] font-semibold text-white shadow-sm backdrop-blur-sm">
              <Books size={11} weight="bold" />
              {dir.bookCount}
            </span>
          </div>

          <CardOverlayText title={dir.name} secondary={secondary} />
        </div>
      </div>
      <CardText title={dir.name} secondary={secondary} />
    </CardFrame>
  )
}

export function DirectoryBrowser({ libraryId }: DirectoryBrowserProps) {
  const { t } = useTranslation('browse')
  const [searchParams, setSearchParams] = useSearchParams()
  const currentPath = searchParams.get('path') || ''

  const { data, isLoading, isError, refetch } = useQuery({
    queryKey: ['libraries', libraryId, 'browse', currentPath],
    queryFn: () => librariesApi.browse(libraryId, currentPath || undefined),
  })

  const setPath = (path: string) => {
    setSearchParams((prev) => {
      const next = new URLSearchParams(prev)
      if (path) {
        next.set('path', path)
      } else {
        next.delete('path')
      }
      return next
    })
  }

  // Determine parent path for "Up one level"
  const parentPath = (() => {
    if (!currentPath) return null
    const parts = currentPath.split('/').filter(Boolean)
    if (parts.length <= 1) return ''
    return parts.slice(0, -1).join('/')
  })()

  if (isLoading) {
    return (
      <div className="space-y-4">
        <div className="h-9 w-64 animate-pulse rounded-lg bg-surface" />
        <GridSkeleton count={12} />
      </div>
    )
  }

  if (isError || !data) {
    return (
      <EmptyState
        icon={<WarningCircle />}
        title={t('series.loadError')}
        action={
          <Button variant="secondary" onClick={() => refetch()}>
            {t('common:retry', '重试')}
          </Button>
        }
      />
    )
  }

  const hasDirectories = data.directories.length > 0
  const hasSeries = data.series.length > 0
  const isEmpty = !hasDirectories && !hasSeries

  return (
    <div className="space-y-5">
      {/* Breadcrumb Navigation Bar */}
      <nav
        aria-label="Directory Breadcrumbs"
        className="flex flex-wrap items-center gap-1.5 rounded-xl border border-line bg-surface/60 px-3.5 py-2 text-sm backdrop-blur-sm"
      >
        {parentPath !== null && (
          <button
            type="button"
            onClick={() => setPath(parentPath)}
            className="mr-1 inline-flex min-h-9 items-center gap-1 rounded-md px-2 py-1 text-xs font-medium text-ink-2 transition-colors hover:bg-raised hover:text-ink pointer-coarse:min-h-11"
            title={t('directory.parent')}
          >
            <CaretLeft size={14} weight="bold" />
            <span>{t('directory.parent')}</span>
          </button>
        )}

        {data.breadcrumbs.map((crumb, idx) => {
          const isLast = idx === data.breadcrumbs.length - 1
          const isRoot = idx === 0

          return (
            <div key={crumb.path || 'root'} className="inline-flex items-center gap-1.5">
              {idx > 0 && <CaretRight size={13} className="text-ink-4 shrink-0" />}
              {isLast ? (
                <span className="flex min-w-0 items-center gap-1.5 font-semibold text-ink" title={crumb.name}>
                  {isRoot && <House size={15} weight="bold" className="shrink-0 text-accent" />}
                  <span className="break-words [overflow-wrap:anywhere]">{crumb.name}</span>
                </span>
              ) : (
                <button
                  type="button"
                  onClick={() => setPath(crumb.path)}
                  title={crumb.name}
                  className="flex min-h-9 min-w-0 cursor-pointer items-center gap-1.5 font-medium text-ink-3 transition-colors hover:text-ink hover:underline pointer-coarse:min-h-11"
                >
                  {isRoot && <House size={15} className="shrink-0" />}
                  <span className="break-words text-left [overflow-wrap:anywhere]">{crumb.name}</span>
                </button>
              )}
            </div>
          )
        })}
      </nav>

      {/* Empty State */}
      {isEmpty && (
        <EmptyState
          icon={<FolderSimple />}
          title={t('directory.empty')}
          action={
            parentPath !== null ? (
              <Button variant="secondary" onClick={() => setPath(parentPath)}>
                {t('directory.parent')}
              </Button>
            ) : undefined
          }
        />
      )}

      {/* Subdirectories Grid */}
      {hasDirectories && (
        <section>
          {hasSeries && (
            <div className="mb-3 flex items-center gap-2 text-xs font-semibold uppercase tracking-wider text-ink-3">
              <FolderSimple size={16} weight="bold" className="text-accent" />
              <span>
                {t('directory.subfoldersSection')} ({data.directories.length})
              </span>
            </div>
          )}
          <MediaGrid>
            {data.directories.map((dir) => (
              <DirectoryCard
                key={dir.path}
                libraryId={libraryId}
                dir={dir}
                onNavigate={setPath}
              />
            ))}
          </MediaGrid>
        </section>
      )}

      {/* Direct Series Grid */}
      {hasSeries && (
        <section>
          {hasDirectories && (
            <div className="mt-8 mb-3 flex items-center gap-2 text-xs font-semibold uppercase tracking-wider text-ink-3">
              <Books size={16} weight="bold" className="text-accent" />
              <span>
                {t('directory.seriesSection')} ({data.series.length})
              </span>
            </div>
          )}
          <MediaGrid>
            {data.series.map((series) => (
              <SeriesCard key={series.id} series={series} />
            ))}
          </MediaGrid>
        </section>
      )}
    </div>
  )
}
