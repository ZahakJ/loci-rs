/** Full-page note editor (`#/note/:id`): the Add sheet's content with the preview beside it. */
import { useRef } from 'react';
import { t } from '@core/i18n/i18n';
import { useBottomBar } from '@ui/design/primitives/bottomBar';
import { previousHash } from '@ui/routes';
import { Page } from '@ui/shell/Page';
import { NoteForm } from './NoteForm';
import s from './EditNoteView.module.css';

export default function EditNoteView({ noteId }: { noteId: number }) {
  const bar = useRef<HTMLDivElement>(null);
  // The sticky save bar: toasts sit above it instead of covering Save.
  useBottomBar(bar, true);
  const prev = previousHash();
  const back = prev && prev.startsWith('#/browse') ? { href: prev, label: t('browse.title') } : { href: '#/browse', label: t('browse.title') };
  return (
    <NoteForm noteId={noteId} layout="page">
      {({ body, footer, title }) => (
        <Page title={title} back={back} width="wide">
          <div className={s.body}>{body}</div>
          <div ref={bar} className={s.bar}>{footer}</div>
        </Page>
      )}
    </NoteForm>
  );
}
