import { useRef } from 'react';
import { createPortal } from 'react-dom';
import { t } from '@core/i18n/i18n';
import { IconButton } from '../design/primitives/Button';
import { useModalLayer, usePresence } from '../design/primitives/layer';
import s from './CardFrame.module.css';

export interface LightboxProps {
  image: { src: string; alt: string } | null;
  onClose: () => void;
}

/** Full-screen image zoom for card images. Esc, click or the close button dismiss it. */
export function Lightbox({ image, onClose }: LightboxProps) {
  const open = image !== null;
  const { mounted, state } = usePresence(open, '--d-fast');
  const last = useRef(image);
  if (image) last.current = image;
  const panel = useRef<HTMLDivElement>(null);
  useModalLayer(panel, open && mounted, onClose);
  if (!mounted || !last.current) return null;
  const img = last.current;
  return createPortal(
    <div className={s.lightbox} data-state={state} onClick={onClose}>
      <div ref={panel} className={s.figure} role="dialog" aria-modal="true" aria-label={img.alt || t('cardFrame.imageZoom')} tabIndex={-1}>
        <img className={s.image} src={img.src} alt={img.alt} />
        {img.alt && <p className={s.caption}>{img.alt}</p>}
      </div>
      <IconButton className={s.close} icon="close" label={t('common.close')} variant="secondary" onClick={onClose} />
    </div>,
    document.body,
  );
}
