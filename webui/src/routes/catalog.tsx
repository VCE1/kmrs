import { useState } from 'react'
import { Link, useParams, useSearchParams } from 'react-router-dom'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { FolderSimple } from '@phosphor-icons/react'
import { catalogApi, type CatalogNode } from '@/lib/api/catalog'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { EmptyState } from '@/components/ui/EmptyState'
import { GridSkeleton } from '@/components/ui/Skeleton'

function categoryHref(node: CatalogNode) {
  if (node.directory) {
    return `/catalog/${node.slug}?directory=${encodeURIComponent(node.directory)}`
  }
  return `/catalog/${node.slug}`
}

export function CatalogPage() {
  const { slug } = useParams()
  const [params] = useSearchParams()
  const directory = params.get('directory') ?? ''
  const { t } = useTranslation('layout')
  useDocumentTitle(t('nav.catalog'))
  if (slug) return <CatalogShelf key={`${slug}:${directory}`} slug={slug} directory={directory} />
  return <CatalogRoots />
}

function CatalogRoots() {
  const { t } = useTranslation('layout')
  const query = useQuery({ queryKey: ['catalog'], queryFn: () => catalogApi.list() })
  if (query.isLoading) return <GridSkeleton />
  if (query.isError || !query.data) return <EmptyState title={t('catalog.loadFailed')} />
  return (
    <div className="flex flex-col gap-4">
      <h1 className="text-xl font-semibold text-ink">{t('nav.catalog')}</h1>
      <p className="max-w-3xl text-sm text-ink-3">{t('catalog.intro')}</p>
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-3">
        {query.data.map((node) => (
          <CategoryCard key={node.slug} node={node} />
        ))}
      </div>
    </div>
  )
}

function CatalogShelf({ slug, directory }: { slug: string; directory: string }) {
  const { t } = useTranslation('layout')
  const [page, setPage] = useState(0)
  const query = useQuery({
    queryKey: ['catalog', slug, directory, page],
    queryFn: () => catalogApi.get(slug, page, 48, directory),
  })
  if (query.isLoading) return <GridSkeleton />
  if (query.isError || !query.data) return <EmptyState title={t('catalog.loadFailed')} />
  const { category, ancestors, content, totalElements } = query.data
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center gap-2 text-sm text-ink-3">
        <Link to="/catalog" className="hover:text-ink">
          {t('nav.catalog')}
        </Link>
        {ancestors.map((item) => (
          <span key={`${item.slug}:${item.directory}`} className="flex items-center gap-2">
            <span>/</span>
            <Link to={categoryHref(item)} className="hover:text-ink">
              {item.name}
            </Link>
          </span>
        ))}
      </div>
      <h1 className="text-xl font-semibold text-ink">{category.name}</h1>
      {category.note ? <p className="max-w-3xl text-sm text-ink-3">{category.note}</p> : null}
      {category.children.length > 0 && (
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-3">
          {category.children.map((node) => (
            <CategoryCard key={node.directory || node.slug} node={node} />
          ))}
        </div>
      )}
      {totalElements > 0 ? (
        <h2 className="text-sm font-medium text-ink-2">
          {t('catalog.seriesCount', { count: totalElements })}
        </h2>
      ) : null}
      {content.length === 0 && category.children.length === 0 ? (
        <EmptyState title={t('catalog.emptySeries')} />
      ) : content.length === 0 ? null : (
        <ul className="flex flex-col divide-y divide-line/60 rounded-lg border border-line/60">
          {content.map((series) => (
            <li key={series.id}>
              <Link
                to={`/series/${series.id}`}
                className="flex items-baseline justify-between gap-3 px-3 py-2 hover:bg-raised"
              >
                <span className="text-ink">{series.name}</span>
                <span className="shrink-0 text-xs text-ink-3">
                  {series.libraryName} · {series.bookCount}
                </span>
              </Link>
            </li>
          ))}
        </ul>
      )}
      <div className="flex items-center gap-2">
        <button
          type="button"
          className="rounded-md border border-line px-3 py-1 text-sm disabled:opacity-40"
          disabled={page === 0}
          onClick={() => setPage((current) => Math.max(0, current - 1))}
        >
          {t('catalog.prev')}
        </button>
        <button
          type="button"
          className="rounded-md border border-line px-3 py-1 text-sm disabled:opacity-40"
          disabled={query.data.last}
          onClick={() => setPage((current) => current + 1)}
        >
          {t('catalog.next')}
        </button>
      </div>
    </div>
  )
}

function CategoryCard({ node }: { node: CatalogNode }) {
  const { t } = useTranslation('layout')
  return (
    <Link
      to={categoryHref(node)}
      className="flex flex-col gap-2 rounded-xl border border-line/60 bg-surface p-4 hover:border-accent/40"
    >
      <div className="flex items-center gap-2 text-ink">
        <FolderSimple size={22} weight="duotone" />
        <span className="font-medium">{node.name}</span>
      </div>
      <p className="line-clamp-3 text-sm text-ink-3">{node.note}</p>
      <p className="text-xs text-ink-3">{t('catalog.seriesCount', { count: node.seriesCount })}</p>
    </Link>
  )
}
