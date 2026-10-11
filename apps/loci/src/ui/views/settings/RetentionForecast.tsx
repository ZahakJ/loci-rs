import { useEffect, useRef, useState } from 'react';
import { t } from '@core/i18n/i18n';
import type { DeckConfig } from '@core/types';
import { api } from '../../api';
import { Sparkline } from '../../design/primitives/Sparkline';
import { formatPercent } from '../../hooks/format';
import { forecastSentence } from './forecast';
import { workloadRatio } from './presetFields';
import s from './Settings.module.css';

/**
 * The retention → workload sentence and the 90-day sparkline under a desired-retention slider
 * (DESIGN §9). Simulated on the actual cards; falls back to the forgetting-curve ratio.
 * Used by Settings › Studying and the preset editor.
 */
export function RetentionForecast({ config, presetKey }: { config: DeckConfig; presetKey?: string | number }) {
  const [ratio, setRatio] = useState<number | null>(null);
  const [forecast, setForecast] = useState<{ values: number[]; sentence: string } | null>(null);
  const seqRef = useRef(0);
  const dr = config.desiredRetention;
  const newPerDay = config.newPerDay;
  const live = useRef(config);
  live.current = config;

  useEffect(() => {
    const seq = ++seqRef.current;
    const id = window.setTimeout(async () => {
      const draft = live.current;
      try {
        const [here, base] = await Promise.all([
          api.forecastWorkload(null, { ...draft, desiredRetention: dr }),
          api.forecastWorkload(null, { ...draft, desiredRetention: 0.85 }),
        ]);
        if (seq !== seqRef.current) return;
        const sum = (a: number[]) => a.reduce((x, y) => x + y, 0);
        setRatio(sum(base.reviewsPerDay) > 0 ? sum(here.reviewsPerDay) / sum(base.reviewsPerDay) : workloadRatio(dr));
        setForecast({ values: here.reviewsPerDay, sentence: forecastSentence(here) });
      } catch {
        if (seq === seqRef.current) { setRatio(workloadRatio(dr)); setForecast(null); }
      }
    }, 220);
    return () => window.clearTimeout(id);
  }, [dr, newPerDay, config.scheduler, presetKey]);

  const ratioText = ratio === null ? '…' : `${ratio.toFixed(1)}×`;
  return (
    <>
      <p className={s.sentence} aria-live="polite">
        {t('settings.retentionSentence.before')}<strong>{formatPercent(dr)}</strong>{t('settings.retentionSentence.middle')}
        <strong>{ratioText}</strong>{t('settings.retentionSentence.after')}
      </p>
      {forecast && (
        <div className={s.forecast}>
          <Sparkline className={s.forecastSpark} values={forecast.values} fluid height={28} label={forecast.sentence} />
          <span>{forecast.sentence}</span>
        </div>
      )}
    </>
  );
}
