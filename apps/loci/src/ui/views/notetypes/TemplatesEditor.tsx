/**
 * Card templates of a note type: pick a card type, edit front / back / styling in a mono editor,
 * insert fields, see problems from `api.checkTemplate` inline (debounced) and a preview rendered
 * with the first note of this type (or a sample). Card types can be added, renamed and removed.
 */
import { useEffect, useRef, useState } from 'react';
import { t } from '@core/i18n/i18n';
import { NotetypeKind, type Notetype, type NotetypeConfig, type Template } from '@core/types';
import { api } from '@ui/api';
import { Icon } from '@ui/design/icons';
import { Button } from '@ui/design/primitives/Button';
import { Dialog } from '@ui/design/primitives/Dialog';
import { Field as FormField } from '@ui/design/primitives/Field';
import { Input } from '@ui/design/primitives/Input';
import { Menu } from '@ui/design/primitives/Menu';
import { Switch } from '@ui/design/primitives/Switch';
import { Tabs } from '@ui/design/primitives/Tabs';
import { cx } from '@ui/design/primitives/layer';
import { CardPreview } from '../edit/CardPreview';
import { uniqueName } from './templateText';
import s from './Notetypes.module.css';

export interface TemplatesEditorProps {
  draft: Notetype;
  saved: Notetype;
  dirty: boolean;
  onChange: (nt: Notetype) => void;
  onAnnounce: (msg: string) => void;
  /** saves the draft (with the usual confirmation) so the preview can show it */
  onSaveAndPreview?: () => void;
  /** bumps after every successful save; forces the preview to re-render */
  savedVersion?: number;
  saving?: boolean;
}

/** `config.useOwnStyling` (E-15): off = Loci's typography and theme over the note type's CSS. */
type StylingConfig = NotetypeConfig & { useOwnStyling?: boolean };

type Part = 'qfmt' | 'afmt' | 'css';

