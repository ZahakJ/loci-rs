/**
 * Add sheet (DESIGN §10): a 520px side sheet on desktop, full screen on phones. Opened by `a`.
 * With `noteId` it edits that note in place (used from Study's `e` and Browse).
 */
import { useMedia } from '@ui/design/primitives/layer';
import { Sheet } from '@ui/design/primitives/Sheet';
import { t } from '@core/i18n/i18n';
import { NoteForm } from './NoteForm';
import s from './AddSheet.module.css';

export function AddSheet({ open, onClose, deckId, noteId }: { open: boolean; onClose: () => void; deckId?: number | null; noteId?: number | null }) {
  const isPhone = useMedia('(max-width: 719px)');
  return (
    <NoteForm noteId={noteId ?? null} deckId={deckId ?? null} layout="sheet">
      {({ body, footer, title }) => (
        <Sheet
          open={open}
          onClose={onClose}
          title={title}
          footer={footer}
          width="520px"
          phone="full"
          closeLabel={t('common.close')}
          bodyClassName={isPhone ? s.bodyPhone : s.body}
        >
          {body}
        </Sheet>
      )}
    </NoteForm>
  );
}
