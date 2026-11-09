import { useState, useEffect } from 'react'
import { useParams, useSearchParams } from 'react-router-dom'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { FolderSimple, SquaresFour } from '@phosphor-icons/react'
import { librariesApi } from '@/lib/api/libraries'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { HistoryBackButton } from '@/components/ui/BackButton'
import { PageHeader } from '@/components/ui/PageHeader'
import { BrowseTabs } from '@/components/browse/BrowseTabs'
import { SeriesGrid } from '@/components/browse/SeriesGrid'
import { DirectoryBrowser } from '@/components/browse/DirectoryBrowser'
import { SegmentedControl } from '@/components/ui/SegmentedControl'

type ViewMode = 'grid' | 'folder'

export function LibrarySeriesPage() {
  const { t } = useTranslation('browse')
  const { libraryId = '' } = useParams()
  const [searchParams, setSearchParams] = useSearchParams()
  const pathParam = searchParams.get('path')

  const { data: libraries } = useQuery({ queryKey: ['libraries'], queryFn: librariesApi.list })
  const library = libraries?.find((l) => l.id === libraryId)
  const title = library?.name ?? t('libraryFallback')

  useDocumentTitle(title)

  const [viewMode, setViewMode] = useState<ViewMode>(() => {
    if (pathParam !== null) return 'folder'
    const saved = localStorage.getItem('kmrs_browse_view_mode')
    return saved === 'folder' ? 'folder' : 'grid'
  })

  // If URL has path param, switch to folder mode
  useEffect(() => {
    if (pathParam !== null && viewMode !== 'folder') {
      setViewMode('folder')
    }
  }, [pathParam, viewMode])

  const handleModeChange = (mode: ViewMode) => {
    setViewMode(mode)
    localStorage.setItem('kmrs_browse_view_mode', mode)
    if (mode === 'grid' && pathParam !== null) {
      setSearchParams((prev) => {
        const next = new URLSearchParams(prev)
        next.delete('path')
        return next
      })
    }
  }

  const switcher = (
    <SegmentedControl<ViewMode>
      size="sm"
      value={viewMode}
      onChange={handleModeChange}
      options={[
        { value: 'grid', label: t('viewModes.grid'), icon: <SquaresFour size={15} /> },
        { value: 'folder', label: t('viewModes.folder'), icon: <FolderSimple size={15} /> },
      ]}
    />
  )

  return (
    <div>
      <HistoryBackButton to="/dashboard" className="mb-2 -ml-2" />
      <PageHeader title={title} />
      <BrowseTabs tab="series" extra={switcher} />
      {viewMode === 'folder' ? (
        <DirectoryBrowser libraryId={libraryId} />
      ) : (
        <SeriesGrid libraryId={libraryId} />
      )}
    </div>
  )
}
