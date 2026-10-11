import { useId, useRef, useState, type DragEvent, type KeyboardEvent } from 'react';
import { t } from '@core/i18n/i18n';
import { Icon } from '../../design/icons';
import { Button } from '../../design/primitives/Button';
import { cx } from '../../design/primitives/layer';
import { ACCEPTED_EXTENSIONS, isPackageName } from '../../store/importFile';
import s from './DropZone.module.css';

export interface DropZoneProps {
  onFile: (file: File) => void;
  title?: string;
  hint?: string;
  compact?: boolean;
  className?: string;
  autoFocus?: boolean;
}

/** Drop target + file picker for .apkg/.colpkg. Keyboard: the button opens the picker. */
export function DropZone({ onFile, title, hint, compact, className, autoFocus }: DropZoneProps) {
  const [over, setOver] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const id = useId();

  const accept = (file: File | undefined) => {
    if (!file) return;
    if (!isPackageName(file.name)) { setError(t('import.wrongType', { name: file.name })); return; }
    setError(null);
    onFile(file);
  };
  const onDrop = (e: DragEvent) => { e.preventDefault(); setOver(false); accept(e.dataTransfer.files[0]); };
  const onKey = (e: KeyboardEvent) => { if (e.key === 'Enter' && e.target === e.currentTarget) input.current?.click(); };

  return (
    <div
      className={cx(s.zone, compact && s.compact, className)}
      data-over={over}
      onDragEnter={(e) => { e.preventDefault(); setOver(true); }}
      onDragOver={(e) => { e.preventDefault(); e.dataTransfer.dropEffect = 'copy'; }}
      onDragLeave={(e) => { if (!e.currentTarget.contains(e.relatedTarget as Node)) setOver(false); }}
      onDrop={onDrop}
      onClick={(e) => { if (!(e.target as HTMLElement).closest('button')) input.current?.click(); }}
      onKeyDown={onKey}
    >
      <span className={s.icon} aria-hidden><Icon name="import" size={24} /></span>
      <div className={s.title} id={`${id}-title`}>{title ?? t('import.dropTitle')}</div>
      <div className={s.hint}>{hint ?? t('import.dropHint')}</div>
      <Button variant="secondary" onClick={() => input.current?.click()} aria-describedby={`${id}-title`} autoFocus={autoFocus}>{t('import.choose')}</Button>
      <input
        ref={input}
        className={s.input}
        type="file"
        accept={ACCEPTED_EXTENSIONS.join(',')}
        tabIndex={-1}
        aria-hidden
        onChange={(e) => { accept(e.target.files?.[0]); e.target.value = ''; }}
      />
      {error && <div className={s.error} role="alert"><Icon name="warning" size={16} />{error}</div>}
    </div>
  );
}
