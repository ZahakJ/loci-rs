import { createElement as h } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { Checkbox } from '../../../src/ui/design/primitives/Checkbox';
import { Field } from '../../../src/ui/design/primitives/Field';
import { Radio } from '../../../src/ui/design/primitives/Radio';
import { Select } from '../../../src/ui/design/primitives/Select';
import { IconButton } from '../../../src/ui/design/primitives/Button';

const noop = () => undefined;
/** The attributes of the first `<tag …>` in `html`. */
const attrs = (html: string, tag: string) => html.match(new RegExp(`<${tag}\\b[^>]*>`))?.[0] ?? '';

describe('Radio and Checkbox', () => {
  it('wrap a real native input inside the label (forms and screen readers keep working)', () => {
    const html = renderToStaticMarkup(h(Radio, { name: 'kind', value: 'ahead', checked: true, onChange: noop, label: 'Review ahead', description: 'Cards due soon' }));
    expect(html).toMatch(/^<label[^>]*>/);
    const input = attrs(html, 'input');
    for (const a of ['type="radio"', 'name="kind"', 'value="ahead"', 'checked=""']) expect(input).toContain(a);
    expect(html).toContain('Review ahead');
    expect(html).toContain('Cards due soon');
    expect(html).toMatch(/<span[^>]*aria-hidden="true"/);
  });

  it('checkbox passes disabled and an accessible name without a visible label', () => {
    const html = renderToStaticMarkup(h(Checkbox, { checked: false, onChange: noop, disabled: true, 'aria-label': 'Select row' }));
    const input = attrs(html, 'input');
    for (const a of ['type="checkbox"', 'aria-label="Select row"', 'disabled=""']) expect(input).toContain(a);
    expect(html).toContain('<svg');
  });
});

describe('Select', () => {
  it('renders a combobox button with the value before the chevron, labelled by its Field', () => {
    const html = renderToStaticMarkup(h(Field, { label: 'Day starts', id: 'day', children: h(Select<number>, {
      value: 4, onChange: noop, options: [{ value: 0, label: 'Midnight' }, { value: 4, label: '4 AM' }],
    }) }));
    expect(html).toContain('<label for="day"');
    const button = attrs(html, 'button');
    for (const a of ['id="day"', 'type="button"', 'role="combobox"', 'aria-haspopup="listbox"', 'aria-expanded="false"']) expect(button).toContain(a);
    expect(html.indexOf('4 AM')).toBeGreaterThan(0);
    expect(html.indexOf('4 AM')).toBeLessThan(html.indexOf('<svg'));
    expect(html).not.toContain('<select');
  });
});

describe('IconButton', () => {
  it('never sets a title attribute', () => {
    const html = renderToStaticMarkup(h(IconButton, { icon: 'close', label: 'Close' }));
    expect(html).toContain('aria-label="Close"');
    expect(html).not.toContain('title=');
  });
});
