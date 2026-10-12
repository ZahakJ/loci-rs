/**
 * Study screen (DESIGN §8): the most refined thing in the app.
 * Full-bleed: 44px top bar (+ progress hairline), the card centred at 42rem, a bottom bar of
 * constant height. Keyboard-first on desktop, thumb-first on phone.
 */
import { Suspense, lazy, useCallback, useEffect, useLayoutEffect, useReducer, useRef, useState } from 'react';
import { t } from '@core/i18n/i18n';
import type { AnswerResult, Flag, Rating, SessionSummary as Summary, StudyCard, StudyCounts } from '@core/types';
import { CardQueue, CardType, NotetypeKind } from '@core/types';
import { EXTEND_TODAY, api, errorMessage, extendTodayLimits, useApiHas, useQuery } from '@ui/api';
import { CardFrame, type CardFrameHandle, type CardRenderInfo, type CardSide } from '../../card/CardFrame';
import { oneByOneInit, oneByOnePending, oneByOneReducer } from '../../card/oneByOne';
import { Button } from '../../design/primitives/Button';
import { Dialog } from '../../design/primitives/Dialog';
import { EmptyState } from '../../design/primitives/EmptyState';
import type { MenuEntry } from '../../design/primitives/Menu';
import { toast } from '../../design/primitives/Toast';
import { cx, isModalOpen, useMedia } from '../../design/primitives/layer';
import { useTouchOnly } from '../../design/primitives/pointer';
import { primaryPreset } from '../../hooks/presets';
import { isEditable } from '../../keys/keymap';
import { navigate } from '../../routes';
import { useUi } from '../../store/ui';
import { CardInfoSheet } from '../cardinfo/CardInfoPanel';
import { AnswerBar, type BarMode } from './AnswerBar';
import { CustomSessionSheet } from './CustomSessionSheet';
import { FlagDialog } from './FlagDialog';
import { LeechPanel } from './LeechPanel';
import { SessionSummary } from './SessionSummary';
import { SetDueSheet } from './SetDueSheet';
import { TopBar, type QueueKind } from './TopBar';
import {
  cappedElapsed, countOneByOne, frontFieldOrd, gradedAnnouncement, keyHints, moreNewOffer, parseCustom, sessionProgress, totalRemaining, undoSubject,
  type CustomKind,
} from './format';
import { isInteractiveElement, matchStudyKey, type StudyAction } from './keys';
import s from './Study.module.css';

const AddSheet = lazy(() => import('@ui/views/edit/AddSheet').then((m) => ({ default: m.AddSheet })));

type Status = 'loading' | 'study' | 'leech' | 'empty' | 'summary' | 'error';

interface StudyPrefs {
  presetId: number | null;
  autoplayAudio: boolean;
  replayQuestionAudio: boolean;
  maxAnswerSeconds: number;
  showTimer: boolean;
  showIntervalsOnButtons: boolean;
}
const DEFAULT_PREFS: StudyPrefs = { presetId: null, autoplayAudio: true, replayQuestionAudio: true, maxAnswerSeconds: 60, showTimer: false, showIntervalsOnButtons: true };
const prefsCache = new Map<number, StudyPrefs>();

// ── first-sessions explainer (per browser) ──
const EXPLAINER_KEY = 'loci.study.explainer';
interface ExplainerState { sessions: number; dismissed: boolean }
function readExplainer(): ExplainerState {
  try {
    const v = JSON.parse(localStorage.getItem(EXPLAINER_KEY) ?? 'null') as ExplainerState | null;
    if (v && typeof v.sessions === 'number') return { sessions: v.sessions, dismissed: !!v.dismissed };
  } catch { /* storage unavailable */ }
  return { sessions: 0, dismissed: false };
}
function writeExplainer(v: ExplainerState) {
  try { localStorage.setItem(EXPLAINER_KEY, JSON.stringify(v)); } catch { /* ignore */ }
}

function motionMs(token: string, factor = 1): number {
  const v = getComputedStyle(document.documentElement).getPropertyValue(token).trim();
  const n = parseFloat(v);
  if (!Number.isFinite(n)) return 0;
  return (v.endsWith('ms') ? n : n * 1000) * factor;
}
const wait = (ms: number) => new Promise<void>((r) => (ms > 0 ? window.setTimeout(r, ms) : r()));

function queueKind(c: StudyCard | null): QueueKind | null {
  if (!c) return null;
  const q = c.card.queue;
  if (q === CardQueue.New || c.card.type === CardType.New) return 'new';
  if (q === CardQueue.Learn || q === CardQueue.DayLearn || q === CardQueue.Preview) return 'learn';
  return 'review';
}

