import type { ReactNode } from 'react'
import { AnimatePresence, motion, useReducedMotion } from 'motion/react'
import { useTranslation } from 'react-i18next'
import {
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  ArrowsHorizontal,
  ArrowsInLineHorizontal,
  ArrowsVertical,
  BookOpen,
  BookOpenText,
  CornersOut,
  File,
  FrameCorners,
  Scroll,
  X,
} from '@phosphor-icons/react'
import type { ReadingDirection } from '@/lib/api/types'
import {
  READER_BACKGROUNDS,
  useReaderSettings,
  type ContinuousScaleType,
  type PageTransition,
  type ReaderBackground,
  type ScaleType,
} from '@/lib/store/readerSettings'
import type { PagedReaderLayout } from '@/lib/utils/spreads'
import { IconButton } from '@/components/ui/IconButton'
import { SegmentedControl } from '@/components/ui/SegmentedControl'
import { Slider } from '@/components/ui/Slider'
import { Switch } from '@/components/ui/Switch'
import { FieldGroup, OptionTiles, Row, RowGroup, Section, SliderField } from '@/components/reader/panel'
import { cn } from '@/lib/utils/cn'

const SCALE_LABEL_KEYS: Record<ScaleType, string> = {
  SCREEN: 'scale.screen',
  WIDTH: 'scale.width',
  WIDTH_SHRINK_ONLY: 'scale.shrink',
  HEIGHT: 'scale.height',
  ORIGINAL: 'scale.original',
}

const LAYOUT_LABEL_KEYS: Record<PagedReaderLayout, string> = {
  SINGLE_PAGE: 'layout.single',
  DOUBLE_PAGES: 'layout.double',
  DOUBLE_NO_COVER: 'layout.doubleNoCover',
}

const DIRECTION_LABEL_KEYS: Record<ReadingDirection, string> = {
  LEFT_TO_RIGHT: 'common:readingDirection.leftToRight',
  RIGHT_TO_LEFT: 'common:readingDirection.rightToLeft',
  VERTICAL: 'common:readingDirection.vertical',
  WEBTOON: 'common:readingDirection.webtoon',
}

const BACKGROUND_LABEL_KEYS: Record<ReaderBackground, string> = {
  BLACK: 'background.black',
  GRAY: 'background.gray',
  WHITE: 'background.white',
}

const SCALE_ICONS: Record<ScaleType, ReactNode> = {
  SCREEN: <CornersOut />,
  WIDTH: <ArrowsHorizontal />,
  WIDTH_SHRINK_ONLY: <ArrowsInLineHorizontal />,
  HEIGHT: <ArrowsVertical />,
  ORIGINAL: <FrameCorners />,
}

const LAYOUT_ICONS: Record<PagedReaderLayout, ReactNode> = {
  SINGLE_PAGE: <File />,
  DOUBLE_PAGES: <BookOpen />,
  DOUBLE_NO_COVER: <BookOpenText />,
}

const DIRECTION_ICONS: Record<ReadingDirection, ReactNode> = {
  LEFT_TO_RIGHT: <ArrowRight />,
  RIGHT_TO_LEFT: <ArrowLeft />,
  VERTICAL: <ArrowDown />,
  WEBTOON: <Scroll />,
}

const PAGED_SCALE_VALUES: ScaleType[] = ['SCREEN', 'WIDTH', 'WIDTH_SHRINK_ONLY', 'HEIGHT', 'ORIGINAL']
const CONTINUOUS_SCALE_VALUES: ContinuousScaleType[] = ['WIDTH', 'ORIGINAL']
const LAYOUT_VALUES: PagedReaderLayout[] = ['SINGLE_PAGE', 'DOUBLE_PAGES', 'DOUBLE_NO_COVER']
const BACKGROUND_VALUES: ReaderBackground[] = ['BLACK', 'GRAY', 'WHITE']
const PAGE_TRANSITION_VALUES: PageTransition[] = ['AUTO', 'SLIDE', 'FADE']
const PAGE_TRANSITION_LABEL_KEYS: Record<PageTransition, string> = {
  AUTO: 'pageTransition.auto',
  SLIDE: 'pageTransition.slide',
  FADE: 'pageTransition.fade',
}