export function TemplatesEditor({ draft, saved, dirty, onChange, onAnnounce, onSaveAndPreview, savedVersion = 0, saving }: TemplatesEditorProps) {
  const [index, setIndex] = useState(0);
  const [part, setPart] = useState<Part>('qfmt');
  const [issues, setIssues] = useState<string[]>([]);
  const [sample, setSample] = useState<string[] | null>(null);
  const [dialog, setDialog] = useState<'rename' | 'remove' | null>(null);
  const [name, setName] = useState('');
  const area = useRef<HTMLTextAreaElement>(null);
  const tpl = draft.templates[Math.min(index, draft.templates.length - 1)]!;
  const cloze = draft.kind === NotetypeKind.Cloze;

  // problems for the template being edited
  useEffect(() => {
    let live = true;
    const timer = window.setTimeout(() => {
      api.checkTemplate(draft.id, tpl.qfmt, tpl.afmt).then((r) => live && setIssues(r)).catch(() => live && setIssues([]));
    }, 350);
    return () => { live = false; window.clearTimeout(timer); };
  }, [draft.id, tpl.qfmt, tpl.afmt]);

  // preview content: the first note of this type, else field names as sample text
  useEffect(() => {
    let live = true;
    const fallback = saved.fields.map((f, i) => (cloze && i === 0 ? `${f.name}: {{c1::${t('notetypes.sampleCloze')}}}` : `(${f.name})`));
    api.searchIds(`note:"${saved.name.replace(/"/g, '\\"')}"`, 'notes')
      .then(async (ids) => {
        const first = ids[0];
        const n = first !== undefined ? await api.getNote(first) : null;
        if (live) setSample(n ? n.fields : fallback);
      })
      .catch(() => live && setSample(fallback));
    return () => { live = false; };
  }, [saved.id, saved.name, saved.fields, cloze]);

  const setTemplate = (patch: Partial<Template>) => onChange({ ...draft, templates: draft.templates.map((x, k) => (k === index ? { ...x, ...patch } : x)) });
  const value = part === 'css' ? draft.css : tpl[part];
  const setValue = (v: string) => (part === 'css' ? onChange({ ...draft, css: v }) : setTemplate({ [part]: v }));

  const insert = (text: string) => {
    const ta = area.current;
    if (!ta) { setValue(value + text); return; }
    const a = ta.selectionStart, b = ta.selectionEnd;
    const next = value.slice(0, a) + text + value.slice(b);
    setValue(next);
    requestAnimationFrame(() => { ta.focus(); ta.setSelectionRange(a + text.length, a + text.length); });
  };

  const addTemplate = () => {
    const nm = uniqueName(t('notetypes.cardN', { n: draft.templates.length + 1 }), draft.templates.map((x) => x.name));
    const ord = Math.max(-1, ...draft.templates.map((x) => x.ord)) + 1;
    const first = draft.fields[0]?.name ?? 'Front', second = draft.fields[1]?.name ?? first;
    const created: Template = { ord, name: nm, qfmt: `{{${second}}}`, afmt: `{{FrontSide}}\n\n<hr id=answer>\n\n{{${first}}}`, bqfmt: '', bafmt: '', config: {} };
    onChange({ ...draft, templates: [...draft.templates, created] });
    setIndex(draft.templates.length);
    onAnnounce(t('notetypes.templateAdded', { name: nm }));
  };

  const fieldItems = [
    ...draft.fields.map((f) => ({ id: `f-${f.ord}`, label: f.name, onSelect: () => insert(cloze && part !== 'css' && f === draft.fields[0] ? `{{cloze:${f.name}}}` : `{{${f.name}}}`) })),
    { separator: true as const, id: 'sp' },
    { id: 'frontside', label: 'FrontSide', onSelect: () => insert('{{FrontSide}}') },
    { id: 'tags', label: 'Tags', onSelect: () => insert('{{Tags}}') },
    { id: 'deck', label: 'Deck', onSelect: () => insert('{{Deck}}') },
    { id: 'type', label: t('notetypes.typeAnswer'), onSelect: () => insert(`{{type:${draft.fields[1]?.name ?? draft.fields[0]?.name ?? ''}}}`) },
  ];

  const previewNt = draft;
  const ownStyling = !!(draft.config as StylingConfig).useOwnStyling;
  const setOwnStyling = (v: boolean) => onChange({ ...draft, config: { ...draft.config, useOwnStyling: v } as StylingConfig });
  return (
    <div className={s.templates}>
      <div className={s.templateBar}>
        {draft.templates.length > 1 ? (
          <div className={s.chips} role="group" aria-label={t('notetypes.cardTypes')}>
            {draft.templates.map((x, k) => (
              <button key={x.ord} type="button" className={s.chip} aria-pressed={k === index} onClick={() => setIndex(k)}>{x.name}</button>
            ))}
          </div>
        ) : <p className={s.templateName}>{tpl.name}</p>}
        {!cloze && (
          <div className={s.templateActions}>
            <Button size="sm" variant="ghost" icon="plus" onClick={addTemplate}>{t('editor.templates.addTemplate')}</Button>
            <Button size="sm" variant="ghost" icon="edit" onClick={() => { setName(tpl.name); setDialog('rename'); }}>{t('notetypes.rename')}</Button>
            <Button size="sm" variant="ghost" icon="trash" disabled={draft.templates.length <= 1} onClick={() => setDialog('remove')}>{t('editor.templates.removeTemplate')}</Button>
          </div>
        )}
      </div>

      <div className={s.templateGrid}>
        <div className={s.codePane}>
          <div className={s.codeHead}>
            <Tabs
              label={t('editor.templates.title')}
              value={part}
              onChange={setPart}
              items={[{ value: 'qfmt', label: t('editor.templates.front') }, { value: 'afmt', label: t('editor.templates.back') }, { value: 'css', label: t('editor.templates.styling') }]}
              className={s.codeTabs}
            />
            {part !== 'css' && (
              <Menu label={t('editor.templates.insertField')} items={fieldItems} trigger={(p) => <Button {...p} size="sm" variant="ghost" icon="plus" iconEnd="chevron-down">{t('notetypes.field')}</Button>} />
            )}
          </div>
          {part === 'css' && (
            <div className={s.ownStyling}>
              <FormField label={t('notetypes.ownStyling')} help={t('notetypes.ownStylingHelp')} inline>
                <Switch checked={ownStyling} onChange={setOwnStyling} />
              </FormField>
            </div>
          )}
          <textarea
            ref={area}
            className={cx(s.code, issues.length > 0 && part !== 'css' && s.codeInvalid)}
            value={value}
            onChange={(e) => setValue(e.target.value)}
            spellCheck={false}
            autoCapitalize="off"
            autoComplete="off"
            aria-label={`${tpl.name}: ${part === 'qfmt' ? t('editor.templates.front') : part === 'afmt' ? t('editor.templates.back') : t('editor.templates.styling')}`}
            aria-describedby="template-issues"
            dir="ltr"
          />
          <div id="template-issues" className={s.issues} aria-live="polite">
            {issues.length > 0 ? (
              <>
                <p className={s.issuesTitle}><Icon name="warning" size={16} />{t('editor.templates.issues', { count: issues.length })}</p>
                <ul className={s.issueList}>{issues.map((x) => <li key={x}>{x}</li>)}</ul>
              </>
            ) : (
              <p className={s.ok}><Icon name="check" size={16} />{t('notetypes.noProblems')}</p>
            )}
          </div>
        </div>
        <div className={s.previewPane}>
          {sample ? (
            <CardPreview notetype={previewNt} fields={sample} ord={cloze ? 0 : tpl.ord} title={t('editor.preview')} version={`${savedVersion}|${saved.mtime}|${tpl.qfmt}|${tpl.afmt}|${draft.css}`} />
          ) : null}
          {dirty && (
            <div className={s.previewNote}>
              <Icon name="info" size={14} />
              <span>{t('notetypes.previewSaved')}</span>
              {onSaveAndPreview && (
                <Button size="sm" variant="secondary" icon="eye" loading={saving} onClick={onSaveAndPreview} className={s.previewSave}>{t('notetypes.saveAndPreview')}</Button>
              )}
            </div>
          )}
        </div>
      </div>

      <Dialog open={dialog === 'rename'} onClose={() => setDialog(null)} title={t('notetypes.renameTemplate')} confirmLabel={t('notetypes.rename')}
        confirmDisabled={!name.trim() || draft.templates.some((x, k) => k !== index && x.name.toLowerCase() === name.trim().toLowerCase())}
        onConfirm={() => { setTemplate({ name: name.trim() }); setDialog(null); }}>
        <FormField label={t('notetypes.name')}><Input value={name} onChange={(e) => setName(e.target.value)} data-autofocus /></FormField>
      </Dialog>
      <Dialog open={dialog === 'remove'} onClose={() => setDialog(null)} destructive title={t('notetypes.removeTemplateTitle', { name: tpl.name })}
        description={t('notetypes.removeTemplateBody')} confirmLabel={t('editor.templates.removeTemplate')}
        onConfirm={() => { onChange({ ...draft, templates: draft.templates.filter((_, k) => k !== index) }); setIndex(0); setDialog(null); onAnnounce(t('notetypes.templateRemoved', { name: tpl.name })); }} />
    </div>
  );
}
