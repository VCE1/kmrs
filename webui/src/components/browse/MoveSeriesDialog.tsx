import { useEffect, useState, type ReactNode } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { BookBookmark, CaretRight, FolderSimple } from '@phosphor-icons/react'
import { Dialog } from '@/components/ui/Dialog'
import { Button } from '@/components/ui/Button'
import { librariesApi } from '@/lib/api/libraries'
import type { DirectoryDto, LibraryDto } from '@/lib/api/types'
import { cn } from '@/lib/utils/cn'

export interface MoveDestination {
  libraryId: string
  categoryPath?: string
}

interface MoveSeriesDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  name: string
  bookCount: number
  currentLibraryId: string
  libraries: LibraryDto[]
  loading: boolean
  onConfirm: (destination: MoveDestination) => void
}

export function MoveSeriesDialog({
  open,
  onOpenChange,
  name,
  bookCount,
  currentLibraryId,
  libraries,
  loading,
  onConfirm,
}: MoveSeriesDialogProps) {
  const { t } = useTranslation('detail')
  const choices = [...libraries].sort((a, b) => a.name.localeCompare(b.name, 'zh'))
  const [destination, setDestination] = useState<MoveDestination | null>(null)

  useEffect(() => {
    if (!open) return
    setDestination(null)
  }, [open])

  return (
    <Dialog open={open} onOpenChange={onOpenChange} title={t('moveSeries.title')} size="md">
      <div className="space-y-3 px-5 py-4">
        <p className="text-sm text-ink-2">{t('moveSeries.body', { name, count: bookCount })}</p>
        {choices.length === 0 ? (
          <p className="text-sm text-ink-3">{t('moveSeries.empty')}</p>
        ) : (
          <div>
            <p className="mb-1.5 text-sm text-ink-2">{t('moveSeries.pick')}</p>
            <div className="max-h-[50dvh] overflow-y-auto rounded-lg border border-line">
              {choices.map((library) => (
                <LibraryChoice
                  key={library.id}
                  library={library}
                  active={open}
                  currentLibraryId={currentLibraryId}
                  destination={destination}
                  onSelect={setDestination}
                />
              ))}
            </div>
          </div>
        )}
      </div>
      <div className="flex justify-end gap-2 border-t border-line px-5 py-3.5">
        <Button variant="ghost" onClick={() => onOpenChange(false)}>
          {t('common:action.cancel')}
        </Button>
        <Button
          loading={loading}
          disabled={!destination || choices.length === 0}
          onClick={() => destination && onConfirm(destination)}
        >
          {t('moveSeries.confirm')}
        </Button>
      </div>
    </Dialog>
  )
}

function LibraryChoice({
  library,
  active,
  currentLibraryId,
  destination,
  onSelect,
}: {
  library: LibraryDto
  active: boolean
  currentLibraryId: string
  destination: MoveDestination | null
  onSelect: (destination: MoveDestination) => void
}) {
  const nestedQuery = useQuery({
    queryKey: ['libraries', library.id, 'nested'],
    queryFn: () => librariesApi.nested(library.id),
    enabled: active,
  })
  const categories = nestedQuery.data?.nested ?? false
  const pending = active && nestedQuery.isPending
  const [open, setOpen] = useState(false)
  const selected =
    destination?.libraryId === library.id && destination.categoryPath === undefined && !categories
  const currentRoot = library.id === currentLibraryId
  if (currentRoot && !categories) return null

  return (
    <div className="border-b border-line last:border-b-0">
      <div className="flex items-stretch">
        {categories ? (
          <button
            type="button"
            aria-expanded={open}
            aria-label={library.name}
            title={library.name}
            onClick={() => setOpen((value) => !value)}
            className="flex w-10 shrink-0 cursor-pointer items-center justify-center text-ink-3 hover:bg-raised hover:text-ink pointer-coarse:w-11"
          >
            <CaretRight className={cn('size-3.5 transition-transform duration-200', open && 'rotate-90')} />
          </button>
        ) : (
          <span className="w-10 shrink-0 pointer-coarse:w-11" />
        )}
        <button
          type="button"
          title={library.name}
          disabled={categories || currentRoot || pending}
          aria-pressed={selected}
          onClick={() => onSelect({ libraryId: library.id })}
          className={cn(
            'flex min-h-10 min-w-0 flex-1 items-center gap-2 px-1 py-2 text-left text-[13.5px] pointer-coarse:min-h-11',
            categories || currentRoot ? 'cursor-default text-ink' : 'cursor-pointer hover:bg-raised',
            selected && 'bg-accent-soft font-medium text-accent-strong',
          )}
        >
          <BookBookmark className="size-[18px] shrink-0" />
          <span className="min-w-0 flex-1 truncate pointer-coarse:line-clamp-2 pointer-coarse:whitespace-normal">
            {library.name}
          </span>
        </button>
      </div>
      {categories && open ? (
        <div className="ml-3 border-l border-line pb-1 pl-1">
          <CategoryLevel
            libraryId={library.id}
            parentPath=""
            selectedPath={destination?.libraryId === library.id ? destination.categoryPath : undefined}
            onSelect={(path) => onSelect({ libraryId: library.id, categoryPath: path })}
          />
        </div>
      ) : null}
    </div>
  )
}

