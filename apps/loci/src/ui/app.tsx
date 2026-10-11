import { Suspense, useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { t } from '@core/i18n/i18n';
import type { IntegrityReport } from '@core/types';
import { api, errorCode, errorMessage, isCallerError, onCollectionEvent, openReport, ready } from './api';
import { Toaster, toast } from './design/primitives/Toast';
import { useMedia } from './design/primitives/layer';
import { useGlobalKeys } from './keys/useGlobalKeys';
import {
  AddSheet, BacklogView, BackupsView, BrowseView, EditNoteView, HomeView, ImportView, NotetypesView, PresetEditorView,
  SettingsView, StatsView, StudyView, WelcomeView, goBack, isFullBleed, navigate, useRoute, type Route,
} from './routes';
import { AppShell } from './shell/AppShell';
import { BusyIndicator } from './shell/BusyIndicator';
import { BootError, IntegrityBanner, NotFound, Splash, ViewFallback } from './shell/Status';
import { ShortcutsHelp } from './shell/ShortcutsHelp';
import { useUi } from './store/ui';

function renderView(route: Route): ReactNode {
  switch (route.name) {
    case 'home': return <HomeView />;
    case 'welcome': return <WelcomeView />;
    case 'study': return <StudyView deckId={route.deckId} custom={route.custom} />;
    case 'backlog': return <BacklogView deckId={route.deckId} />;
    case 'browse': return <BrowseView key={route.q ?? ''} initialQuery={route.q} />;
    case 'stats': return <StatsView deckId={route.deckId} />;
    case 'settings': return <SettingsView section={route.section} />;
    case 'preset': return <PresetEditorView presetId={route.presetId} />;
    case 'notetypes': return <NotetypesView />;
    case 'import': return <ImportView />;
    case 'backups': return <BackupsView />;
    case 'note': return <EditNoteView noteId={route.noteId} />;
    case 'add': return <HomeView />;
    case 'notFound': return <NotFound />;
  }
}

export function App() {
  const route = useRoute();
  const [boot, setBoot] = useState<'loading' | 'ready' | 'error'>('loading');
  const [bootError, setBootError] = useState<string>('');
  const [integrity, setIntegrity] = useState<IntegrityReport | null>(null);
  const isPhone = useMedia('(max-width: 719px)');
  const addSheet = useUi((s) => s.addSheet);
  const openAdd = useUi((s) => s.openAdd);
  const closeAdd = useUi((s) => s.closeAdd);
  const setBusy = useUi((s) => s.setBusy);
  const setSettings = useUi((s) => s.setSettings);
  const syncSystemTheme = useUi((s) => s.syncSystemTheme);

  // underlying view for '#/add' on desktop: the last non-add route
  const lastRoute = useRef<Route>({ name: 'home' });
  if (route.name !== 'add') lastRoute.current = route;

  // boot: open collection, load settings, first-run redirect
  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const report = await openReport;
        await ready;
        const [settings, empty] = await Promise.all([api.getSettings(), api.isEmpty()]);
        if (!alive) return;
        setSettings(settings);
        if (!report.ok) setIntegrity(report);
        const r = window.location.hash || '#/';
        if (empty && (r === '#/' || r === '#' || r === '')) navigate('#/welcome', { replace: true });
        setBoot('ready');
        void import('../bridge').then((b) => b.reportReady());
      } catch (e) {
        if (!alive) return;
        setBootError(errorMessage(e));
        setBoot('error');
      }
    })();
    return () => { alive = false; };
  }, [setSettings]);

  // collection events → busy indicator, error toasts, settings refresh
  useEffect(() => {
    const offs = [
      onCollectionEvent('progress', (e) => { if (e.type === 'progress') setBusy(e.op, e.progress); }),
      // A failed call reaches its caller too, which shows it inline or in its own toast; only
      // errors nobody asked for (automatic backups) are toasted here. Deferred a beat so the
      // caller's rejection is always recorded first.
      onCollectionEvent('error', (e) => {
        if (e.type !== 'error') return;
        const message = e.error.message;
        window.setTimeout(() => { if (!isCallerError(message)) toast({ message, tone: 'danger', id: `error:${message}` }); }, 30);
      }),
      onCollectionEvent('changed', (e) => { if (e.type === 'changed' && e.what.includes('settings')) void api.getSettings().then(setSettings); }),
      onCollectionEvent('optimised', (e) => {
        if (e.type !== 'optimised' || !e.result.adopted) return;
        if (useUi.getState().manualOptimise.includes(e.result.configId)) return; // the preset editor says it
        toast({ message: t('toast.optimised'), icon: 'sparkle' });
      }),
    ];
    // A collection error that no caller handled still gets said once.
    const onUnhandled = (ev: PromiseRejectionEvent) => {
      if (errorCode(ev.reason) === undefined) return;
      toast({ message: errorMessage(ev.reason), tone: 'danger', id: `error:${errorMessage(ev.reason)}` });
    };
    window.addEventListener('unhandledrejection', onUnhandled);
    return () => { offs.forEach((off) => off()); window.removeEventListener('unhandledrejection', onUnhandled); };
  }, [setBusy, setSettings]);

  // follow the system theme while the preference is "system"
  useEffect(() => {
    const mq = window.matchMedia('(prefers-color-scheme: dark)');
    const on = () => syncSystemTheme();
    mq.addEventListener('change', on);
    return () => mq.removeEventListener('change', on);
  }, [syncSystemTheme]);

  const onAdd = useCallback(() => {
    if (window.matchMedia('(max-width: 719px)').matches) navigate('#/add');
    else openAdd({});
  }, [openAdd]);
  useGlobalKeys(route, onAdd);

  // move focus to the new page's heading on navigation (not on first load)
  const first = useRef(true);
  const routeKey = route.name === 'add' ? null : `${route.name}:${JSON.stringify(route)}`;
  useEffect(() => {
    if (boot !== 'ready' || routeKey === null) return;
    if (first.current) { first.current = false; return; }
    window.scrollTo({ top: 0 });
    const id = window.setTimeout(() => {
      if (document.activeElement && document.activeElement !== document.body && document.querySelector('main')?.contains(document.activeElement)) return;
      const h = document.querySelector<HTMLElement>('main [data-page-title], main h1');
      (h ?? document.getElementById('main'))?.focus({ preventScroll: true });
    }, 120);
    return () => window.clearTimeout(id);
  }, [routeKey, boot]);

  if (boot === 'loading') return <Splash />;
  if (boot === 'error') return <BootError message={bootError} />;

  const addOpen = route.name === 'add' || addSheet.open;
  const underlying = route.name === 'add' ? lastRoute.current : route;
  const closeAddSheet = () => {
    if (route.name === 'add') goBack('#/');
    else closeAdd();
  };
  const phoneAddFullscreen = route.name === 'add' && isPhone;

  return (
    <>
      <AppShell route={underlying} fullBleed={isFullBleed(underlying)} hideNavOnPhone={underlying.name === 'note' || phoneAddFullscreen} onAdd={onAdd}>
        {integrity && <IntegrityBanner problems={integrity.problems} onDismiss={() => setIntegrity(null)} />}
        <Suspense fallback={<ViewFallback />}>
          {renderView(underlying)}
        </Suspense>
      </AppShell>
      <Suspense fallback={null}>
        {addOpen && (
          <AddSheet
            open={addOpen}
            onClose={closeAddSheet}
            deckId={route.name === 'add' ? route.deckId ?? null : addSheet.deckId ?? null}
            noteId={addSheet.noteId ?? null}
          />
        )}
      </Suspense>
      <ShortcutsHelp />
      <BusyIndicator />
      <Toaster closeLabel={t('common.dismiss')} />
    </>
  );
}
