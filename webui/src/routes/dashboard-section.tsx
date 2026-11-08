import { Fragment, useEffect, useState } from 'react'
import { Navigate, useParams } from 'react-router-dom'
import { useInfiniteQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Books, WarningCircle } from '@phosphor-icons/react'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { usePinnedLibraries } from '@/lib/store/clientSettings'
import type { SeriesDto } from '@/lib/api/types'
import { BackButton } from '@/components/ui/BackButton'
import { Button } from '@/components/ui/Button'
import { EmptyState } from '@/components/ui/EmptyState'
import { PageHeader } from '@/components/ui/PageHeader'
import { GridSkeleton } from '@/components/ui/Skeleton'
import { MediaGrid } from '@/components/media/MediaGrid'
import { Sentinel } from '@/components/filters/Sentinel'
import { DASHBOARD_SECTIONS, type DashboardSectionKey } from '@/components/dashboard/sections'
import { dashboardLibraryIds, dashboardScope } from '@/components/dashboard/sectionConfig'
import { PinnedLibrariesEmpty } from '@/components/dashboard/PinnedLibrariesEmpty'
import { PinLibrariesDialog } from '@/components/layout/PinLibrariesDialog'
import { BooksSelectionBar } from '@/components/browse/BooksSelectionBar'
import { SeriesSelectionBar } from '@/components/browse/SeriesSelectionBar'
import { SelectionModeButton } from '@/components/selection/SelectionModeButton'
import { cardSelection, useSelection } from '@/components/selection/useSelection'
import { useRecentReadingDays } from '@/lib/hooks/useRecentReadingDays'

export function DashboardSectionPage() {
  const { t } = useTranslation('dashboard')
  const { libraryId, sectionKey = '' } = useParams()
  const section = Object.hasOwn(DASHBOARD_SECTIONS, sectionKey)
    ? DASHBOARD_SECTIONS[sectionKey as DashboardSectionKey]
    : undefined
  const homeTo = libraryId ? `/libraries/${libraryId}/recommended` : '/dashboard'

  // pinned stays undefined until settings load; don't fire an unfiltered query in that window
  const { pinned, isPending: settingsPending } = usePinnedLibraries()
  const libraryIds = dashboardLibraryIds(libraryId, pinned)
  const scope = dashboardScope(libraryId, pinned)
  const pinnedEmpty = !libraryId && pinned !== undefined && pinned.length === 0
  const [pinDialogOpen, setPinDialogOpen] = useState(false)
  const selection = useSelection()
  const clearSelection = selection.clear
  useEffect(() => {
    clearSelection()
  }, [sectionKey, scope, clearSelection])

  useDocumentTitle(section ? t(section.titleKey) : undefined)

  const { days: recentReadingDays, isPending: readingWindowPending } = useRecentReadingDays()

  // same key+queryFn as the dashboard row, so the first pages come straight from cache
  const query = useInfiniteQuery({
    enabled: !!section && !settingsPending && !readingWindowPending && !pinnedEmpty,
    queryKey: ['dashboard', sectionKey, scope, recentReadingDays],
    queryFn: ({ pageParam }) => {
      if (!section) throw new Error(`unknown dashboard section: ${sectionKey}`)
      return section.fetchPage(libraryIds, pageParam, { recentReadingDays })
    },
    getNextPageParam: (last) => (last.last ? undefined : last.number + 1),
    initialPageParam: 0,
  })

  if (!section) return <Navigate to={homeTo} replace />

  const items = query.data?.pages.flatMap((p) => p.content) ?? []
  const loadedIds = items.map((item) => item.id)
  const total = query.data?.pages[0]?.totalElements

  return (
    <div>
      <BackButton to={homeTo} className="mb-2 -ml-2" />
      <PageHeader
        title={t(section.titleKey)}
        subtitle={total != null ? t(section.kind === 'book' ? 'bookCount' : 'seriesCount', { count: total }) : undefined}
        actions={<SelectionModeButton active={selection.active} onToggle={selection.toggleActive} />}
      />
      {pinnedEmpty ? (
        <PinnedLibrariesEmpty onManage={() => setPinDialogOpen(true)} />
      ) : query.isPending ? (
        <GridSkeleton count={18} />
      ) : query.isLoadingError ? (
        <EmptyState
          icon={<WarningCircle />}
          title={t('loadError')}
          body={query.error.message}
          action={<Button onClick={() => query.refetch()}>{t('common:action.retry')}</Button>}
        />
      ) : items.length === 0 ? (
        <EmptyState icon={<Books />} title={t('empty.title')} />
      ) : (
        <>
          <MediaGrid>
            {items.map((item) => (
              <Fragment key={item.id}>{section.renderGrid(item, cardSelection(selection, item.id, loadedIds))}</Fragment>
            ))}
          </MediaGrid>
          <Sentinel
            active={!!query.hasNextPage}
            onIntersect={() => {
              if (!query.isFetchingNextPage) void query.fetchNextPage()
            }}
          />
        </>
      )}
      {section.kind === 'book' ? (
        <BooksSelectionBar selection={selection} loadedIds={loadedIds} />
      ) : (
        <SeriesSelectionBar selection={selection} items={items as SeriesDto[]} />
      )}
      <PinLibrariesDialog open={pinDialogOpen} onOpenChange={setPinDialogOpen} />
    </div>
  )
}
