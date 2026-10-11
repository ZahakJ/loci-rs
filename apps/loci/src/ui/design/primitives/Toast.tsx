import { useEffect } from 'react';
import { create } from 'zustand';
import { Icon, type IconName } from '../icons';
import { cx } from './layer';
import s from './Toast.module.css';

export interface ToastInput {
  message: string;
  action?: { label: string; onClick: () => void };
  tone?: 'info' | 'success' | 'danger';
  icon?: IconName;
  /** ms; Infinity keeps it until dismissed. Default 5000 (8000 with an action). */
  duration?: number;
  /** replaces an existing toast with the same id */
  id?: string;
}
interface ToastItem extends ToastInput { id: string; leaving: boolean }

interface ToastState {
  toasts: ToastItem[];
  push: (t: ToastInput) => string;
  dismiss: (id: string) => void;
}

let seq = 0;
/** Visible toasts at most (newest on top). */
export const MAX_TOASTS = 2;
const timers = new Map<string, number>();

export const useToasts = create<ToastState>((set, get) => ({
  toasts: [],
  push: (t) => {
    const id = t.id ?? `t${++seq}`;
    const item: ToastItem = { ...t, id, leaving: false };
    // At most two at a time: the newest and the one before it; older ones are dismissed.
    const next = [...get().toasts.filter((x) => x.id !== id && !x.leaving), item];
    for (const old of next.slice(0, -MAX_TOASTS)) { window.clearTimeout(timers.get(old.id)); timers.delete(old.id); }
    set({ toasts: next.slice(-MAX_TOASTS) });
    const ms = t.duration ?? (t.action ? 8000 : 5000);
    window.clearTimeout(timers.get(id));
    if (Number.isFinite(ms)) timers.set(id, window.setTimeout(() => get().dismiss(id), ms));
    return id;
  },
  dismiss: (id) => {
    window.clearTimeout(timers.get(id));
    timers.delete(id);
    set({ toasts: get().toasts.map((x) => (x.id === id ? { ...x, leaving: true } : x)) });
    window.setTimeout(() => set({ toasts: get().toasts.filter((x) => x.id !== id) }), 140);
  },
}));

/** Show a toast from anywhere (not only components). */
export function toast(input: ToastInput | string): string {
  return useToasts.getState().push(typeof input === 'string' ? { message: input } : input);
}
export function dismissToast(id: string) { useToasts.getState().dismiss(id); }

/** The polite live region. Mount once (the shell does). */
export function Toaster({ closeLabel = 'Dismiss' }: { closeLabel?: string }) {
  const toasts = useToasts((st) => st.toasts);
  const dismiss = useToasts((st) => st.dismiss);
  useEffect(() => () => timers.forEach((t) => window.clearTimeout(t)), []);
  return (
    <div className={s.region} role="region" aria-label="Notifications" data-loci-keep="">
      <div aria-live="polite" aria-atomic="false" style={{ display: 'contents' }}>
        {toasts.map((t) => (
          <div key={t.id} className={cx(s.toast, t.tone === 'danger' && s.danger)} data-leaving={t.leaving} role="status">
            <Icon className={s.icon} name={t.icon ?? (t.tone === 'danger' ? 'warning' : t.tone === 'success' ? 'check' : 'info')} size={18} />
            <span className={s.message}>{t.message}</span>
            {t.action && (
              <button type="button" className={s.action} onClick={() => { t.action!.onClick(); dismiss(t.id); }}>{t.action.label}</button>
            )}
            <button type="button" className={s.close} aria-label={closeLabel} onClick={() => dismiss(t.id)}><Icon name="close" size={16} /></button>
          </div>
        ))}
      </div>
    </div>
  );
}
