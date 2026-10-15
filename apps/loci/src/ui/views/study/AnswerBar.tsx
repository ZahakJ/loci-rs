import { t } from '@core/i18n/i18n';
import type { Rating, SchedulingPreview } from '@core/types';
import { Icon } from '../../design/icons';
import { Kbd } from '../../design/primitives/Kbd';
import { useBottomBar } from '../../design/primitives/bottomBar';
import { cx } from '../../design/primitives/layer';
import { useRef } from 'react';
import { GRADE_KEYS, RATINGS, gradeLabel, gradeName } from './format';
import s from './Study.module.css';
import { Tooltip } from '../../design/primitives/Tooltip';

export type BarMode =
  | { kind: 'front'; typing?: boolean }
  | { kind: 'reveal-next'; revealed: number; total: number }
  | { kind: 'back'; preview: SchedulingPreview | null; showIntervals: boolean }
  | { kind: 'idle' };

export interface AnswerBarProps {
  mode: BarMode;
  /** keycaps only on fine pointers (internals §6) */
  showKeys: boolean;
  /** keycaps rendered but faded out (after the first cards of a session; see keyHints) */
  keysFaded?: boolean;
  /** the mouse entered or left the bar (brings faded keycaps back) */
  onHoverChange?: (hover: boolean) => void;
  busy?: boolean;
  onShow: () => void;
  onRevealNext: () => void;
  onGrade: (r: Rating) => void;
  /** first-sessions explainer line (null = hidden) */
  explainer: { onDismiss: () => void } | null;
}

/** Bottom bar: always the same height so nothing moves between question and answer. */
export function AnswerBar({ mode, showKeys, keysFaded, onHoverChange, busy, onShow, onRevealNext, onGrade, explainer }: AnswerBarProps) {
  const bar = useRef<HTMLDivElement>(null);
  useBottomBar(bar); // toasts sit above this bar, never over the grade buttons
  return (
    <div
      ref={bar}
      className={s.bar}
      data-mode={mode.kind}
      data-keys={showKeys && keysFaded ? 'faded' : undefined}
      onPointerEnter={(e) => { if (e.pointerType === 'mouse') onHoverChange?.(true); }}
      onPointerLeave={(e) => { if (e.pointerType === 'mouse') onHoverChange?.(false); }}
    >
      <div className={s.barInner}>
        {mode.kind === 'front' && (
          // While the typed-answer box has focus, Space types a space and Enter checks the answer.
          <button type="button" className={s.showButton} onClick={onShow} disabled={busy} data-bar-primary onMouseDown={(e) => { if (mode.typing) e.preventDefault(); }}>
            <span>{mode.typing ? t('studyScreen.check') : t('study.showAnswer')}</span>
            {showKeys && <Kbd className={s.barKbd}>{mode.typing ? 'enter' : 'space'}</Kbd>}
          </button>
        )}
        {mode.kind === 'reveal-next' && (
          <button type="button" className={s.showButton} onClick={onRevealNext} disabled={busy} data-bar-primary>
            <span>{t('study.revealNext')}</span>
            <span className={s.revealCount}>{t('studyScreen.revealCount', { revealed: mode.revealed, total: mode.total })}</span>
            {showKeys && <Kbd className={s.barKbd}>space</Kbd>}
          </button>
        )}
        {mode.kind === 'back' && (
          <div className={s.grades} role="group" aria-label={t('studyScreen.gradeGroup')}>
            {RATINGS.map((r) => {
              const interval = mode.showIntervals ? gradeLabel(mode.preview, r) : '';
              return (
                <button
                  key={r}
                  type="button"
                  className={cx(s.grade, s[GRADE_KEYS[r]])}
                  onClick={() => onGrade(r)}
                  disabled={busy}
                  aria-keyshortcuts={r === 3 ? '3 Space' : String(r)}
                  aria-label={interval ? t('studyScreen.gradeAria', { grade: gradeName(r), interval }) : gradeName(r)}
                >
                  <span className={s.gradeName}>
                    {gradeName(r)}
                    {showKeys && <Kbd className={s.gradeKbd}>{String(r)}</Kbd>}
                  </span>
                  <span className={cx(s.gradeInterval, 'num')} aria-hidden>{interval || ' '}</span>
                </button>
              );
            })}
          </div>
        )}
        {mode.kind === 'idle' && <div className={s.barSpacer} aria-hidden />}
        {explainer && (
          <p className={s.explainer}>
            <span className={s.explainerText}>{t('studyScreen.explainer')}</span>
            <Tooltip content={t('studyScreen.explainerDismissAria')}>
              <button type="button" className={s.explainerDismiss} onClick={explainer.onDismiss} aria-label={t('studyScreen.explainerDismissAria')}>
                <Icon name="close" size={14} />
              </button>
            </Tooltip>
          </p>
        )}
      </div>
    </div>
  );
}