function CategoryLevel({
  libraryId,
  parentPath,
  selectedPath,
  onSelect,
}: {
  libraryId: string
  parentPath: string
  selectedPath?: string
  onSelect: (path: string) => void
}) {
  const { t } = useTranslation('detail')
  const query = useQuery({
    queryKey: ['libraries', libraryId, 'browse', parentPath],
    queryFn: () => librariesApi.browse(libraryId, parentPath || undefined),
  })
  if (query.isLoading) {
    return <div className="mx-2 my-1 h-6 animate-pulse rounded-md bg-raised" />
  }
  if (query.isError) {
    return (
      <button
        type="button"
        onClick={() => query.refetch()}
        className="px-2 py-1 text-left text-[12.5px] text-ink-3 hover:text-ink"
      >
        {t('moveSeries.retry')}
      </button>
    )
  }
  const directories = query.data?.directories ?? []
  if (directories.length === 0) {
    return <p className="px-2 py-1 text-[12.5px] text-ink-3">{t('moveSeries.noCategories')}</p>
  }
  return (
    <>
      {directories.map((dir) => (
        <CategoryRow
          key={dir.path}
          libraryId={libraryId}
          dir={dir}
          selectedPath={selectedPath}
          onSelect={onSelect}
        />
      ))}
    </>
  )
}

function CategoryRow({
  libraryId,
  dir,
  selectedPath,
  onSelect,
}: {
  libraryId: string
  dir: DirectoryDto
  selectedPath?: string
  onSelect: (path: string) => void
}) {
  const selected = selectedPath === dir.path
  const childSelected = selectedPath === dir.path || (selectedPath?.startsWith(`${dir.path}/`) ?? false)
  const [open, setOpen] = useState(childSelected)
  const canNest = dir.subDirectoryCount > 0

  return (
    <div>
      <div className="flex items-stretch">
        {canNest ? (
          <button
            type="button"
            aria-expanded={open}
            aria-label={dir.name}
            title={dir.name}
            onClick={() => setOpen((value) => !value)}
            className="flex w-8 shrink-0 cursor-pointer items-center justify-center rounded-md text-ink-3 hover:bg-raised hover:text-ink pointer-coarse:w-11"
          >
            <CaretRight className={cn('size-3.5 transition-transform duration-200', open && 'rotate-90')} />
          </button>
        ) : (
          <span className="w-8 shrink-0 pointer-coarse:w-11" />
        )}
        <button
          type="button"
          title={dir.name}
          aria-pressed={selected}
          onClick={() => onSelect(dir.path)}
          className={cn(
            'flex min-h-10 min-w-0 flex-1 cursor-pointer items-center gap-2 rounded-lg px-1.5 py-1.5 text-left text-[13px] pointer-coarse:min-h-11',
            selected ? 'bg-accent-soft font-medium text-accent-strong' : 'text-ink-2 hover:bg-raised hover:text-ink',
          )}
        >
          <FolderSimple className="size-4 shrink-0" />
          <span className="min-w-0 flex-1 truncate pointer-coarse:line-clamp-2 pointer-coarse:whitespace-normal">
            {dir.name}
          </span>
        </button>
      </div>
      {canNest && open ? (
        <CategoryBranch>
          <CategoryLevel
            libraryId={libraryId}
            parentPath={dir.path}
            selectedPath={selectedPath}
            onSelect={onSelect}
          />
        </CategoryBranch>
      ) : null}
    </div>
  )
}

function CategoryBranch({ children }: { children: ReactNode }) {
  return <div className="ml-2 border-l border-line pl-1">{children}</div>
}
