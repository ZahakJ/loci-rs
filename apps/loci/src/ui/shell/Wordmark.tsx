import { Icon } from '../design/icons';
import { cx } from '../design/primitives/layer';
import s from './Wordmark.module.css';

/** The palace mark + "Loci" set in Newsreader. */
export function Wordmark({ size = 'md', className, showName = true }: { size?: 'sm' | 'md' | 'lg' | 'xl'; className?: string; showName?: boolean }) {
  return (
    <span className={cx(s.wordmark, s[size], className)}>
      <span className={s.mark} aria-hidden><Icon name="palace" size={size === 'xl' ? 40 : size === 'lg' ? 30 : size === 'sm' ? 20 : 24} /></span>
      {showName && <span className={s.name}>Loci</span>}
    </span>
  );
}