export default function StudyView({ deckId, custom }: { deckId: number | null; custom?: string }) {
  const [restart, setRestart] = useState(0);
  return <StudySession key={`${deckId ?? 'all'}|${custom ?? ''}|${restart}`} deckId={deckId} custom={custom} onRestart={() => setRestart((n) => n + 1)} />;
}

function StudySession({ deckId, custom, onRestart }: { deckId: number | null; custom?: string; onRestart: () => void }) {
  const [status, setStatus] = useState<Status>('loading');
  const [current, setCurrent] = useState<StudyCard | null>(null);
  const [side, setSide] = useState<CardSide>('front');
  const [counts, setCounts] = useState<StudyCounts | null>(null);
  const [done, setDone] = useState(0);
  const [turn, setTurn] = useState(0);
  const [leaving, setLeaving] = useState(false);
  const [busy, setBusy] = useState(false);
  const [typed, setTyped] = useState('');
  const [obo, dispatchObo] = useReducer(oneByOneReducer, oneByOneInit(0));
  const [animateReveal, setAnimateReveal] = useState(false);
  const [summary, setSummary] = useState<Summary | null>(null);
  const [leech, setLeech] = useState<{ card: StudyCard; result: AnswerResult } | null>(null);
  const [error, setError] = useState('');
  const [prefs, setPrefs] = useState<StudyPrefs>(DEFAULT_PREFS);
  const [elapsed, setElapsed] = useState(0);
  const [announcement, setAnnouncement] = useState('');
  const [infoOpen, setInfoOpen] = useState(false);
  const [editNote, setEditNote] = useState<number | null>(null);
  const [flagOpen, setFlagOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [customOpen, setCustomOpen] = useState<CustomKind['kind'] | null>(null);
  const [dueOpen, setDueOpen] = useState(false);
  const [typing, setTyping] = useState(false);
  const [moreNew, setMoreNew] = useState(0);
  const canExtend = useApiHas(EXTEND_TODAY);
  const isPreview = parseCustom(custom)?.kind === 'preview';
  const [explainer, setExplainer] = useState<ExplainerState>(() => readExplainer());

  const settings = useUi((st) => st.settings);
  const studyHints = useUi((st) => st.studyHints);
  const [barHover, setBarHover] = useState(false);
  const finePointer = useMedia('(pointer: fine)');
  const touchOnly = useTouchOnly();
  const frame = useRef<CardFrameHandle>(null);
  const stageRef = useRef<HTMLDivElement>(null);
  const shownAt = useRef(performance.now());
  const history = useRef<Array<'grade' | 'other'>>([]);
  const sessionOpen = useRef(false);
  const lastFlag = useRef<Flag>(1);
  const doneRef = useRef(0);
  doneRef.current = done;

  const deckQuery = useQuery((a) => (deckId === null ? Promise.resolve(null) : a.getDeck(deckId)), [deckId], ['decks']);
  const deckName = current?.deckName ?? leech?.card.deckName ?? deckQuery.data?.name ?? '';

  const announce = useCallback((msg: string) => {
    setAnnouncement('');
    requestAnimationFrame(() => setAnnouncement(msg));
  }, []);

  const show = useCallback((card: StudyCard, prefix?: string) => {
    setCurrent(card);
    setCounts(card.counts);
    setSide('front');
    setTyped('');
    setTyping(false);
    dispatchObo({ type: 'reset', total: 0 });
    setAnimateReveal(false);
    setLeaving(false);
    setTurn((n) => n + 1);
    shownAt.current = performance.now();
    setElapsed(0);
    announce([prefix, t('studyScreen.questionShown')].filter(Boolean).join('. '));
  }, [announce]);

  const finish = useCallback(async (prefix?: string) => {
    try {
      const sum = await api.endStudy();
      sessionOpen.current = false;
      setLeaving(false);
      setCurrent(null);
      if (doneRef.current === 0 && sum.cards === 0) { setStatus('empty'); return; }
      setSummary(sum);
      setStatus('summary');
      announce([prefix, t('studyScreen.summary.announce', { count: sum.cards })].filter(Boolean).join('. '));
    } catch (e) {
      setError(errorMessage(e));
      setStatus('error');
    }
  }, [announce]);

  // ── start ──
  useEffect(() => {
    let alive = true;
    const kind = parseCustom(custom);
    void (async () => {
      try {
        if (custom && !kind) { setError(t('studyScreen.customInvalid')); setStatus('error'); return; }
        const first = kind ? await api.startCustomStudy(deckId, kind) : await api.startStudy(deckId);
        if (!alive) return;
        sessionOpen.current = true;
        if (!first) { setStatus('empty'); return; }
        const ex = readExplainer();
        const next = { ...ex, sessions: ex.sessions + 1 };
        writeExplainer(next);
        setExplainer(next);
        show(first);
        setStatus('study');
      } catch (e) {
        if (!alive) return;
        setError(errorMessage(e));
        setStatus('error');
      }
    })();
    return () => { alive = false; };
  }, [deckId, custom, show]);

  // Keycaps teach again in every new session (count of answers, help opened).
  useEffect(() => { useUi.getState().resetStudyHints(); }, []);

  // End the session when leaving the screen.
  useEffect(() => () => { if (sessionOpen.current) { sessionOpen.current = false; void api.endStudy().catch(() => undefined); } }, []);

  useEffect(() => { document.title = `${deckName ? deckName.split('::').pop() : t('studyScreen.title')} · Loci`; }, [deckName]);

  // ── preset preferences for the current card's deck ──
  const did = current?.card.did ?? null;
  useEffect(() => {
    if (did === null) return;
    const cached = prefsCache.get(did);
    if (cached) { setPrefs(cached); return; }
    let alive = true;
    void (async () => {
      try {
        const deck = await api.getDeck(did);
        const preset = await api.getPreset(deck.configId);
        const c = preset.config;
        const p: StudyPrefs = {
          presetId: preset.id, autoplayAudio: c.autoplayAudio, replayQuestionAudio: c.replayQuestionAudio,
          maxAnswerSeconds: c.maxAnswerSeconds, showTimer: c.showTimer, showIntervalsOnButtons: c.showIntervalsOnButtons,
        };
        prefsCache.set(did, p);
        if (alive) setPrefs(p);
      } catch { /* keep defaults */ }
    })();
    return () => { alive = false; };
  }, [did]);

  // ── timer ──
  useEffect(() => {
    if (!prefs.showTimer || status !== 'study') return;
    const id = window.setInterval(() => setElapsed(performance.now() - shownAt.current), 1000);
    return () => window.clearInterval(id);
  }, [prefs.showTimer, status, turn]);

  // Keep keyboard focus on the stage so Space/Enter belong to the study keys.
  useLayoutEffect(() => {
    if (status === 'study') stageRef.current?.focus({ preventScroll: true });
  }, [turn, status]);

  // A soft fade at the bottom of the card area while more of the card is below (short screens).
  useEffect(() => {
    const stage = stageRef.current;
    if (!stage) return;
    let frameId = 0;
    const check = () => {
      frameId = 0;
      // measured on the card itself, so the column's bottom padding never counts as "more"
      const card = stage.querySelector('article') ?? stage.firstElementChild;
      const more = card ? card.getBoundingClientRect().bottom > stage.getBoundingClientRect().bottom + 4 : false;
      if (more) stage.setAttribute('data-more', ''); else stage.removeAttribute('data-more');
    };
    const schedule = () => { if (!frameId) frameId = requestAnimationFrame(check); };
    const ro = typeof ResizeObserver !== 'undefined' ? new ResizeObserver(schedule) : null;
    ro?.observe(stage);
    for (const child of Array.from(stage.children)) ro?.observe(child);
    stage.addEventListener('scroll', schedule, { passive: true });
    schedule();
    return () => { ro?.disconnect(); stage.removeEventListener('scroll', schedule); if (frameId) cancelAnimationFrame(frameId); };
  }, [turn, status, side]);

  // ── actions ──
  const reveal = useCallback(() => {
    if (status !== 'study' || side !== 'front' || leaving || !current) return;
    const total = countOneByOne(current.rendered.back.html);
    dispatchObo({ type: 'reset', total });
    setSide('back');
    setAnimateReveal(true);
    announce(total > 0 ? t('studyScreen.answerShownOneByOne', { count: total }) : t('a11y.answerShown'));
  }, [status, side, leaving, current, announce]);

  const revealNext = useCallback(() => {
    if (!oneByOnePending(obo)) return;
    dispatchObo({ type: 'advance' });
    announce(t('studyScreen.revealCount', { revealed: obo.revealed + 1, total: obo.total }));
  }, [obo, announce]);

  const advanceAfterRemoval = useCallback(async (prefix?: string) => {
    const next = await api.nextCard();
    if (next) show(next, prefix);
    else await finish(prefix);
  }, [show, finish]);

  const grade = useCallback(async (r: Rating) => {
    if (busy || status !== 'study' || side !== 'back' || leaving || !current || oneByOnePending(obo)) return;
    const card = current;
    setBusy(true);
    frame.current?.stopAudio();
    const elapsedMs = cappedElapsed(performance.now() - shownAt.current, prefs.maxAnswerSeconds);
    setLeaving(true);
    try {
      const [res] = await Promise.all([api.answerCard(card.card.id, r, elapsedMs), wait(motionMs('--d-base', 0.8))]);
      history.current.push('grade');
      setDone((d) => d + 1);
      useUi.getState().countStudyAnswer();
      setCounts(res.counts);
      const said = gradedAnnouncement(r, card.preview);
      if (res.becameLeech) {
        setLeech({ card, result: res });
        setCurrent(null);
        setLeaving(false);
        setStatus('leech');
        announce(`${said}. ${t('studyScreen.leech.title')}`);
        return;
      }
      if (res.next) show(res.next, said);
      else await finish(said);
    } catch (e) {
      setLeaving(false);
      toast({ message: t('studyScreen.answerFailed', { error: errorMessage(e) }), tone: 'danger' });
    } finally {
      setBusy(false);
    }
  }, [busy, status, side, leaving, current, obo, prefs.maxAnswerSeconds, show, finish, announce]);

  const undo = useCallback(async () => {
    if (busy) return;
    setBusy(true);
    try {
      const res = await api.undo();
      if (!res) { toast({ message: t('undo.nothing'), icon: 'info' }); return; }
      const last = history.current.pop();
      if (last === 'grade') setDone((d) => Math.max(0, d - 1));
      toast({ message: t('common.undone', { label: undoSubject(res.label) }), icon: 'undo' });
      const next = await api.nextCard();
      sessionOpen.current = true;
      setLeech(null);
      setSummary(null);
      if (next) { show(next, t('common.undone', { label: undoSubject(res.label) })); setStatus('study'); }
      else if (status === 'study') await finish();
    } catch (e) {
      toast({ message: t('undo.failed'), tone: 'danger' });
      console.error(e);
    } finally {
      setBusy(false);
    }
  }, [busy, show, finish, status]);

  const removeCurrent = useCallback(async (kind: 'bury' | 'suspend', scope: 'card' | 'note' = 'card') => {
    if (!current || busy) return;
    setBusy(true);
    try {
      const ids = scope === 'note' ? (await api.getCardsOfNote(current.note.id)).map((c) => c.id) : [current.card.id];
      if (!ids.includes(current.card.id)) ids.push(current.card.id);
      if (kind === 'bury') await api.buryCards(ids); else await api.suspendCards(ids, true);
      history.current.push('other');
      const msg = scope === 'note'
        ? (kind === 'bury' ? t('studyScreen.noteBuried') : t('studyScreen.noteSuspended'))
        : (kind === 'bury' ? t('study.buried') : t('study.suspended'));
      toast({ message: msg, icon: kind === 'bury' ? 'bury' : 'suspend', action: { label: t('common.undo'), onClick: () => void undo() } });
      await advanceAfterRemoval(msg);
    } catch (e) {
      toast({ message: t('common.saveFailedDetail', { error: errorMessage(e) }), tone: 'danger' });
    } finally {
      setBusy(false);
    }
  }, [current, busy, undo, advanceAfterRemoval]);

  /** Ctrl+Shift+D: Anki's set-due-date syntax on the current card. */
  const setDue = useCallback(async (days: string) => {
    if (!current || busy) return;
    setBusy(true);
    try {
      await api.bulk([current.card.id], { kind: 'setDue', days });
      history.current.push('other');
      setDueOpen(false);
      const msg = t('studyScreen.setDue.done');
      toast({ message: msg, icon: 'calendar', action: { label: t('common.undo'), onClick: () => void undo() } });
      await advanceAfterRemoval(msg);
    } catch (e) {
      toast({ message: t('common.saveFailedDetail', { error: errorMessage(e) }), tone: 'danger' });
    } finally {
      setBusy(false);
    }
  }, [current, busy, undo, advanceAfterRemoval]);

  const setFlag = useCallback(async (flag: Flag) => {
    if (!current) return;
    const card = current;
    try {
      await api.setFlag([card.card.id], flag);
      if (flag) lastFlag.current = flag;
      setCurrent((c) => (c && c.card.id === card.card.id ? { ...c, card: { ...c.card, flags: (c.card.flags & ~7) | flag } } : c));
      const msg = flag ? t('studyScreen.flagSet', { colour: t(`study.flags.${flag}`) }) : t('study.unflagged');
      toast({ message: msg, icon: 'flag', id: 'study-flag' });
      announce(msg);
    } catch (e) {
      toast({ message: t('common.saveFailedDetail', { error: errorMessage(e) }), tone: 'danger' });
    }
  }, [current, announce]);

  const toggleMark = useCallback(async () => {
    if (!current) return;
    const note = current.note;
    const marked = note.tags.some((tg) => tg.toLowerCase() === 'marked');
    const tags = marked ? note.tags.filter((tg) => tg.toLowerCase() !== 'marked') : [...note.tags, 'marked'];
    try {
      await api.updateNote(note.id, note.fields, tags);
      history.current.push('other');
      setCurrent((c) => (c && c.note.id === note.id ? { ...c, note: { ...c.note, tags } } : c));
      const msg = marked ? t('study.unmarked') : t('study.marked');
      toast({ message: msg, icon: 'star', id: 'study-mark' });
      announce(msg);
    } catch (e) {
      toast({ message: t('common.saveFailedDetail', { error: errorMessage(e) }), tone: 'danger' });
    }
  }, [current, announce]);

  const deleteNote = useCallback(async () => {
    if (!current) return;
    setBusy(true);
    try {
      await api.deleteNotes([current.note.id]);
      history.current.push('other');
      setDeleteOpen(false);
      const msg = t('studyScreen.noteDeleted');
      toast({ message: msg, icon: 'trash', action: { label: t('common.undo'), onClick: () => void undo() } });
      await advanceAfterRemoval(msg);
    } catch (e) {
      toast({ message: t('common.saveFailedDetail', { error: errorMessage(e) }), tone: 'danger' });
    } finally {
      setBusy(false);
    }
  }, [current, undo, advanceAfterRemoval]);

  /** After the editor closes, re-render the card in place (same side). */
  const closeEditor = useCallback(async () => {
    const nid = editNote;
    setEditNote(null);
    const card = current ?? leech?.card;
    if (!card || nid === null || card.note.id !== nid) return;
    try {
      const [rendered, note] = await Promise.all([api.renderCard(card.card.id), api.getNote(nid)]);
      setCurrent((c) => (c && c.card.id === card.card.id ? { ...c, rendered, note } : c));
    } catch {
      if (current && status === 'study') await advanceAfterRemoval();
    }
  }, [editNote, current, leech, status, advanceAfterRemoval]);

  const leave = useCallback(() => {
    if (sessionOpen.current) { sessionOpen.current = false; void api.endStudy().catch(() => undefined); }
    navigate('#/');
  }, []);

  // ── leech flow ──
  const resolveLeech = useCallback(async (action: 'reset' | 'suspend' | 'keep', after?: () => void) => {
    if (!leech) return;
    setBusy(true);
    try {
      await api.resolveLeech(leech.card.card.id, action);
      if (action !== 'keep') toast({ message: action === 'reset' ? t('studyScreen.leech.didReset') : t('study.suspended'), icon: action === 'reset' ? 'undo' : 'suspend' });
      const res = leech.result;
      setLeech(null);
      setStatus('study');
      after?.();
      if (res.next) show(res.next);
      else await finish();
    } catch (e) {
      toast({ message: t('common.saveFailedDetail', { error: errorMessage(e) }), tone: 'danger' });
    } finally {
      setBusy(false);
    }
  }, [leech, show, finish]);

  const addHint = useCallback(async (hint: string) => {
    if (!leech) return;
    setBusy(true);
    try {
      const { note, card } = leech.card;
      const nt = await api.getNotetype(note.ntid);
      const ord = frontFieldOrd(nt.templates, nt.kind === NotetypeKind.Cloze, card.ord, nt.fields.map((f) => f.name), note.fields);
      const fields = [...note.fields];
      fields[ord] = `${fields[ord] ?? ''}<div class="loci-hint" dir="auto"><small>${escapeHtml(hint)}</small></div>`;
      await api.updateNote(note.id, fields, note.tags);
      toast({ message: t('studyScreen.leech.hintSaved'), icon: 'check' });
    } catch (e) {
      toast({ message: t('common.saveFailedDetail', { error: errorMessage(e) }), tone: 'danger' });
      setBusy(false);
      return;
    }
    setBusy(false);
    await resolveLeech('keep');
  }, [leech, resolveLeech]);

  // ── 'nothing due' with new cards left: offer to raise today's new-card limit ──
  useEffect(() => {
    if (status !== 'empty' || custom || !canExtend) { setMoreNew(0); return; }
    let alive = true;
    void (async () => {
      try {
        const [stats, newPerDay] = await Promise.all([
          api.stats({ deckId }),
          (async () => {
            if (deckId !== null) return (await api.getPreset((await api.getDeck(deckId)).configId)).config.newPerDay;
            const [tree, presets] = await Promise.all([api.deckTree(), api.listPresets()]);
            return primaryPreset(tree, presets)?.config.newPerDay ?? 20;
          })(),
        ]);
        if (alive) setMoreNew(moreNewOffer(stats.cardCounts.new, newPerDay));
      } catch { if (alive) setMoreNew(0); }
    })();
    return () => { alive = false; };
  }, [status, custom, canExtend, deckId]);

  const learnMoreNew = useCallback(async () => {
    if (moreNew <= 0 || busy) return;
    setBusy(true);
    try {
      await extendTodayLimits(deckId, moreNew, 0);
      onRestart();
    } catch (e) {
      toast({ message: t('studyScreen.moreNewFailed', { error: errorMessage(e) }), tone: 'danger' });
      setBusy(false);
    }
  }, [moreNew, busy, deckId, onRestart]);

  // ── keyboard ──
  const runAction = useCallback((a: StudyAction) => {
    switch (a.type) {
      case 'reveal': if (side === 'front') reveal(); else if (oneByOnePending(obo)) revealNext(); break;
      case 'grade':
        if (oneByOnePending(obo)) { if (a.rating === 3) revealNext(); break; }
        void grade(a.rating);
        break;
      case 'undo': void undo(); break;
      case 'edit': if (current) setEditNote(current.note.id); break;
      case 'suspend': void removeCurrent('suspend'); break;
      case 'suspend-note': void removeCurrent('suspend', 'note'); break;
      case 'bury': void removeCurrent('bury'); break;
      case 'bury-note': void removeCurrent('bury', 'note'); break;
      case 'delete-note': if (current) setDeleteOpen(true); break;
      case 'set-due': if (current) setDueOpen(true); break;
      case 'flag-toggle': if (current) void setFlag((current.card.flags & 7) ? 0 : lastFlag.current); break;
      case 'flag': if (current) void setFlag((current.card.flags & 7) === a.flag ? 0 : a.flag); break;
      case 'mark': void toggleMark(); break;
      case 'info': if (current) setInfoOpen((o) => !o); break;
      case 'replay': frame.current?.replay(); break;
      case 'type-focus': if (side === 'front') frame.current?.focusTyped(); break;
      case 'help': break;
      case 'leave': leave(); break;
    }
  }, [side, obo, current, reveal, revealNext, grade, undo, removeCurrent, setFlag, toggleMark, leave]);

  const live = useRef({ runAction, status, side, leaving });
  useLayoutEffect(() => { live.current = { runAction, status, side, leaving }; });

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || isModalOpen()) return;
      const target = (e.composedPath()[0] as Element | undefined) ?? document.activeElement;
      if (target instanceof Element && target.closest('[role="menu"]')) return;
      const editable = isEditable(target as Element) || isEditable(document.activeElement);
      const S = live.current;
      const interactive = target instanceof Element && target !== stageRef.current && isInteractiveElement(target);
      const phase = S.status === 'study' && !S.leaving ? S.side : 'other';
      const action = matchStudyKey({ key: e.key, ctrlKey: e.ctrlKey, metaKey: e.metaKey, altKey: e.altKey, shiftKey: e.shiftKey, repeat: e.repeat, editable, interactive }, phase);
      if (!action || action.type === 'help') return; // '?' belongs to the shell's help overlay
      if (S.status === 'leech' && action.type !== 'undo' && action.type !== 'leave') return;
      e.preventDefault();
      S.runAction(action);
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, []);

  // ── derived ──
  const flag = (current?.card.flags ?? 0) & 7;
  const marked = !!current?.note.tags.some((tg) => tg.toLowerCase() === 'marked');
  const remaining = totalRemaining(counts);
  const progress = status === 'summary' ? 1 : sessionProgress(done, remaining);
  const showKeys = !touchOnly;
  const hints = keyHints({
    answered: studyHints.answered, hover: barHover, helpOpened: studyHints.helpOpened,
    setting: settings?.showKeyboardHints ?? true, touch: touchOnly,
  });
  const hasCard = !!current && status === 'study';
  const pending = oneByOnePending(obo);

  const barMode: BarMode = !hasCard ? { kind: 'idle' }
    : side === 'front' ? { kind: 'front', typing }
      : pending ? { kind: 'reveal-next', revealed: obo.revealed, total: obo.total }
        : { kind: 'back', preview: current!.preview, showIntervals: prefs.showIntervalsOnButtons };

  const menu: MenuEntry[] = [
    { id: 'edit', label: t('study.editCard'), icon: 'edit', kbd: 'e', disabled: !hasCard, onSelect: () => current && setEditNote(current.note.id) },
    { id: 'info', label: t('study.cardInfo'), icon: 'info', kbd: 'i', disabled: !hasCard, onSelect: () => setInfoOpen(true) },
    { id: 'flag', label: t('studyScreen.flagMenu'), icon: 'flag', kbd: 'f', disabled: !hasCard, onSelect: () => setFlagOpen(true) },
    { id: 'mark', label: marked ? t('studyScreen.unmark') : t('study.mark'), icon: 'star', kbd: 'm', disabled: !hasCard, onSelect: () => void toggleMark() },
    { id: 'sep1', separator: true },
    { id: 'bury', label: t('study.bury'), icon: 'bury', kbd: 'b', disabled: !hasCard, onSelect: () => void removeCurrent('bury') },
    { id: 'bury-note', label: t('studyScreen.buryNote'), icon: 'bury', kbd: '=', disabled: !hasCard, onSelect: () => void removeCurrent('bury', 'note') },
    { id: 'suspend', label: t('study.suspend'), icon: 'suspend', kbd: 's', disabled: !hasCard, onSelect: () => void removeCurrent('suspend') },
    { id: 'suspend-note', label: t('studyScreen.suspendNote'), icon: 'suspend', kbd: '!', disabled: !hasCard, onSelect: () => void removeCurrent('suspend', 'note') },
    { id: 'set-due', label: t('studyScreen.setDue.menu'), icon: 'calendar', kbd: 'ctrl+shift+d', disabled: !hasCard, onSelect: () => setDueOpen(true) },
    { id: 'delete', label: t('studyScreen.deleteNote'), icon: 'trash', kbd: 'ctrl+delete', danger: true, disabled: !hasCard, onSelect: () => setDeleteOpen(true) },
    { id: 'sep2', separator: true },
    { id: 'undo', label: t('study.undoAnswer'), icon: 'undo', kbd: 'z', onSelect: () => void undo() },
    { id: 'options', label: t('studyScreen.options'), icon: 'settings', disabled: prefs.presetId === null, onSelect: () => prefs.presetId !== null && navigate(`#/settings/presets/${prefs.presetId}`) },
    { id: 'help', label: t('study.help'), icon: 'keyboard', kbd: '?', onSelect: () => useUi.getState().setHelpOpen(true) },
  ];

  const explainerOn = !explainer.dismissed && explainer.sessions > 0 && explainer.sessions <= 3 && (status === 'study');
  const dismissExplainer = () => { const v = { ...explainer, dismissed: true }; writeExplainer(v); setExplainer(v); };

  const typedOptions = { ignoreCase: settings?.typedAnswerIgnoreCase ?? false, ignoreDiacritics: settings?.typedAnswerIgnoreDiacritics ?? false };

  const onRendered = useCallback((info: CardRenderInfo) => {
    if (info.side === 'front' && info.hasTypeInput && finePointer) frame.current?.focusTyped();
  }, [finePointer]);

  return (
    <div className={s.root} data-status={status}>
      <TopBar
        deckName={deckName}
        counts={status === 'study' || status === 'leech' ? counts : null}
        currentKind={status === 'study' ? queueKind(current) : null}
        progress={progress}
        flag={status === 'study' ? flag : 0}
        marked={status === 'study' && marked}
        elapsed={prefs.showTimer && status === 'study' && side === 'front' ? Math.min(elapsed, prefs.maxAnswerSeconds > 0 ? prefs.maxAnswerSeconds * 1000 : Infinity) : null}
        menu={menu}
        onLeave={leave}
        preview={isPreview && status !== 'error'}
      />

      <div ref={stageRef} className={s.stage} tabIndex={-1} data-study-stage>
        {status === 'loading' && (
          <div className={s.cardColumn} aria-busy="true">
            <div className={cx(s.panel, s.panelLoading)}><span className="sr-only">{t('common.loading')}</span><span className={s.loadingLine} /><span className={s.loadingLine} /></div>
          </div>
        )}

        {status === 'study' && current && (
          <div className={s.cardColumn}>
            <div key={turn} className={s.cardMotion} data-leaving={leaving || undefined}>
              <article className={s.panel} aria-label={side === 'front' ? t('studyScreen.question') : t('studyScreen.answer')}>
                <CardFrame
                  ref={frame}
                  card={current.rendered}
                  side={side}
                  typed={typed}
                  onTypedChange={setTyped}
                  onTypedSubmit={reveal}
                  onTypedFocus={setTyping}
                  typedOptions={typedOptions}
                  clozeRevealed={side === 'back' && obo.total > 0 ? obo.revealed : undefined}
                  onClozeClick={(i) => dispatchObo({ type: 'reveal', index: i })}
                  autoplay={prefs.autoplayAudio}
                  replayQuestionAudio={prefs.replayQuestionAudio}
                  animateReveal={animateReveal}
                  onRendered={onRendered}
                />
              </article>
            </div>
          </div>
        )}

        {status === 'leech' && leech && (
          <div className={s.cardColumn}>
            <LeechPanel
              lapses={leech.card.card.lapses + 1}
              busy={busy}
              onRewrite={() => { setEditNote(leech.card.note.id); }}
              onHint={(h) => void addHint(h)}
              onReset={() => void resolveLeech('reset')}
              onSuspend={() => void resolveLeech('suspend')}
              onKeep={() => void resolveLeech('keep')}
            />
          </div>
        )}

        {status === 'empty' && (
          <div className={s.cardColumn}>
            <EmptyState
              className={s.empty}
              icon="check"
              title={custom ? t('studyScreen.emptyCustom') : t('study.nothingDue')}
              body={custom ? t('studyScreen.emptyCustomHint') : t('studyScreen.nothingDueBody')}
              actions={(
                <>
                  {moreNew > 0 && (
                    <Button variant="primary" icon="plus" loading={busy} onClick={() => void learnMoreNew()}>{t('studyScreen.moreNew', { count: moreNew })}</Button>
                  )}
                  <Button variant={moreNew > 0 ? 'secondary' : 'primary'} icon="calendar" onClick={() => navigate(`#/study/${deckId ?? 'all'}?custom=ahead:1`)}>{t('studyScreen.studyAhead')}</Button>
                  <Button variant="secondary" onClick={() => setCustomOpen('ahead')}>{t('customStudy.title')}</Button>
                  <Button variant="ghost" icon="home" onClick={leave}>{t('studyScreen.summary.home')}</Button>
                </>
              )}
            />
          </div>
        )}

        {status === 'summary' && summary && (
          <div className={s.cardColumn}>
            <SessionSummary summary={summary} onHome={leave} onMore={() => setCustomOpen('ahead')} />
          </div>
        )}

        {status === 'error' && (
          <div className={s.cardColumn}>
            <EmptyState
              className={s.empty}
              icon="warning"
              title={t('studyScreen.errorTitle')}
              body={error}
              actions={(
                <>
                  <Button variant="primary" onClick={onRestart}>{t('common.retry')}</Button>
                  <Button variant="ghost" icon="home" onClick={leave}>{t('studyScreen.summary.home')}</Button>
                </>
              )}
            />
          </div>
        )}
      </div>

      {(status === 'study' || status === 'loading' || status === 'leech') && (
        <AnswerBar
          mode={status === 'study' ? barMode : { kind: 'idle' }}
          showKeys={hints !== 'off'}
          keysFaded={hints === 'faded'}
          onHoverChange={setBarHover}
          busy={busy}
          onShow={reveal}
          onRevealNext={revealNext}
          onGrade={(r) => void grade(r)}
          explainer={explainerOn ? { onDismiss: dismissExplainer } : null}
        />
      )}

      <div className="sr-only" role="status" aria-live="polite" aria-atomic="true">{announcement}</div>

      <CardInfoSheet open={infoOpen} onClose={() => setInfoOpen(false)} cardId={current?.card.id ?? null} />
      <FlagDialog open={flagOpen} current={flag} showKeys={showKeys} onClose={() => setFlagOpen(false)} onPick={(f) => { setFlagOpen(false); void setFlag(f); }} />
      <Dialog
        open={deleteOpen}
        onClose={() => setDeleteOpen(false)}
        title={t('studyScreen.deleteTitle')}
        description={t('studyScreen.deleteBody')}
        destructive
        busy={busy}
        confirmLabel={t('common.delete')}
        cancelLabel={t('common.cancel')}
        onConfirm={deleteNote}
      />
      <SetDueSheet open={dueOpen} busy={busy} onClose={() => setDueOpen(false)} onSubmit={(d) => void setDue(d)} />
      <CustomSessionSheet open={customOpen !== null} initial={customOpen ?? 'ahead'} onClose={() => setCustomOpen(null)} deckId={deckId} onRestart={onRestart} />
      {editNote !== null && (
        <Suspense fallback={null}>
          <AddSheet open onClose={() => void closeEditor()} noteId={editNote} deckId={deckId} />
        </Suspense>
      )}
    </div>
  );
}

const escapeHtml = (v: string) => v.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
