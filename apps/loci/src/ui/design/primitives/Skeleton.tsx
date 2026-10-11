import type { CSSProperties } from 'react';
import { cx } from './layer';
import s from './Skeleton.module.css';

export interface SkeletonProps {
  width?: CSSProperties['inlineSize'];
  height?: CSSProperties['blockSize'];
  radius?: 'sm' | 'md' | 'lg' | 'pill';
  /** render N text lines instead of one block */
  lines?: number;
  className?: string;
}

/** Loading placeholder; announced once via aria-busy on the container you place it in. */
export function Skeleton({ width, height, radius = 'sm', lines, className }: SkeletonProps) {
  const r = `var(--r-${radius})`;
  if (lines) {
    return (
      <span className={cx(s.lines, className)} aria-hidden>
        {Array.from({ length: lines }, (_, i) => (
          <span key={i} className={cx(s.sk, s.text)} style={{ inlineSize: i === lines - 1 && lines > 1 ? '62%' : width ?? '100%', borderRadius: r }} />
        ))}
      </span>
    );
  }
  return <span className={cx(s.sk, className)} aria-hidden style={{ inlineSize: width ?? '100%', blockSize: height ?? '1em', borderRadius: r }} />;
}
