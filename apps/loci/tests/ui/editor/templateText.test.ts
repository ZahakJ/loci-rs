import { describe, expect, it } from 'vitest';
import { moveItem, referencedFields, renameFieldRefs, uniqueName } from '@ui/views/notetypes/templateText';

describe('template text helpers', () => {
  it('renames every form of a field reference', () => {
    const tpl = '{{Front}} {{#Front}}x{{/Front}} {{^Front}}y{{/Front}} {{type:Front}} {{text:hint:Front}} {{Frontier}}';
    expect(renameFieldRefs(tpl, 'Front', 'Word')).toBe('{{Word}} {{#Word}}x{{/Word}} {{^Word}}y{{/Word}} {{type:Word}} {{text:hint:Word}} {{Frontier}}');
    expect(renameFieldRefs('{{a.b}}', 'a.b', 'c')).toBe('{{c}}');
  });
  it('lists referenced fields', () => {
    expect(referencedFields('{{#Header}}{{Header}}{{/Header}}{{cloze:Text}}{{FrontSide}}')).toEqual(['Header', 'Text', 'FrontSide']);
  });
  it('makes unique names and moves items', () => {
    expect(uniqueName('Basic', ['basic', 'Basic 2'])).toBe('Basic 3');
    expect(uniqueName('New', [])).toBe('New');
    expect(moveItem([1, 2, 3], 0, 2)).toEqual([2, 3, 1]);
    expect(moveItem([1, 2, 3], 0, 5)).toEqual([1, 2, 3]);
  });
});
