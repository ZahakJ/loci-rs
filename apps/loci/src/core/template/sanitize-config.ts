/**
 * DOMPurify configuration for card HTML (internals §2: sanitised, then rendered in a shadow root).
 *
 * Allowed: common formatting, headings, lists, tables, ruby, code/pre/kbd, images and media
 * (img/audio/video/source/track with src), span/div with class/style/dir/lang, links
 * (href limited to http(s)/mailto/# by the hook in `sanitizeCardHtml`), `<button>` (audio
 * buttons, IO toggle) and the SVG subset used for image-occlusion masks.
 * Forbidden: script, iframe, object, embed, style elements (render.ts lifts <style> into the
 * scoped stylesheet beforehand), forms, meta/link/base, every on* handler, javascript: URLs.
 * `data-*` attributes are allowed (cloze ordinals, audio indices, maths flags).
 */
import type { Config } from 'dompurify';

const HTML_TAGS = [
  'a', 'abbr', 'b', 'bdi', 'bdo', 'big', 'blockquote', 'br', 'caption', 'center', 'cite', 'code', 'col', 'colgroup',
  'dd', 'del', 'details', 'dfn', 'div', 'dl', 'dt', 'em', 'figcaption', 'figure', 'font', 'h1', 'h2', 'h3', 'h4', 'h5',
  'h6', 'hr', 'i', 'img', 'ins', 'kbd', 'li', 'mark', 'ol', 'p', 'picture', 'pre', 'q', 'rb', 'rp', 'rt', 'rtc', 'ruby', 's',
  'samp', 'small', 'span', 'strike', 'strong', 'sub', 'summary', 'sup', 'table', 'tbody', 'td', 'tfoot', 'th', 'thead',
  'time', 'tr', 'tt', 'u', 'ul', 'var', 'wbr', 'audio', 'video', 'source', 'track', 'button', 'section', 'article',
  'aside', 'header', 'footer', 'main', 'nav', 'label',
];
const SVG_TAGS = ['svg', 'g', 'rect', 'ellipse', 'circle', 'polygon', 'polyline', 'path', 'line', 'text', 'tspan', 'title', 'defs', 'clippath', 'mask', 'use'];

const ATTRS = [
  'class', 'id', 'style', 'dir', 'lang', 'title', 'alt', 'src', 'href', 'width', 'height', 'align', 'valign', 'colspan',
  'rowspan', 'span', 'start', 'reversed', 'type', 'controls', 'loop', 'muted', 'preload', 'poster', 'kind', 'srclang',
  'label', 'default', 'color', 'face', 'size', 'datetime', 'cite', 'open', 'draggable', 'role', 'aria-label',
  'aria-hidden', 'aria-describedby', 'tabindex', 'loading', 'decoding',
  // svg
  'viewbox', 'preserveaspectratio', 'x', 'y', 'x1', 'y1', 'x2', 'y2', 'cx', 'cy', 'r', 'rx', 'ry', 'points', 'd',
  'fill', 'fill-opacity', 'fill-rule', 'stroke', 'stroke-width', 'stroke-opacity', 'stroke-dasharray', 'stroke-linejoin',
  'stroke-linecap', 'vector-effect', 'transform', 'opacity', 'font-size', 'font-family', 'text-anchor',
  'dominant-baseline', 'clip-path', 'xmlns',
];

/** Allowed URI schemes for src/href/poster: http(s), mailto, blob, relative paths and fragments. data: is handled by DOMPurify's DATA_URI_TAGS (img/audio/video/source/track). */
export const CARD_URI_REGEXP = /^(?:(?:https?|mailto|blob):|#|\/|\.{1,2}\/|[^a-z]|[a-z0-9+.\-_ %]+(?:[^a-z0-9+.\-:]|$))/i;

export const CARD_SANITIZE_CONFIG: Config = {
  ALLOWED_TAGS: [...HTML_TAGS, ...SVG_TAGS],
  ALLOWED_ATTR: ATTRS,
  ALLOW_DATA_ATTR: true,
  ALLOW_ARIA_ATTR: true,
  ALLOWED_URI_REGEXP: CARD_URI_REGEXP,
  FORBID_TAGS: ['script', 'iframe', 'object', 'embed', 'style', 'link', 'meta', 'base', 'form', 'input', 'textarea', 'select', 'frame', 'frameset', 'applet', 'noscript', 'template', 'foreignobject', 'canvas'],
  FORBID_ATTR: ['srcset', 'formaction', 'action', 'xlink:href', 'ping', 'background'],
  ALLOW_UNKNOWN_PROTOCOLS: false,
  KEEP_CONTENT: true,
  RETURN_TRUSTED_TYPE: false,
};

/** Links on cards may only go to http(s), mailto or an in-card fragment. */
export function isAllowedHref(href: string): boolean {
  return /^(?:https?:|mailto:|#)/i.test(href.trim());
}

/** Inline styles must not load anything remote or run script-ish constructs. */
export function isSafeStyleAttr(style: string): boolean {
  const s = style.replace(/\\/g, '').toLowerCase();
  if (/expression\s*\(|javascript:|behavior\s*:|-moz-binding/.test(s)) return false;
  for (const m of s.matchAll(/url\(\s*['"]?\s*([^'")\s]*)/g)) {
    const ref = m[1] ?? '';
    if (/^(?:https?:)?\/\//.test(ref) || !/^(?:data:image\/|blob:|#|[^:]*$)/.test(ref)) return false;
  }
  return true;
}
