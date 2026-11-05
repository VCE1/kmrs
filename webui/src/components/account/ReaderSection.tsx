import { CircleHalf } from '@phosphor-icons/react'
import { useTranslation } from 'react-i18next'
import {
  READER_BACKGROUNDS,
  useReaderSettings,
  type ReaderBackground,
  type ScaleType,
} from '@/lib/store/readerSettings'
import type { ReadingDirection } from '@/lib/api/types'
import type { PagedReaderLayout } from '@/lib/utils/spreads'
import { readingDirectionLabel } from '@/lib/utils/format'
import { cn } from '@/lib/utils/cn'
import { SegmentedControl } from '@/components/ui/SegmentedControl'
import { Switch } from '@/components/ui/Switch'
import { Section, SettingRow } from './Section'

const directions: ReadingDirection[] = ['LEFT_TO_RIGHT', 'RIGHT_TO_LEFT', 'VERTICAL', 'WEBTOON']
const backgrounds: ReaderBackground[] = ['AUTO', 'BLACK', 'GRAY', 'WHITE']

export function ReaderSection() {
  const { t } = useTranslation('account')
  const readingDirection = useReaderSettings((s) => s.readingDirection)
  const scale = useReaderSettings((s) => s.scale)
  const pageLayout = useReaderSettings((s) => s.pageLayout)
  const splitWide = useReaderSettings((s) => s.splitWide)
  const background = useReaderSettings((s) => s.background)
  const update = useReaderSettings((s) => s.update)

  return (
    <Section title={t('reader.defaultsTitle')}>
      <div className="flex flex-col divide-y divide-line">
        <SettingRow label={t('reader.direction')}>
          <SegmentedControl<ReadingDirection>
            options={directions.map((d) => ({ value: d, label: readingDirectionLabel(d) }))}
            value={readingDirection}
            onChange={(readingDirection) => update({ readingDirection })}
          />
        </SettingRow>
        <SettingRow label={t('reader.scale')}>
          <div className="max-w-full overflow-x-auto">
            <SegmentedControl<ScaleType>
              options={[
                { value: 'SCREEN', label: t('reader.scaleScreen') },
                { value: 'WIDTH', label: t('reader.scaleWidth') },
                { value: 'WIDTH_SHRINK_ONLY', label: t('reader.scaleShrinkWidth') },
                { value: 'HEIGHT', label: t('reader.scaleHeight') },
                { value: 'ORIGINAL', label: t('reader.scaleOriginal') },
              ]}
              value={scale}
              onChange={(scale) => update({ scale })}
            />
          </div>
        </SettingRow>
        <SettingRow label={t('reader.pageLayout')}>
          <SegmentedControl<PagedReaderLayout>
            options={[
              { value: 'SINGLE_PAGE', label: t('reader.layoutSingle') },
              { value: 'DOUBLE_PAGES', label: t('reader.layoutDouble') },
              { value: 'DOUBLE_NO_COVER', label: t('reader.layoutDoubleNoCover') },
            ]}
            value={pageLayout}
            onChange={(pageLayout) => update({ pageLayout })}
          />
        </SettingRow>
        <SettingRow label={t('reader.splitWide')}>
          <Switch checked={splitWide} onCheckedChange={(splitWide) => update({ splitWide })} label={t('reader.splitWide')} />
        </SettingRow>
        <SettingRow label={t('reader.background')}>
          <div className="flex items-center gap-2.5">
            {backgrounds.map((bg) => (
              <button
                key={bg}
                type="button"
                title={t(`reader.backgrounds.${bg.toLowerCase()}`)}
                aria-label={t('reader.backgroundLabel', { color: t(`reader.backgrounds.${bg.toLowerCase()}`) })}
                aria-pressed={background === bg}
                onClick={() => update({ background: bg })}
                style={bg === 'AUTO' ? undefined : { background: READER_BACKGROUNDS[bg] }}
                className={cn(
                  'size-7 cursor-pointer rounded-full text-ink-2 transition-transform active:scale-95',
                  bg !== 'AUTO' && 'border border-line-strong',
                  background === bg && 'ring-2 ring-accent ring-offset-2 ring-offset-surface',
                )}
              >
                {bg === 'AUTO' && <CircleHalf aria-hidden weight="fill" className="size-full" />}
              </button>
            ))}
          </div>
        </SettingRow>
      </div>
    </Section>
  )
}
