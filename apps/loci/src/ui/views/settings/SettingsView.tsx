import { useEffect, useMemo, useState } from 'react';
import { t } from '@core/i18n/i18n';
import type { CollectionSettings, DeckConfigPreset, DeckTreeNode } from '@core/types';
import { api, errorMessage, useQuery } from '../../api';
import { Icon } from '../../design/icons';
import { Card } from '../../design/primitives/Card';
import { Segmented } from '../../design/primitives/Segmented';
import { Select } from '../../design/primitives/Select';
import { Skeleton } from '../../design/primitives/Skeleton';
import { Slider } from '../../design/primitives/Slider';
import { Switch } from '../../design/primitives/Switch';
import { Input } from '../../design/primitives/Input';
import { toast } from '../../design/primitives/Toast';
import { useTouchOnly } from '../../design/primitives/pointer';
import { formatCount, formatHour, formatPercent } from '../../hooks/format';
import { decksUsing, primaryPreset } from '../../hooks/presets';
import { Page } from '../../shell/Page';
import { useUi } from '../../store/ui';
import type { ThemePref } from '../../store/theme';
import { DataSection } from './DataSection';
import { SyncSection } from './SyncSection';
import { PresetList } from './PresetList';
import { LEARNING_PRESETS, detectLearningPreset, type LearningPreset } from './presetFields';
import { RetentionForecast } from './RetentionForecast';
import { AboutSection } from './AboutSection';
import { SavedNote, Section, SettingRow, SideIndex } from './SettingsParts';
import s from './Settings.module.css';

export { decksUsing };

