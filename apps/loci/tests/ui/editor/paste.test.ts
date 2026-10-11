import { describe, expect, it } from 'vitest';
import { cleanPastedHtml, plainTextToHtml, pastedFileName, extensionFor } from '@ui/editor/paste';

describe('cleanPastedHtml', () => {
  it('strips styles, classes and plain spans', () => {
    expect(cleanPastedHtml('<span style="color:red" class="x">hello</span> <b class="k">world</b>')).toBe('hello <b>world</b>');
  });
  it('keeps spans that carry dir or lang, without other attributes', () => {
    expect(cleanPastedHtml('<span dir="rtl" style="font-size:20px" lang="ar">مرحبا</span>')).toBe('<span dir="rtl" lang="ar">مرحبا</span>');
  });
  it('maps strong/em and keeps the allowed tags', () => {
    expect(cleanPastedHtml('<strong>a</strong><em>b</em><u>c</u>H<sub>2</sub>O x<sup>2</sup>')).toBe('<b>a</b><i>b</i><u>c</u>H<sub>2</sub>O x<sup>2</sup>');
    expect(cleanPastedHtml('<ul><li>one</li><li>two</li></ul>')).toBe('<ul><li>one</li><li>two</li></ul>');
    expect(cleanPastedHtml('<pre><code>a\n  b</code></pre>')).toBe('<pre><code>a\n  b</code></pre>');
  });
  it('turns blocks into line breaks', () => {
    expect(cleanPastedHtml('<p>one</p><p>two</p><div>three</div>')).toBe('one<br>two<br>three');
    expect(cleanPastedHtml('<h1>Title</h1>body')).toBe('Title<br>body');
  });
  it('drops scripts, styles, comments and Office tags with their content', () => {
    expect(cleanPastedHtml('<style>p{color:red}</style><script>alert(1)</script><!-- c --><o:p>junk</o:p>ok')).toBe('ok');
  });
  it('honours Office fragment markers', () => {
    expect(cleanPastedHtml('<html><body>before<!--StartFragment--><b>in</b><!--EndFragment-->after</body></html>')).toBe('<b>in</b>');
  });
  it('keeps safe links and images only', () => {
    expect(cleanPastedHtml('<a href="https://x.org" target="_blank" style="c">x</a>')).toBe('<a href="https://x.org">x</a>');
    expect(cleanPastedHtml('<a href="javascript:alert(1)">x</a>')).toBe('x');
    expect(cleanPastedHtml('<img src="a.png" width="20" onerror="x()" alt="A">')).toBe('<img src="a.png" alt="A">');
    expect(cleanPastedHtml('<img src="javascript:x">')).toBe('');
  });
  it('converts bold/italic inline styles (Google Docs) and unwraps the normal-weight b wrapper', () => {
    const gdocs = '<b style="font-weight:normal;" id="docs-internal-guid-1"><span style="font-weight:700">Bold</span> <span style="font-style:italic">it</span></b>';
    expect(cleanPastedHtml(gdocs)).toBe('<b>Bold</b> <i>it</i>');
  });
  it('balances unclosed tags and collapses runs of breaks', () => {
    expect(cleanPastedHtml('<b>open')).toBe('<b>open</b>');
    expect(cleanPastedHtml('a<br><br><br><br>b')).toBe('a<br><br>b');
    expect(cleanPastedHtml('<br>a<br>')).toBe('a');
  });
  it('preserves entities and cloze syntax', () => {
    expect(cleanPastedHtml('<span>{{c1::a &amp; b}}</span>')).toBe('{{c1::a &amp; b}}');
  });
});

describe('plain text and file names', () => {
  it('escapes plain text and keeps line breaks', () => {
    expect(plainTextToHtml('a < b\r\nc & d')).toBe('a &lt; b<br>c &amp; d');
  });
  it('names pasted files', () => {
    expect(extensionFor('image/jpeg')).toBe('jpg');
    expect(pastedFileName({ name: 'diagram.png', type: 'image/png' })).toBe('diagram.png');
    expect(pastedFileName({ name: 'image.png', type: 'image/png' }, 36)).toBe('paste-10.png');
    expect(pastedFileName({ type: 'audio/mpeg' }, 35)).toBe('paste-z.mp3');
  });
});
