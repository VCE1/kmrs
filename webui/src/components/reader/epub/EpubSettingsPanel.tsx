import { AnimatePresence, motion, useReducedMotion } from 'motion/react'
import { useTranslation } from 'react-i18next'
import { useQuery } from '@tanstack/react-query'
import { ArrowCounterClockwise, BookOpen, Scroll, X } from '@phosphor-icons/react'
import { useReaderSettings, type EpubTheme } from '@/lib/store/readerSettings'
import { fontsApi } from '@/lib/api/fonts'
import { IconButton } from '@/components/ui/IconButton'
import { Slider } from '@/components/ui/Slider'
import { Switch } from '@/components/ui/Switch'
import { Chip, FieldGroup, OptionTiles, Row, RowGroup, Section, SliderField } from '@/components/reader/panel'
import { isServerFont, serverFontValue } from '@/components/reader/epub/fonts'
import { LocalFontPicker } from '@/components/reader/epub/LocalFontPicker'

const THEME_VALUES: EpubTheme[] = ['DAY', 'SEPIA', 'NIGHT']
const THEME_LABEL_KEYS: Record<EpubTheme, string> = {
  DAY: 'epub.themeDay',
  SEPIA: 'epub.themeSepia',
  NIGHT: 'epub.themeNight',
}
const THEME_SWATCH: Record<EpubTheme, { bg: string; fg: string }> = {
  DAY: { bg: '#fefefe', fg: '#121212' },
  SEPIA: { bg: '#e9ddc8', fg: '#000000' },
  NIGHT: { bg: '#000000', fg: '#fefefe' },
}

interface EpubSettingsPanelProps {
  open: boolean
  onAlwaysFullscreenChange: (on: boolean) => void
  onClose: () => void
}

export function EpubSettingsPanel({ open, onAlwaysFullscreenChange, onClose }: EpubSettingsPanelProps) {
  const theme = useReaderSettings((s) => s.epubTheme)
  const scroll = useReaderSettings((s) => s.epubScroll)
  const fontFamily = useReaderSettings((s) => s.epubFontFamily)
  const fontSize = useReaderSettings((s) => s.epubFontSize)
  const lineHeight = useReaderSettings((s) => s.epubLineHeight)
  const alwaysFullscreen = useReaderSettings((s) => s.alwaysFullscreen)
  const update = useReaderSettings((s) => s.update)
  const reduceMotion = useReducedMotion()
  const { t } = useTranslation('reader')
  const serverFontsQuery = useQuery({ queryKey: ['fonts', 'families'], queryFn: fontsApi.families, staleTime: Infinity })
  const serverFonts = serverFontsQuery.data ?? []

  const themeOptions = THEME_VALUES.map((value) => ({
    value,
    label: t(THEME_LABEL_KEYS[value]),
    icon: (
      <span
        aria-hidden
        className="flex size-6 items-center justify-center rounded-full border border-line-strong text-[13px] font-medium"
        style={{ background: THEME_SWATCH[value].bg, color: THEME_SWATCH[value].fg }}
      >
        A
      </span>
    ),
  }))
  const layoutOptions = [
    { value: 'paginated', label: t('epub.layoutPaginated'), icon: <BookOpen /> },
    { value: 'scroll', label: t('epub.layoutScroll'), icon: <Scroll /> },
  ]

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
              <Section title={t('epub.theme')}>
                <OptionTiles
                  options={themeOptions}
                  value={theme}
                  onChange={(epubTheme) => update({ epubTheme })}
                  perRow={3}
                />
              </Section>

              <Section title={t('epub.layout')}>
                <OptionTiles
                  options={layoutOptions}
                  value={scroll ? 'scroll' : 'paginated'}
                  onChange={(v) => update({ epubScroll: v === 'scroll' })}
                  perRow={2}
                />
              </Section>

              <Section title={t('epub.typography')}>
                <FieldGroup>
                  <div className="space-y-2">
                    <div className="text-[13px] text-ink">{t('epub.fontFamily')}</div>
                    <div className="flex flex-wrap gap-1.5">
                      <Chip selected={fontFamily === 'Original'} onClick={() => update({ epubFontFamily: 'Original' })}>
                        {t('epub.fontPublisher')}
                      </Chip>
                      <LocalFontPicker
                        value={fontFamily}
                        onSelect={(epubFontFamily) => update({ epubFontFamily })}
                      />
                      {serverFonts.map((family) => (
                        <Chip
                          key={family}
                          selected={isServerFont(fontFamily) && fontFamily === serverFontValue(family)}
                          onClick={() => update({ epubFontFamily: serverFontValue(family) })}
                        >
                          {family}
                        </Chip>
                      ))}
                    </div>
                  </div>
                  <SliderField
                    label={t('epub.fontSize')}
                    value={`${Math.round(fontSize * 100)}%`}
                    action={
                      fontSize !== 1 && (
                        <IconButton
                          label={t('epub.resetDefault')}
                          className="size-6 text-ink-3 hover:bg-overlay hover:text-ink"
                          onClick={() => update({ epubFontSize: 1 })}
                        >
                          <ArrowCounterClockwise className="size-3.5" />
                        </IconButton>
                      )
                    }
                  >
                    <Slider
                      value={fontSize}
                      onValueChange={(v) => update({ epubFontSize: v })}
                      min={0.7}
                      max={2}
                      step={0.05}
                      label={t('epub.fontSize')}
                    />
                  </SliderField>
                  <SliderField
                    label={t('epub.lineHeight')}
                    value={lineHeight === null ? t('epub.default') : lineHeight.toFixed(1)}
                    action={
                      lineHeight !== null && (
                        <IconButton
                          label={t('epub.resetDefault')}
                          className="size-6 text-ink-3 hover:bg-overlay hover:text-ink"
                          onClick={() => update({ epubLineHeight: null })}
                        >
                          <ArrowCounterClockwise className="size-3.5" />
                        </IconButton>
                      )
                    }
                  >
                    <Slider
                      value={lineHeight ?? 1.5}
                      onValueChange={(v) => update({ epubLineHeight: v })}
                      min={1}
                      max={2.5}
                      step={0.1}
                      label={t('epub.lineHeight')}
                    />
                  </SliderField>
                </FieldGroup>
              </Section>

              <Section title={t('panel.behavior')}>
                <RowGroup>
                  <Row label={t('panel.alwaysFullscreen')}>
                    <Switch
                      checked={alwaysFullscreen}
                      onCheckedChange={onAlwaysFullscreenChange}
                      label={t('panel.alwaysFullscreen')}
                    />
                  </Row>
                </RowGroup>
              </Section>
            </div>
          </motion.aside>
        </>
      )}
    </AnimatePresence>
  )
}