export default function SettingsView({ section }: { section?: string }) {
  const settings = useUi((st) => st.settings);
  const setSettingsStore = useUi((st) => st.setSettings);
  const themePref = useUi((st) => st.themePref);
  const setThemePref = useUi((st) => st.setThemePref);
  const presets = useQuery((a) => a.listPresets(), [], ['presets']);
  const tree = useQuery((a) => a.deckTree(), [], ['decks', 'presets']);
  // null until the user picks one: the editor follows the preset most cards use (not blindly id 1)
  const [picked, setPicked] = useState<number | null>(null);
  const presetId = picked ?? primaryPreset(tree.data, presets.data)?.id ?? null;
  const touchOnly = useTouchOnly();
  const [savedAt, setSavedAt] = useState({ appearance: 0, study: 0 });
  const [advancedOpen, setAdvancedOpen] = useState(section === 'advanced' || section === 'presets');

  useEffect(() => {
    if (!section) return;
    const id = window.setTimeout(() => document.getElementById(`settings-${section}`)?.scrollIntoView({ block: 'start' }), 200);
    return () => window.clearTimeout(id);
  }, [section]);

  const patchSettings = async (patch: Partial<CollectionSettings>) => {
    if (!settings) return;
    setSettingsStore({ ...settings, ...patch });
    try {
      const next = await api.setSettings(patch);
      setSettingsStore(next);
      setSavedAt((v) => ({ ...v, appearance: Date.now() }));
    } catch (e) {
      toast({ message: t('common.saveFailed'), tone: 'danger' });
      console.error(e);
    }
  };

  const onTheme = (pref: ThemePref) => { setThemePref(pref); void patchSettings({ theme: pref }); };

  const items = useMemo(() => [
    { id: 'appearance', label: t('settings.appearance') },
    { id: 'study', label: t('settings.study') },
    { id: 'advanced', label: t('settings.advanced') },
    { id: 'sync', label: t('settings.sync') },
    { id: 'data', label: t('settings.data') },
    { id: 'about', label: t('settings.about') },
  ], []);

  return (
    <Page title={t('settings.title')} subtitle={t('settings.subtitle')} width="wide">
      <div className={s.layout}>
        <SideIndex items={items} label={t('settings.sections')} hrefFor={(id) => `#/settings?section=${id}`} />
        <div className={s.sections}>
          <Section id="appearance" title={t('settings.appearance')} status={<SavedNote at={savedAt.appearance} />}>
            <Card padding="none" className={s.rows}>
              <SettingRow label={t('settings.theme')} help={t('settings.themeHelp')}>
                <Segmented
                  label={t('settings.theme')}
                  value={themePref}
                  onChange={onTheme}
                  options={[
                    { value: 'system', label: t('theme.system'), icon: 'system' },
                    { value: 'light', label: t('theme.light'), icon: 'sun' },
                    { value: 'dark', label: t('theme.dark'), icon: 'moon' },
                  ]}
                />
              </SettingRow>
              {settings ? (
                <>
                  <SettingRow label={t('settings.cardFont')} help={t('settings.cardFontHelp')} stack>
                    <div className={s.wideControl}>
                      <Segmented
                        label={t('settings.cardFont')}
                        value={settings.cardFont}
                        onChange={(cardFont) => void patchSettings({ cardFont })}
                        options={[
                          { value: 'serif', label: t('settings.font.serif') },
                          { value: 'sans', label: t('settings.font.sans') },
                          { value: 'system', label: t('settings.font.system') },
                        ]}
                      />
                      <div className={s.sample} aria-label={t('settings.sample')}>
                        <small aria-hidden>{t('settings.sample')}</small>
                        {t('settings.sampleText')}
                        <span className={s.sampleArabic} dir="rtl" lang="ar">الذاكرة قصرٌ من غرف</span>
                      </div>
                    </div>
                  </SettingRow>
                  <SettingRow label={t('settings.cardSize')} help={t('settings.cardSizeHelp')} stack>
                    <Slider
                      label={t('settings.cardSize')}
                      min={16} max={32} step={1}
                      value={settings.cardFontSize || 22}
                      format={(v) => `${v}px`}
                      onChange={(cardFontSize) => setSettingsStore({ ...settings, cardFontSize })}
                      onCommit={(cardFontSize) => void patchSettings({ cardFontSize })}
                      className={s.wideControl}
                    />
                  </SettingRow>
                  <SettingRow label={t('settings.cardAlign')} help={t('settings.cardAlignHelp')}>
                    <Segmented
                      label={t('settings.cardAlign')}
                      value={settings.cardAlign}
                      onChange={(cardAlign) => void patchSettings({ cardAlign })}
                      options={[{ value: 'start', label: t('settings.align.start') }, { value: 'center', label: t('settings.align.center') }]}
                    />
                  </SettingRow>
                  <SettingRow label={t('settings.kbdHints')} help={touchOnly ? t('settings.kbdHintsTouch') : t('settings.kbdHintsHelp')} inline>
                    <Switch label={t('settings.kbdHints')} checked={settings.showKeyboardHints} onChange={(showKeyboardHints) => void patchSettings({ showKeyboardHints })} />
                  </SettingRow>
                  <SettingRow label={t('settings.motion')} help={t('settings.motionHelp')}>
                    <Segmented
                      label={t('settings.motion')}
                      value={settings.reducedMotion}
                      onChange={(reducedMotion) => void patchSettings({ reducedMotion })}
                      options={[
                        { value: 'system', label: t('settings.motion.system') },
                        { value: 'on', label: t('settings.motion.reduce') },
                        { value: 'off', label: t('settings.motion.full') },
                      ]}
                    />
                  </SettingRow>
                </>
              ) : <div className={s.row}><Skeleton lines={4} /></div>}
            </Card>
          </Section>

          <StudySection
            presets={presets.data}
            presetId={presetId}
            setPresetId={setPicked}
            tree={tree.data}
            settings={settings}
            patchSettings={patchSettings}
            onPresetSaved={(p) => { presets.setData((list) => (list ?? []).map((x) => (x.id === p.id ? p : x))); setSavedAt((v) => ({ ...v, study: Date.now() })); }}
            savedAt={savedAt.study}
          />

          <Section id="advanced" title={t('settings.advanced')}>
            <button type="button" className={s.disclosure} aria-expanded={advancedOpen} aria-controls="advanced-body" onClick={() => setAdvancedOpen((v) => !v)}>
              <span className={s.disclosureText}>
                <span className={s.disclosureTitle}>{t('settings.advancedTitle')}</span>
                <span className={s.help}>{t('settings.advancedHelp')}</span>
              </span>
              <Icon name="chevron-down" />
            </button>
            {advancedOpen && (
              <div id="advanced-body" className={s.disclosureBody}>
                <PresetList presets={presets.data} tree={tree.data} onChanged={() => void presets.refetch()} />
                <div className={s.subhead}>{t('settings.more')}</div>
                <a href="#/settings/notetypes" className={s.linkRow}>
                  <Icon name="layers" />
                  <span className={s.text}><span className={s.label}>{t('settings.notetypes')}</span><span className={s.help}>{t('settings.notetypesHelp')}</span></span>
                  <Icon name="chevron-right" size={18} />
                </a>
              </div>
            )}
          </Section>

          <SyncSection />
          <DataSection />
          <AboutSection />
        </div>
      </div>
    </Page>
  );
}