interface SettingsPanelProps {
  open: boolean
  direction: ReadingDirection
  onDirectionChange: (direction: ReadingDirection) => void
  onAlwaysFullscreenChange: (on: boolean) => void
  onClose: () => void
}

export function SettingsPanel({
  open,
  direction,
  onDirectionChange,
  onAlwaysFullscreenChange,
  onClose,
}: SettingsPanelProps) {
  const scale = useReaderSettings((s) => s.scale)
  const pageLayout = useReaderSettings((s) => s.pageLayout)
  const splitWide = useReaderSettings((s) => s.splitWide)
  const continuousScale = useReaderSettings((s) => s.continuousScale)
  const continuousPadding = useReaderSettings((s) => s.continuousPadding)
  const continuousMargin = useReaderSettings((s) => s.continuousMargin)
  const swipe = useReaderSettings((s) => s.swipe)
  const animations = useReaderSettings((s) => s.animations)
  const pageTransition = useReaderSettings((s) => s.pageTransition)
  const alwaysFullscreen = useReaderSettings((s) => s.alwaysFullscreen)
  const background = useReaderSettings((s) => s.background)
  const update = useReaderSettings((s) => s.update)
  const reduceMotion = useReducedMotion()
  const { t } = useTranslation('reader')

  const webtoon = direction === 'WEBTOON'

  const directionOptions = (Object.keys(DIRECTION_LABEL_KEYS) as ReadingDirection[]).map((value) => ({
    value,
    label: t(DIRECTION_LABEL_KEYS[value]),
    icon: DIRECTION_ICONS[value],
  }))
  const pagedScaleOptions = PAGED_SCALE_VALUES.map((value) => ({
    value,
    label: t(SCALE_LABEL_KEYS[value]),
    icon: SCALE_ICONS[value],
  }))
  const continuousScaleOptions = CONTINUOUS_SCALE_VALUES.map((value) => ({
    value,
    label: t(SCALE_LABEL_KEYS[value]),
    icon: SCALE_ICONS[value],
  }))
  const layoutOptions = LAYOUT_VALUES.map((value) => ({
    value,
    label: t(LAYOUT_LABEL_KEYS[value]),
    icon: LAYOUT_ICONS[value],
  }))
  const transitionOptions = PAGE_TRANSITION_VALUES.map((value) => ({
    value,
    label: t(PAGE_TRANSITION_LABEL_KEYS[value]),
  }))

  return (
    <AnimatePresence>
      {open && (
        <>
          <motion.div
            key="backdrop"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: reduceMotion ? 0 : 0.2 }}
            className="fixed inset-0 z-30 bg-black/40"
            onClick={onClose}
          />
          <motion.aside
            key="panel"
            role="dialog"
            aria-label={t('panel.title')}
            initial={{ x: '100%' }}
            animate={{ x: 0 }}
            exit={{ x: '100%' }}
            transition={{ duration: reduceMotion ? 0 : 0.25, ease: [0.16, 1, 0.3, 1] }}
            className="theme-dark fixed top-0 right-0 z-30 flex h-full w-80 max-w-[85vw] safe-top safe-bottom flex-col border-l border-line bg-bg/90 text-ink backdrop-blur-xl"
          >
            <div className="flex shrink-0 items-center justify-between border-b border-line px-4 py-3">
              <h2 className="text-[15px] font-semibold">{t('panel.title')}</h2>
              <IconButton
                label={t('panel.close')}
                className="text-ink-2 hover:bg-overlay hover:text-ink"
                onClick={onClose}
              >
                <X className="size-4" />
              </IconButton>
            </div>

            <div className="min-h-0 flex-1 space-y-5 overflow-y-auto px-3 py-4">
              <Section title={t('panel.direction')}>
                <OptionTiles options={directionOptions} value={direction} onChange={onDirectionChange} perRow={4} />
              </Section>

              <Section title={t('panel.scale')}>
                {webtoon ? (
                  <OptionTiles
                    options={continuousScaleOptions}
                    value={continuousScale}
                    onChange={(v) => update({ continuousScale: v })}
                    perRow={2}
                  />
                ) : (
                  <OptionTiles
                    options={pagedScaleOptions}
                    value={scale}
                    onChange={(v) => update({ scale: v })}
                    perRow={3}
                  />
                )}
              </Section>

              {!webtoon && (
                <Section title={t('panel.layout')}>
                  <OptionTiles
                    options={layoutOptions}
                    value={pageLayout}
                    onChange={(v) => update({ pageLayout: v })}
                    perRow={3}
                  />
                  <RowGroup>
                    <Row label={t('panel.splitWide')}>
                      <Switch
                        checked={splitWide}
                        onCheckedChange={(v) => update({ splitWide: v })}
                        label={t('panel.splitWide')}
                      />
                    </Row>
                  </RowGroup>
                </Section>
              )}

              <Section title={t('panel.behavior')}>
                <RowGroup>
                  <Row label={t('panel.background')}>
                    <div className="flex gap-2.5">
                      {BACKGROUND_VALUES.map((value) => (
                        <button
                          key={value}
                          type="button"
                          aria-label={t(BACKGROUND_LABEL_KEYS[value])}
                          aria-pressed={background === value}
                          title={t(BACKGROUND_LABEL_KEYS[value])}
                          onClick={() => update({ background: value })}
                          className={cn(
                            'size-6 cursor-pointer rounded-full border border-line-strong',
                            background === value && 'ring-2 ring-accent ring-offset-2 ring-offset-raised',
                          )}
                          style={{ background: READER_BACKGROUNDS[value] }}
                        />
                      ))}
                    </div>
                  </Row>
                  <Row label={t('panel.swipe')}>
                    <Switch checked={swipe} onCheckedChange={(v) => update({ swipe: v })} label={t('panel.swipe')} />
                  </Row>
                  <Row label={t('panel.animations')}>
                    <Switch
                      checked={animations}
                      onCheckedChange={(v) => update({ animations: v })}
                      label={t('panel.animations')}
                    />
                  </Row>
                  {animations && !webtoon && (
                    <Row label={t('panel.pageTransition')}>
                      <SegmentedControl
                        options={transitionOptions}
                        value={pageTransition}
                        onChange={(v) => update({ pageTransition: v })}
                        size="sm"
                      />
                    </Row>
                  )}
                  <Row label={t('panel.alwaysFullscreen')}>
                    <Switch
                      checked={alwaysFullscreen}
                      onCheckedChange={onAlwaysFullscreenChange}
                      label={t('panel.alwaysFullscreen')}
                    />
                  </Row>
                </RowGroup>
              </Section>

              {webtoon && (
                <Section title={t('common:readingDirection.webtoon')}>
                  <FieldGroup>
                    <SliderField label={t('panel.sidePadding')} value={`${continuousPadding}%`}>
                      <Slider
                        value={continuousPadding}
                        onValueChange={(v) => update({ continuousPadding: v })}
                        min={0}
                        max={40}
                        step={5}
                        label={t('panel.sidePadding')}
                      />
                    </SliderField>
                    <SliderField label={t('panel.pageGap')} value={`${continuousMargin}px`}>
                      <Slider
                        value={continuousMargin}
                        onValueChange={(v) => update({ continuousMargin: v })}
                        min={0}
                        max={15}
                        step={5}
                        label={t('panel.pageGap')}
                      />
                    </SliderField>
                  </FieldGroup>
                </Section>
              )}
            </div>
          </motion.aside>
        </>
      )}
    </AnimatePresence>
  )
}
