import { useEffect, useRef, useState } from 'react';
import { t } from '@core/i18n/i18n';
import { Button } from '../../design/primitives/Button';
import { Field } from '../../design/primitives/Field';
import { Input } from '../../design/primitives/Input';
import { Sheet } from '../../design/primitives/Sheet';
import { isDueSyntax } from './format';
import s from './Study.module.css';

export interface SetDueSheetProps {
  open: boolean;
  busy?: boolean;
  onClose: () => void;
  /** Anki's set-due-date syntax: '0' today, '1!' tomorrow and reset the interval, '3-7' a random day in range */
  onSubmit: (days: string) => void;
}

/** Ctrl+Shift+D in study: set the current card's due date (Anki's syntax). */
export function SetDueSheet({ open, busy, onClose, onSubmit }: SetDueSheetProps) {
  const [days, setDays] = useState('');
  const [touched, setTouched] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => { if (open) { setDays(''); setTouched(false); } }, [open]);
  const valid = isDueSyntax(days);
  const submit = () => { setTouched(true); if (valid && !busy) onSubmit(days.trim()); };
  return (
    <Sheet
      open={open}
      onClose={onClose}
      title={t('studyScreen.setDue.title')}
      width="26rem"
      phone="bottom"
      initialFocus={input}
      footer={(
        <>
          <Button variant="ghost" onClick={onClose}>{t('common.cancel')}</Button>
          <Button variant="primary" icon="calendar" onClick={submit} disabled={!valid} loading={busy}>{t('studyScreen.setDue.action')}</Button>
        </>
      )}
    >
      <form className={s.custom} onSubmit={(e) => { e.preventDefault(); submit(); }}>
        <Field label={t('studyScreen.setDue.label')} help={t('studyScreen.setDue.help')} error={touched && days.trim() && !valid ? t('studyScreen.setDue.invalid') : undefined}>
          <Input ref={input} value={days} onChange={(e) => setDays(e.target.value)} placeholder="1, 1!, 3-7" mono inputMode="text" autoComplete="off" spellCheck={false} autoCapitalize="off" />
        </Field>
      </form>
    </Sheet>
  );
}
