import { useEffect, useState } from 'react';
import { t } from '@core/i18n/i18n';
import { cx } from '../design/primitives/layer';
import { useUi } from '../store/ui';
import s from './BusyIndicator.module.css';

/** Thin top progress line + a small chip for long operations, driven by 'progress' events. */
export function BusyIndicator() {
  const busy = useUi((st) => st.busy);
  const ops = Object.values(busy).sort((a, b) => b.at - a.at);
  const top = ops[0];
  const [visible, setVisible] = useState(false);
  // only show for operations that last a moment (no flicker for quick ones)
  useEffect(() => {
    if (!top) { setVisible(false); return; }
    const id = window.setTimeout(() => setVisible(true), 350);
    return () => window.clearTimeout(id);
  }, [top?.op]); // eslint-disable-line react-hooks/exhaustive-deps

  const label = top ? t(`busy.${top.op}`) : '';
  const pct = top && top.progress.total > 0 ? top.progress.done / top.progress.total : undefined;
  return (
    <>
      <div className="sr-only" aria-live="polite">{top && visible ? label : ''}</div>
      {top && visible && (
        <>
          <div className={cx(s.bar, pct === undefined && s.indeterminate)} aria-hidden>
            <div className={s.fill} style={pct === undefined ? undefined : { transform: `scaleX(${Math.max(0.03, pct)})` }} />
          </div>
          <div className={s.chip} aria-hidden data-loci-keep="">
            <span className={s.dot} />
            <span>{label}{top.progress.phase && top.progress.phase !== 'Done' ? ` · ${top.progress.phase}` : ''}</span>
          </div>
        </>
      )}
    </>
  );
}
