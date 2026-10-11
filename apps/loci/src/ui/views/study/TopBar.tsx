import { t } from '@core/i18n/i18n';
import type { StudyCounts } from '@core/types';
import { Icon } from '../../design/icons';
import { IconButton } from '../../design/primitives/Button';
import { Menu, type MenuEntry } from '../../design/primitives/Menu';
import { Pill } from '../../design/primitives/Pill';
import { cx } from '../../design/primitives/layer';
import { formatClock, splitDeckName } from './format';
import s from './Study.module.css';

export type QueueKind = 'new' | 'learn' | 'review';

export interface TopBarProps {
  deckName: string;
  counts: StudyCounts | null;
  /** which count the current card belongs to (Anki underlines it) */
  currentKind: QueueKind | null;
  progress: number;
  flag: number;
  marked: boolean;
  /** elapsed ms when the timer is on, else null */
  elapsed: number | null;
  menu: MenuEntry[];
  onLeave: () => void;
  /** a preview session: nothing is recorded (shows a small pill) */
  preview?: boolean;
}

/** One count pill whose number ticks when it changes. */
function CountPill({ tone, value, label, current }: { tone: 'new' | 'learn' | 'due'; value: number; label: string; current: boolean }) {
  return (
    <span className={cx(s.countWrap, current && s.countCurrent)} title={label}>
      {value === 0
        ? <Pill tone={tone} value={0} label={label} />
        : (
          <Pill tone={tone} label={label}>
            <span key={value} className={s.tick}>{value.toLocaleString()}</span>
          </Pill>
        )}
    </span>
  );
}

export function TopBar({ deckName, counts, currentKind, progress, flag, marked, elapsed, menu, onLeave, preview }: TopBarProps) {
  const { parents, leaf } = splitDeckName(deckName);
  return (
    <header className={s.topbar}>
      <div
        className={s.progress}
        role="progressbar"
        aria-label={t('studyScreen.progress')}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(progress * 100)}
      >
        <div className={s.progressFill} style={{ transform: `scaleX(${progress})` }} />
      </div>
      <div className={s.topInner}>
        <IconButton icon="chevron-left" label={t('study.leave')} onClick={onLeave} className={s.leave} />
        <div className={s.deck} title={deckName}>
          {parents.length > 0 && <span className={s.deckParents}>{parents.join(' / ')}<span aria-hidden className={s.deckSep}>/</span></span>}
          <span className={s.deckLeaf}>{leaf || t('studyScreen.allDecks')}</span>
          {flag > 0 && (
            <span className={s.flagMark} data-flag={flag} title={t(`study.flags.${flag}`)}>
              <Icon name="flag" size={16} label={t('studyScreen.flaggedAs', { colour: t(`study.flags.${flag}`) })} />
            </span>
          )}
          {marked && (
            <span className={s.markMark} title={t('studyScreen.markedNote')}>
              <Icon name="star" size={16} label={t('studyScreen.markedNote')} />
            </span>
          )}
        </div>
        <div className={s.topEnd}>
          {preview && (
            <span className={s.previewPill} title={t('studyScreen.previewPillTitle')} data-preview-pill>
              <Icon name="info" size={14} />
              <span className={s.previewLong}>{t('studyScreen.previewPill')}</span>
              <span className={s.previewShort} aria-hidden>{t('studyScreen.previewPillShort')}</span>
            </span>
          )}
          {elapsed !== null && (
            <span className={s.timer} role="timer" aria-label={t('study.timer')}>
              <Icon name="clock" size={16} />
              <span className="tabular">{formatClock(elapsed)}</span>
            </span>
          )}
          {counts && (
            <div className={s.counts} aria-label={t('study.counts', { new: counts.new, learn: counts.learn, review: counts.review })} role="group">
              <CountPill tone="new" value={counts.new} label={t('studyScreen.countNew', { count: counts.new })} current={currentKind === 'new'} />
              <CountPill tone="learn" value={counts.learn} label={t('studyScreen.countLearn', { count: counts.learn })} current={currentKind === 'learn'} />
              <CountPill tone="due" value={counts.review} label={t('studyScreen.countDue', { count: counts.review })} current={currentKind === 'review'} />
            </div>
          )}
          <Menu
            label={t('common.moreActions')}
            items={menu}
            trigger={(p) => <IconButton {...p} icon="more" label={t('common.moreActions')} />}
          />
        </div>
      </div>
    </header>
  );
}
