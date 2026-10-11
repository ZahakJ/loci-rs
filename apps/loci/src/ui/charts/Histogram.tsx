/** Bucketed distribution (intervals, stability, difficulty): single-series bars over ordered buckets. */
import { Bars } from './Bars';
import type { Tone } from './ChartFrame';

export interface HistogramProps {
  buckets: Array<{ bucket: string; count: number }>;
  title: string;
  desc: string;
  seriesLabel: string;
  categoryHeader: string;
  tone?: Tone;
  height?: number;
  /** emphasise buckets (e.g. "hard" difficulty) */
  highlight?: number;
  format?: (v: number) => string;
  className?: string;
}

export function Histogram({ buckets, title, desc, seriesLabel, categoryHeader, tone = 'accent', height = 150, highlight, format, className }: HistogramProps) {
  return (
    <Bars
      data={buckets.map((b) => ({ label: b.bucket, values: [b.count] }))}
      series={[{ key: 'count', label: seriesLabel, tone }]}
      title={title}
      desc={desc}
      height={height}
      categoryHeader={categoryHeader}
      maxXLabels={10}
      highlight={highlight}
      format={format}
      className={className}
    />
  );
}
