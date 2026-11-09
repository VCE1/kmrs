import type { ReactNode } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { SegmentedControl } from '@/components/ui/SegmentedControl'

const TABS = ['series', 'books'] as const
type Tab = (typeof TABS)[number]

/** komga-style navigation between a library's browse views. */
export function BrowseTabs({ tab, extra }: { tab: Tab; extra?: ReactNode }) {
  const { t } = useTranslation('browse')
  const { libraryId } = useParams()
  const navigate = useNavigate()

  return (
    <div className="mb-5 flex flex-wrap items-center justify-between gap-3 overflow-x-auto pb-1">
      <SegmentedControl<Tab>
        options={TABS.map((value) => ({ value, label: t(`tabs.${value}`) }))}
        value={tab}
        // a tab switch re-views the same library, it isn't a navigation step: replace, or
        // history.back replays tab states instead of leaving the library
        onChange={(next) => navigate(`/libraries/${libraryId}/${next}`, { replace: true })}
      />
      {extra && <div className="flex items-center gap-2">{extra}</div>}
    </div>
  )
}
