import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import './ui/fonts.css';
import './ui/design/tokens.css';
import './ui/design/base.css';
import './ui/design/motion.css';
import { getDir, getLocale, setLocale } from '@core/i18n/i18n';
import { App } from './ui/app';
import { applyTheme, readStoredTheme, resolveTheme, systemPrefersDark } from './ui/store/theme';
import { installLeaveBackup } from './bridge';
import { syncWindowTheme } from './ui/shell/native';

// Only English ships today; the plumbing follows the browser so a new dictionary just works.
setLocale(typeof navigator !== 'undefined' ? navigator.language : 'en');
document.documentElement.lang = getLocale();
document.documentElement.dir = getDir();

applyTheme(resolveTheme(readStoredTheme(localStorage), systemPrefersDark()));
void syncWindowTheme(readStoredTheme(localStorage));

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);

installLeaveBackup();
