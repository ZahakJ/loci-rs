import { useRef, type ReactNode } from 'react';
import { t } from '@core/i18n/i18n';
import { Icon } from '../design/icons';
import { Button } from '../design/primitives/Button';
import { IconButton } from '../design/primitives/Button';
import { Kbd } from '../design/primitives/Kbd';
import { cx } from '../design/primitives/layer';
import { useBottomBar } from '../design/primitives/bottomBar';
import { navSection, type Route } from '../routes';
import { useUi } from '../store/ui';
import { NAV } from './nav';
import { ThemeToggle } from './ThemeToggle';
import { Wordmark } from './Wordmark';
import { APP_VERSION } from './version';
import s from './AppShell.module.css';
import { Tooltip } from '../design/primitives/Tooltip';

export interface AppShellProps {
  route: Route;
  children: ReactNode;
  /** no navigation at all (Study, Welcome) */
  fullBleed?: boolean;
  /** hide the bottom tab bar on phones (full-screen editors) */
  hideNavOnPhone?: boolean;
  onAdd: () => void;
}

export function AppShell({ route, children, fullBleed, hideNavOnPhone, onAdd }: AppShellProps) {
  const section = navSection(route);
  const setHelp = useUi((st) => st.setHelpOpen);
  const current = (id: string) => (section === id ? 'page' : undefined);
  const tabbar = useRef<HTMLElement>(null);
  // the phone tab bar measures 0 where it is hidden (desktop, tablet, full-screen editors)
  useBottomBar(tabbar, !fullBleed && !hideNavOnPhone);

  if (fullBleed) {
    return (
      <div className={cx(s.shell, s.fullbleed, 'loci-fullbleed')}>
        <main id="main" className={s.main} tabIndex={-1}>{children}</main>
      </div>
    );
  }

  return (
    <div className={cx(s.shell, hideNavOnPhone && s.hideNavPhone)}>
      <a href="#main" className={s.skip} onClick={(e) => { e.preventDefault(); document.getElementById('main')?.focus(); }}>{t('shell.skip')}</a>

      <aside className={s.sidebar} aria-label={t('shell.sidebar')}>
        <a href="#/" className={s.brand} aria-label={t('shell.homeLink')}><Wordmark /></a>
        <nav aria-label={t('shell.primaryNav')}>
          <ul role="list" className={s.nav}>
            {NAV.map((n) => (
              <li key={n.id}>
                <a href={n.href} className={s.navLink} aria-current={current(n.id)}>
                  <Icon name={n.icon} />
                  <span className={s.navLabel}>{t(n.label)}</span>
                  {n.kbd && <Kbd className={s.navKbd}>{n.kbd}</Kbd>}
                </a>
              </li>
            ))}
          </ul>
        </nav>
        <div className={s.sideFoot}>
          <Button variant="primary" icon="plus" kbd="a" block onClick={onAdd}>{t('shell.add')}</Button>
          <Tooltip content={t('shell.syncLater')}>
            <div className={s.sync}>
              <span className={s.syncDot} aria-hidden />
              <span>{t('shell.localOnly')}</span>
            </div>
          </Tooltip>
          <div className={s.utility}>
            <ThemeToggle size="sm" />
            <IconButton size="sm" icon="keyboard" label={t('shell.shortcuts')} onClick={() => setHelp(true)} />
            <span className={s.version}>v{APP_VERSION}</span>
          </div>
        </div>
      </aside>

      <header className={s.topbar}>
        <a href="#/" className={s.brand} aria-label={t('shell.homeLink')}><Wordmark size="sm" /></a>
        <nav aria-label={t('shell.primaryNav')}>
          <ul role="list" className={s.topnav}>
            {NAV.map((n) => (
              <li key={n.id}>
                <a href={n.href} className={s.navLink} aria-current={current(n.id)}>
                  <Icon name={n.icon} size={18} />
                  <span>{t(n.label)}</span>
                </a>
              </li>
            ))}
          </ul>
        </nav>
        <div className={s.topActions}>
          <ThemeToggle />
          <Button variant="primary" size="sm" icon="plus" onClick={onAdd}>{t('shell.add')}</Button>
        </div>
      </header>

      <main id="main" className={s.main} tabIndex={-1}>{children}</main>

      <nav ref={tabbar} className={s.tabbar} aria-label={t('shell.primaryNav')}>
        {NAV.slice(0, 2).map((n) => (
          <a key={n.id} href={n.href} className={s.tab} aria-current={current(n.id)}>
            <Icon name={n.icon} size={22} />
            <span>{t(n.label)}</span>
          </a>
        ))}
        <div className={s.addTab}>
          <button type="button" className={s.addButton} aria-label={t('shell.addNote')} onClick={onAdd}>
            <Icon name="plus" size={24} />
          </button>
        </div>
        {NAV.slice(2).map((n) => (
          <a key={n.id} href={n.href} className={s.tab} aria-current={current(n.id)}>
            <Icon name={n.icon} size={22} />
            <span>{t(n.label)}</span>
          </a>
        ))}
      </nav>
    </div>
  );
}
