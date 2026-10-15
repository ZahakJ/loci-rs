/**
 * UI-side sanitiser wrapper for rendered card HTML (DOMPurify needs a DOM, so this stays in the
 * frontend). The card renderer itself lives in the Rust engine (`loci-core::template`).
 */
import type { Config as PurifyConfig } from 'dompurify';
import { CARD_SANITIZE_CONFIG, isAllowedHref, isSafeStyleAttr } from './sanitize-config';

export interface HtmlSanitizer {
  sanitize(dirty: string, config: PurifyConfig): string;
  addHook?(entryPoint: 'uponSanitizeAttribute', hook: (node: Element, data: { attrName: string; attrValue: string; keepAttr: boolean }) => void): void;
  removeHook?(entryPoint: 'uponSanitizeAttribute'): unknown;
}

/**
 * Sanitises rendered card HTML with an injected DOMPurify instance and Loci's config.
 * When the sanitizer supports hooks, links are restricted to http(s)/mailto/# and style
 * attributes that load remote resources are dropped.
 */
export function sanitizeCardHtml(html: string, purify: HtmlSanitizer): string {
  const hook = (node: Element, data: { attrName: string; attrValue: string; keepAttr: boolean }) => {
    const name = data.attrName.toLowerCase();
    if (name === 'href' || name === 'xlink:href') {
      if (node.nodeName.toLowerCase() === 'a' ? !isAllowedHref(data.attrValue) : !data.attrValue.startsWith('#')) data.keepAttr = false;
    } else if (name === 'style' && !isSafeStyleAttr(data.attrValue)) data.keepAttr = false;
  };
  if (!purify.addHook) return purify.sanitize(html, CARD_SANITIZE_CONFIG);
  purify.addHook('uponSanitizeAttribute', hook);
  try { return purify.sanitize(html, CARD_SANITIZE_CONFIG); }
  finally { purify.removeHook?.('uponSanitizeAttribute'); }
}

