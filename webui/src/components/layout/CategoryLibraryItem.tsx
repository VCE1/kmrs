import { useState, type ReactNode } from 'react'
import { NavLink, useLocation, useSearchParams } from 'react-router-dom'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { BookBookmark, CaretRight, FolderSimple } from '@phosphor-icons/react'
import { librariesApi } from '@/lib/api/libraries'
import type { DirectoryDto, LibraryDto } from '@/lib/api/types'
import { cn } from '@/lib/utils/cn'
import { useChanged } from '@/lib/hooks/useChanged'

/** A scanned file is not a category. Folder names such as 政治 are. */
const BOOK_FILE = /\.(?:pdf|epub|mobi|azw3?|cbz|cbr|zip|rar)$/i

/** Libraries whose sidebar entry opens category folders instead of a flat series list. */
export function libraryUsesCategories(name: string) {
  return name.includes('编程随想')
}

function categoryDirectories(directories: DirectoryDto[]) {
  return directories.filter((dir) => !BOOK_FILE.test(dir.name))
}

/** Height animates while the subtree stays mounted, so closing does not snap. */
function CategoryBranch({ open, children }: { open: boolean; children: ReactNode }) {
  const [seen, setSeen] = useState(open)
  if (open && !seen) setSeen(true)

  return (
    <div
      className={cn(
        'grid transition-[grid-template-rows,opacity] duration-200 ease-out',
        open ? 'grid-rows-[1fr] opacity-100' : 'pointer-events-none grid-rows-[0fr] opacity-0',
      )}
    >
      <div className="overflow-hidden" inert={open ? undefined : true}>
        {seen ? children : null}
      </div>
    </div>
  )
}

export function CategoryLibraryItem({
  library,
  onNavigate,
  actions,
}: {
  library: LibraryDto
  onNavigate?: () => void
  actions?: ReactNode
}) {
  const { pathname } = useLocation()
  const [params] = useSearchParams()
  const inside = pathname.startsWith(`/libraries/${library.id}`)
  const currentPath = inside ? (params.get('path') ?? '') : ''
  const [open, setOpen] = useState(inside)

  if (useChanged([inside]) && inside) setOpen(true)

  return (
    <div>
      <div className="group relative">
        <button
          type="button"
          aria-expanded={open}
          title={library.name}
          onClick={() => setOpen((value) => !value)}
          className={cn(
            'flex min-h-10 w-full cursor-pointer items-center gap-3 rounded-lg px-3 py-2 text-left text-[13.5px] transition-colors duration-150 pointer-coarse:min-h-12',
            inside ? 'font-medium text-ink' : 'text-ink-2 hover:bg-raised hover:text-ink',
            actions && 'pr-9',
          )}
        >
          <BookBookmark className="size-[18px] shrink-0" />
          <span className="min-w-0 flex-1 truncate pointer-coarse:line-clamp-2 pointer-coarse:whitespace-normal">{library.name}</span>
          <CaretRight
            className={cn('size-3.5 shrink-0 text-ink-3 transition-transform duration-200', open && 'rotate-90')}
          />
        </button>
        {actions ? <div className="absolute inset-y-0 right-2 flex items-center">{actions}</div> : null}
      </div>
      <CategoryBranch open={open}>
        <div className="mt-0.5 ml-3 flex flex-col gap-0.5 border-l border-line pl-1 pointer-fine:ml-[21px] pointer-fine:pl-1.5">
          <CategoryLevel libraryId={library.id} parentPath="" currentPath={currentPath} onNavigate={onNavigate} />
        </div>
      </CategoryBranch>
    </div>
  )
}

function CategoryLevel({
  libraryId,
  parentPath,
  currentPath,
  onNavigate,
}: {
  libraryId: string
  parentPath: string
  currentPath: string
  onNavigate?: () => void
}) {
  const { t } = useTranslation('layout')
  const query = useQuery({
    queryKey: ['libraries', libraryId, 'browse', parentPath],
    queryFn: () => librariesApi.browse(libraryId, parentPath || undefined),
  })
  if (query.isLoading) {
    return <div className="mx-2 my-1 h-6 animate-pulse rounded-md bg-surface" />
  }
  if (query.isError) {
    return (
      <button
        type="button"
        onClick={() => query.refetch()}
        className="px-2 py-1 text-left text-[12.5px] text-ink-3 hover:text-ink"
      >
        {t('common:retry', '重试')}
      </button>
    )
  }
  const directories = categoryDirectories(query.data?.directories ?? [])
  if (directories.length === 0) {
    return <p className="px-2 py-1 text-[12.5px] text-ink-3">{t('nav.noCategories')}</p>
  }
  return (
    <>
      {directories.map((dir) => (
        <CategoryRow key={dir.path} libraryId={libraryId} dir={dir} currentPath={currentPath} onNavigate={onNavigate} />
      ))}
    </>
  )
}

function CategoryRow({
  libraryId,
  dir,
  currentPath,
  onNavigate,
}: {
  libraryId: string
  dir: DirectoryDto
  currentPath: string
  onNavigate?: () => void
}) {
  const exact = currentPath === dir.path
  const childOpen = currentPath === dir.path || currentPath.startsWith(`${dir.path}/`)
  const [manual, setManual] = useState<boolean | null>(null)
  const open = manual ?? childOpen
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
            onClick={() => setManual(!open)}
            className="flex w-8 shrink-0 cursor-pointer items-center justify-center rounded-md text-ink-3 hover:bg-raised hover:text-ink pointer-coarse:w-11"
          >
            <CaretRight className={cn('size-3.5 transition-transform duration-200', open && 'rotate-90')} />
          </button>
        ) : (
          <span className="w-8 shrink-0 pointer-coarse:w-11" />
        )}
        <NavLink
          to={`/libraries/${libraryId}/series?path=${encodeURIComponent(dir.path)}`}
          title={dir.name}
          onClick={onNavigate}
          className={cn(
            'flex min-h-10 min-w-0 flex-1 items-center gap-2 rounded-lg px-1.5 py-1.5 text-[13px] transition-colors duration-150 pointer-coarse:min-h-11',
            exact ? 'bg-accent-soft font-medium text-accent-strong' : 'text-ink-2 hover:bg-raised hover:text-ink',
          )}
        >
          <FolderSimple className="size-4 shrink-0" />
          <span className="min-w-0 flex-1 truncate pointer-coarse:line-clamp-2 pointer-coarse:whitespace-normal">{dir.name}</span>
          <span className="shrink-0 text-[11px] text-ink-3 tabular-nums">{dir.bookCount}</span>
        </NavLink>
      </div>
      {canNest ? (
        <CategoryBranch open={open}>
          <div className="ml-2 border-l border-line pl-1 pointer-fine:ml-3">
            <CategoryLevel
              libraryId={libraryId}
              parentPath={dir.path}
              currentPath={currentPath}
              onNavigate={onNavigate}
            />
          </div>
        </CategoryBranch>
      ) : null}
    </div>
  )
}
