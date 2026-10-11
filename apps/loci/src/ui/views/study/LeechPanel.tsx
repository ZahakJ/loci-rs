import { useEffect, useId, useRef, useState } from 'react';
import { t } from '@core/i18n/i18n';
import { Button } from '../../design/primitives/Button';
import { Field } from '../../design/primitives/Field';
import { Textarea } from '../../design/primitives/Input';
import s from './Study.module.css';

export interface LeechPanelProps {
  lapses: number;
  busy: boolean;
  onRewrite: () => void;
  onHint: (hint: string) => void;
  onReset: () => void;
  onSuspend: () => void;
  onKeep: () => void;
}

/** "This card keeps slipping": a calm inline sheet after a card becomes a leech (DESIGN §7, §8). */
export function LeechPanel({ lapses, busy, onRewrite, onHint, onReset, onSuspend, onKeep }: LeechPanelProps) {
  const [hinting, setHinting] = useState(false);
  const [hint, setHint] = useState('');
  const titleRef = useRef<HTMLHeadingElement>(null);
  const hintRef = useRef<HTMLTextAreaElement>(null);
  const hintId = useId();
  useEffect(() => { titleRef.current?.focus({ preventScroll: true }); }, []);
  useEffect(() => { if (hinting) hintRef.current?.focus(); }, [hinting]);
  return (
    <section className={s.leech} aria-labelledby="leech-title" data-leech>
      <h2 id="leech-title" ref={titleRef} tabIndex={-1} className={s.leechTitle}>{t('studyScreen.leech.title')}</h2>
      <p className={s.leechBody}>{t('studyScreen.leech.body', { count: lapses })}</p>
      {!hinting ? (
        <div className={s.leechActions}>
          <Button variant="primary" icon="edit" onClick={onRewrite} disabled={busy}>{t('studyScreen.leech.rewrite')}</Button>
          <Button variant="secondary" icon="text" onClick={() => setHinting(true)} disabled={busy}>{t('studyScreen.leech.hint')}</Button>
          <Button variant="secondary" icon="undo" onClick={onReset} disabled={busy}>{t('studyScreen.leech.reset')}</Button>
          <Button variant="secondary" icon="suspend" onClick={onSuspend} disabled={busy}>{t('studyScreen.leech.suspend')}</Button>
          <Button variant="ghost" onClick={onKeep} disabled={busy} data-autofocus>{t('studyScreen.leech.keep')}</Button>
        </div>
      ) : (
        <form
          className={s.hintForm}
          onSubmit={(e) => { e.preventDefault(); if (hint.trim()) onHint(hint.trim()); }}
        >
          <Field label={t('studyScreen.leech.hintLabel')} help={t('studyScreen.leech.hintHelp')} id={hintId}>
            <Textarea
              ref={hintRef}
              rows={2}
              dir="auto"
              value={hint}
              onChange={(e) => setHint(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) { e.preventDefault(); if (hint.trim()) onHint(hint.trim()); }
                if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); setHinting(false); }
              }}
              placeholder={t('studyScreen.leech.hintPlaceholder')}
            />
          </Field>
          <div className={s.leechActions}>
            <Button type="submit" variant="primary" disabled={busy || !hint.trim()} loading={busy}>{t('studyScreen.leech.hintSave')}</Button>
            <Button variant="ghost" onClick={() => setHinting(false)} disabled={busy}>{t('common.cancel')}</Button>
          </div>
        </form>
      )}
    </section>
  );
}
