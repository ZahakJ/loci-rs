import { t } from '@core/i18n/i18n';
import type { Flag } from '@core/types';
import { Icon } from '../../design/icons';
import { Button } from '../../design/primitives/Button';
import { Dialog } from '../../design/primitives/Dialog';
import { Kbd } from '../../design/primitives/Kbd';
import { cx } from '../../design/primitives/layer';
import s from './Study.module.css';

export interface FlagDialogProps {
  open: boolean;
  current: number;
  showKeys: boolean;
  onClose: () => void;
  onPick: (flag: Flag) => void;
}

const FLAGS: Flag[] = [1, 2, 3, 4, 5, 6, 7];

/** Flag colour picker (the overflow menu's Flag submenu). ctrl+1..7 work without it. */
export function FlagDialog({ open, current, showKeys, onClose, onPick }: FlagDialogProps) {
  return (
    <Dialog
      open={open}
      onClose={onClose}
      title={t('studyScreen.flagTitle')}
      width="24rem"
      actions={<Button variant="ghost" onClick={onClose}>{t('common.cancel')}</Button>}
    >
      <div className={s.flagList} role="group" aria-label={t('study.flag')}>
        {FLAGS.map((f) => (
          <button
            key={f}
            type="button"
            aria-pressed={current === f}
            className={cx(s.flagOption, current === f && s.flagOn)}
            data-flag={f}
            data-autofocus={current === f || (current === 0 && f === 1) ? true : undefined}
            onClick={() => onPick(current === f ? 0 : f)}
          >
            <span className={s.flagSwatch} aria-hidden><Icon name="flag" size={18} /></span>
            <span className={s.flagName}>{t(`study.flags.${f}`)}</span>
            {current === f && <Icon name="check" size={16} className={s.flagCheck} />}
            {showKeys && <Kbd>{`ctrl+${f}`}</Kbd>}
          </button>
        ))}
        {current !== 0 && (
          <button type="button" className={s.flagOption} onClick={() => onPick(0)}>
            <span className={s.flagSwatch} aria-hidden><Icon name="close" size={18} /></span>
            <span className={s.flagName}>{t('studyScreen.removeFlag')}</span>
            {showKeys && <Kbd>ctrl+0</Kbd>}
          </button>
        )}
      </div>
    </Dialog>
  );
}
