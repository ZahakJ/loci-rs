import { t } from '@core/i18n/i18n';
import { IconButton } from '../design/primitives/Button';
import { useUi } from '../store/ui';
import type { ThemePref } from '../store/theme';

const ORDER: ThemePref[] = ['system', 'light', 'dark'];
const ICON = { system: 'system', light: 'sun', dark: 'moon' } as const;

/** Cycles system → light → dark. The full control lives in Settings. */
export function ThemeToggle({ size = 'md' }: { size?: 'sm' | 'md' }) {
  const pref = useUi((s) => s.themePref);
  const set = useUi((s) => s.setThemePref);
  const next = ORDER[(ORDER.indexOf(pref) + 1) % ORDER.length]!;
  return (
    <IconButton
      size={size}
      icon={ICON[pref]}
      label={t('theme.toggle', { current: t(`theme.${pref}`), next: t(`theme.${next}`) })}
      title={t('theme.current', { current: t(`theme.${pref}`) })}
      onClick={() => set(next)}
    />
  );
}
