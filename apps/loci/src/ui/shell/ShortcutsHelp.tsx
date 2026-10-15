import { Fragment } from 'react';
import { t } from '@core/i18n/i18n';
import { Button } from '../design/primitives/Button';
import { Dialog } from '../design/primitives/Dialog';
import { Kbd } from '../design/primitives/Kbd';
import { SHORTCUT_GROUPS } from '../keys/keymap';
import { useUi } from '../store/ui';
import s from './ShortcutsHelp.module.css';

/** The `?` overlay: every shortcut, including the study keys from internals §6. */
export function ShortcutsHelp() {
  const open = useUi((st) => st.helpOpen);
  const setOpen = useUi((st) => st.setHelpOpen);
  return (
    <Dialog
      open={open}
      onClose={() => setOpen(false)}
      title={t('keys.title')}
      icon="keyboard"
      width="52rem"
      actions={<Button variant="primary" onClick={() => setOpen(false)} data-autofocus>{t('common.done')}</Button>}
    >
      <div className={s.grid}>
        {SHORTCUT_GROUPS.map((g) => (
          <section key={g.id} className={s.group} aria-labelledby={`keys-${g.id}`}>
            <h3 id={`keys-${g.id}`}>{t(g.title)}</h3>
            <dl className={s.list}>
              {g.items.map((it) => (
                <div key={it.label} className={s.row}>
                  <dt>{t(it.label)}</dt>
                  <dd className={s.keys}>
                    {it.keys.map((k, i) => (
                      <Fragment key={k}>
                        {i > 0 && <span>{it.keys.length === 2 && it.label.endsWith('flagColour') ? '–' : '/'}</span>}
                        <Kbd always>{k}</Kbd>
                      </Fragment>
                    ))}
                  </dd>
                </div>
              ))}
            </dl>
          </section>
        ))}
      </div>
    </Dialog>
  );
}