function StudySection({ presets, presetId, setPresetId, tree, settings, patchSettings, onPresetSaved, savedAt }: {
  presets: DeckConfigPreset[] | undefined; presetId: number | null; setPresetId: (id: number) => void; tree: DeckTreeNode | undefined;
  settings: CollectionSettings | null; patchSettings: (p: Partial<CollectionSettings>) => Promise<void>;
  onPresetSaved: (p: DeckConfigPreset) => void; savedAt: number;
}) {
  const preset = presets?.find((p) => p.id === presetId) ?? (presetId === null ? undefined : presets?.[0]);
  const [draft, setDraft] = useState(preset?.config);
  useEffect(() => { setDraft(preset?.config); }, [preset]);

  const save = async (patch: Partial<DeckConfigPreset['config']>) => {
    if (!preset || !draft) return;
    const config = { ...draft, ...patch };
    setDraft(config);
    try {
      const saved = await api.savePreset({ id: preset.id, name: preset.name, config });
      onPresetSaved(saved);
    } catch (e) {
      toast({ message: t('common.saveFailedDetail', { error: errorMessage(e) }), tone: 'danger' });
    }
  };

  const using = preset ? decksUsing(tree, preset.id) : [];
  const learning = draft ? detectLearningPreset(draft) : null;
  const shortList = (names: string[]) => names.map((n) => n.split('::').pop()).slice(0, 4).join(', ') + (names.length > 4 ? ` +${formatCount(names.length - 4)}` : '');

  return (
    <Section id="study" title={t('settings.study')} lede={t('settings.studyLede')} status={<SavedNote at={savedAt} />}>
      <Card padding="none" className={s.rows}>
        {presets && preset && (
          <div className={s.presetPicker}>
            {presets.length > 1 && (
              <>
                <span id="preset-picker-label">{t('settings.editingPreset')}</span>
                <Select aria-labelledby="preset-picker-label" value={preset.id} onChange={setPresetId} options={presets.map((p) => ({ value: p.id, label: p.name }))} />
              </>
            )}
            <span className={s.presetDecks} data-preset-decks>
              {using.length ? t('settings.presetDecks', { name: preset.name, list: shortList(using) }) : t('settings.presetDecksNone', { name: preset.name })}
            </span>
          </div>
        )}
        {!draft ? <div className={s.row}><Skeleton lines={5} /></div> : (
          <>
            <SettingRow label={t('settings.learningPreset')} help={learning ? t(`settings.learning.${learning}Help`) : t('settings.learning.customHelp')}>
              <Segmented<LearningPreset>
                label={t('settings.learningPreset')}
                value={learning}
                onChange={(k) => void save({ ...LEARNING_PRESETS[k] })}
                options={[
                  { value: 'relaxed', label: t('settings.learning.relaxed') },
                  { value: 'standard', label: t('settings.learning.standard') },
                  { value: 'intense', label: t('settings.learning.intense') },
                ]}
              />
            </SettingRow>
            <SettingRow label={t('settings.budget')} help={t('settings.budgetHelp')} stack>
              <Slider
                label={t('settings.budget')} min={5} max={120} step={5}
                value={draft.minutesPerDay}
                format={(v) => t('time.short', { m: v })}
                showScale
                onChange={(minutesPerDay) => setDraft({ ...draft, minutesPerDay })}
                onCommit={(minutesPerDay) => void save({ minutesPerDay })}
              />
            </SettingRow>
            <SettingRow label={t('settings.newPerDay')} help={t('settings.newPerDayHelp')} stack>
              <Slider
                label={t('settings.newPerDay')} min={0} max={100} step={1}
                value={Math.min(100, draft.newPerDay)}
                format={(v) => t('settings.newPerDayValue', { n: v })}
                showScale
                onChange={(n) => setDraft({ ...draft, newPerDay: n })}
                onCommit={(n) => void save({ newPerDay: n })}
              />
            </SettingRow>
            <SettingRow label={t('settings.retention')} help={t('settings.retentionHelp')} stack>
              <div className={s.retention}>
                <Slider
                  label={t('settings.retention')} min={0.7} max={0.99} step={0.01}
                  value={draft.desiredRetention}
                  format={(v) => formatPercent(v)}
                  showScale
                  onChange={(desiredRetention) => setDraft({ ...draft, desiredRetention })}
                  onCommit={(desiredRetention) => void save({ desiredRetention })}
                />
                <RetentionForecast config={draft} presetKey={preset?.id} />
              </div>
            </SettingRow>
          </>
        )}
        {settings && (
          <>
            <SettingRow label={t('settings.dayStarts')} help={t('settings.dayStartsHelp')}>
              <Select
                className={s.selectControl}
                aria-label={t('settings.dayStarts')}
                value={settings.rolloverHour}
                onChange={(rolloverHour) => void patchSettings({ rolloverHour })}
                options={Array.from({ length: 24 }, (_, h) => ({ value: h, label: formatHour(h) }))}
              />
            </SettingRow>
            <SettingRow label={t('settings.learnAhead')} help={t('settings.learnAheadHelp')}>
              <Input
                key={settings.learnAheadMinutes}
                type="number" min={0} max={1440} suffix={t('settings.learnAheadUnit')} className={s.narrowInput}
                aria-label={t('settings.learnAhead')}
                defaultValue={settings.learnAheadMinutes}
                onBlur={(e) => { const v = Math.max(0, Math.min(1440, Math.round(Number(e.target.value)) || 0)); if (v !== settings.learnAheadMinutes) void patchSettings({ learnAheadMinutes: v }); }}
                onKeyDown={(e) => { if (e.key === 'Enter') (e.target as HTMLInputElement).blur(); }}
              />
            </SettingRow>
            <SettingRow label={t('settings.typedCase')} help={t('settings.typedCaseHelp')} inline>
              <Switch label={t('settings.typedCase')} checked={settings.typedAnswerIgnoreCase} onChange={(v) => void patchSettings({ typedAnswerIgnoreCase: v })} />
            </SettingRow>
            <SettingRow label={t('settings.typedDiacritics')} help={t('settings.typedDiacriticsHelp')} inline>
              <Switch label={t('settings.typedDiacritics')} checked={settings.typedAnswerIgnoreDiacritics} onChange={(v) => void patchSettings({ typedAnswerIgnoreDiacritics: v })} />
            </SettingRow>
          </>
        )}
      </Card>
      {preset && (
        <p className={s.help}>
          {t('settings.moreOptions')} <a href={`#/settings/presets/${preset.id}`}>{t('settings.openPreset', { name: preset.name })}</a>
        </p>
      )}
    </Section>
  );
}
