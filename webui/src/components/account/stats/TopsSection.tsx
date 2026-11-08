import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router-dom'
import { statsApi } from '@/lib/api/stats'
import type { NamedValueDto, SeriesValueDto } from '@/lib/api/types'
import { Skeleton } from '@/components/ui/Skeleton'
import { cn } from '@/lib/utils/cn'
import { Section } from '../Section'
import { SectionState } from './SectionState'

interface TopItem {
  key: string
  name: string
  value: number
  label: string
  to?: string
}

function TopColumn({ title, items }: { title: string; items: TopItem[] }) {
  const max = Math.max(...items.map((i) => i.value))
  const rowClass = 'relative flex items-center justify-between gap-3 rounded-md px-2 py-1.5'
  return (
    <div>
      <h3 className="mb-2 text-[11px] font-medium tracking-[0.08em] text-ink-3 uppercase">{title}</h3>
      <div className="flex flex-col gap-0.5">
        {items.map((item) => {
          const content = (
            <>
              <div
                className="absolute inset-y-0 left-0 rounded-md bg-accent-soft"
                style={{ width: `${(item.value / max) * 100}%` }}
              />
              <span
                className={cn(
                  'relative min-w-0 truncate text-[13px] text-ink-2',
                  item.to && 'transition-colors group-hover:text-accent-strong',
                )}
              >
                {item.name}
              </span>
              <span className="relative shrink-0 text-xs tabular-nums text-ink-3">{item.label}</span>
            </>
          )
          return item.to ? (
            <Link key={item.key} to={item.to} className={cn('group', rowClass)}>
              {content}
            </Link>
          ) : (
            <div key={item.key} className={rowClass}>
              {content}
            </div>
          )
        })}
      </div>
    </div>
  )
}


export function TopsSection({ libraryId }: { libraryId?: string }) {
  const { t } = useTranslation('stats')
  const query = useQuery({
    queryKey: ['stats', 'tops', libraryId ?? 'all'],
    queryFn: () => statsApi.tops(libraryId),
  })

  return (
    <Section title={t('tops.title')}>
      <SectionState query={query} skeleton={<Skeleton className="h-40 w-full" />}>
        {(tops) => {
          const named = (items: NamedValueDto[]): TopItem[] =>
            items.map((i) => ({ key: i.name, name: i.name, value: i.value, label: t('tops.series', { count: i.value }) }))
          const series = (items: SeriesValueDto[]): TopItem[] =>
            items.map((i) => ({
              key: i.seriesId,
              name: i.name,
              value: i.value,
              label: t('tops.booksOfTotal', { read: i.value, count: i.total }),
              to: `/series/${i.seriesId}`,
            }))
          const columns = [
            { title: t('tops.authors'), items: named(tops.topAuthors) },
            { title: t('tops.genres'), items: named(tops.topGenres) },
            { title: t('tops.tags'), items: named(tops.topTags) },
          ].filter((c) => c.items.length > 0)
          if (tops.topSeries.length === 0 && columns.length === 0)
            return <p className="text-sm text-ink-3">{t('tops.empty')}</p>
          return (
            <div className="space-y-6">
              {/* series titles run long, so the series ranking gets the full width */}
              {tops.topSeries.length > 0 && (
                <TopColumn title={t('tops.seriesHeading')} items={series(tops.topSeries)} />
              )}
              {columns.length > 0 && (
                <div className="grid gap-6 sm:grid-cols-3">
                  {columns.map((c) => (
                    <TopColumn key={c.title} title={c.title} items={c.items} />
                  ))}
                </div>
              )}
            </div>
          )
        }}
      </SectionState>
    </Section>
  )
}
