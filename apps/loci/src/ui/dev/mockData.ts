/**
 * Seeded fake content for the mock CollectionApi (dev and screenshots only).
 * Deterministic: the same build always produces the same collection.
 */
import type { DeckConfig, Notetype, Field, Template } from '@core/types';
import { NotetypeKind } from '@core/types';

/** Fixed "now" so screenshots are stable: Tue 6 Oct 2026, 09:30 UTC. */
export const NOW_MS = Date.UTC(2026, 9, 6, 9, 30);
export const CRT_S = Date.UTC(2024, 0, 1) / 1000;
export const TODAY = Math.floor((NOW_MS / 1000 - CRT_S) / 86400);

export function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export const FSRS6_DEFAULT = [
  0.212, 1.2931, 2.3065, 8.2956, 6.4133, 0.8334, 3.0194, 0.001, 1.8722, 0.1666, 0.796,
  1.4835, 0.0614, 0.2629, 1.6483, 0.6014, 1.8729, 0.5425, 0.0912, 0.0658, 0.1542,
];

export const STANDARD_CONFIG: DeckConfig = {
  scheduler: 'fsrs',
  newPerDay: 20, reviewsPerDay: 9999, minutesPerDay: 20,
  learnSteps: [1, 10], relearnSteps: [10],
  graduatingInterval: 1, easyInterval: 4,
  newOrder: 'due', newGather: 'deck', newSort: 'template-then-order',
  reviewOrder: 'due-then-random', newReviewMix: 'mix', interdayLearningMix: 'mix',
  initialEase: 2500, easyBonus: 1.3, intervalModifier: 1, hardMultiplier: 1.2, lapseMultiplier: 0,
  minimumLapseInterval: 1, maximumInterval: 36500,
  fsrsParams: FSRS6_DEFAULT, desiredRetention: 0.9, historicalRetention: 0.9,
  autoOptimise: true, optimiseEveryReviews: 400,
  leechThreshold: 8, leechAction: 'tag',
  burySiblingsNew: true, burySiblingsReview: true, burySiblingsInterday: false,
  autoplayAudio: true, replayQuestionAudio: true,
  maxAnswerSeconds: 60, showTimer: false, showIntervalsOnButtons: true,
};

const field = (ord: number, name: string, rtl = false): Field => ({
  ord, name, config: { sticky: false, rtl, font: '', size: 0, description: '', plainText: false, collapsed: false },
});
const tmpl = (ord: number, name: string, qfmt: string, afmt: string): Template => ({ ord, name, qfmt, afmt, bqfmt: '', bafmt: '', config: {} });
const nt = (id: number, name: string, kind: NotetypeKind, fields: Field[], templates: Template[], builtin: Notetype['config']['builtin']): Notetype => ({
  id, name, kind, css: '.card { text-align: center; }', latexPre: '', latexPost: '', sortField: 0, fields, templates,
  config: { req: templates.map((t) => [t.ord, 'any', [0]] as [number, 'any', number[]]), builtin }, mtime: CRT_S + 100, usn: -1,
});

export const NOTETYPES: Notetype[] = [
  nt(1, 'Basic', NotetypeKind.Standard, [field(0, 'Front'), field(1, 'Back')], [tmpl(0, 'Card 1', '{{Front}}', '{{FrontSide}}<hr id=answer>{{Back}}')], 'basic'),
  nt(2, 'Basic (and reversed card)', NotetypeKind.Standard, [field(0, 'Front'), field(1, 'Back')], [
    tmpl(0, 'Card 1', '{{Front}}', '{{FrontSide}}<hr id=answer>{{Back}}'),
    tmpl(1, 'Card 2', '{{Back}}', '{{FrontSide}}<hr id=answer>{{Front}}'),
  ], 'basic-reversed'),
  nt(3, 'Basic (type in the answer)', NotetypeKind.Standard, [field(0, 'Front'), field(1, 'Back')], [
    tmpl(0, 'Card 1', '{{Front}}\n\n{{type:Back}}', '{{Front}}<hr id=answer>{{type:Back}}'),
  ], 'basic-typed'),
  nt(4, 'Cloze', NotetypeKind.Cloze, [field(0, 'Text'), field(1, 'Back Extra')], [tmpl(0, 'Cloze', '{{cloze:Text}}', '{{cloze:Text}}<br>{{Back Extra}}')], 'cloze'),
  nt(5, 'Image Occlusion', NotetypeKind.Cloze, [field(0, 'Occlusion'), field(1, 'Image'), field(2, 'Header'), field(3, 'Back Extra'), field(4, 'Comments')], [
    tmpl(0, 'Image Occlusion', '{{#Header}}<div>{{Header}}</div>{{/Header}}{{cloze:Occlusion}}', '{{cloze:Occlusion}}<br>{{Back Extra}}'),
  ], 'image-occlusion'),
];

export interface DeckSeed {
  id: number; name: string; configId: number; description: string;
  /** synthetic due counts for today (the card pool is cycled to fill them) */
  counts: { new: number; learn: number; review: number };
  totalCards: number;
}

export const DECKS: DeckSeed[] = [
  { id: 1001, name: 'Languages', configId: 2, description: 'Everything spoken and written.', counts: { new: 0, learn: 0, review: 3 }, totalCards: 64 },
  { id: 1002, name: 'Languages::Arabic', configId: 2, description: 'Modern Standard Arabic: core vocabulary and short sentences.', counts: { new: 6, learn: 3, review: 24 }, totalCards: 1240 },
  { id: 1003, name: 'Languages::Spanish', configId: 2, description: 'Travel Spanish, verbs first.', counts: { new: 5, learn: 0, review: 11 }, totalCards: 486 },
  { id: 1004, name: 'Medicine', configId: 3, description: 'Second-year boards.', counts: { new: 0, learn: 1, review: 4 }, totalCards: 312 },
  { id: 1005, name: 'Medicine::Pharmacology', configId: 3, description: 'Mechanisms, classes, antidotes.', counts: { new: 8, learn: 2, review: 22 }, totalCards: 2108 },
  { id: 1006, name: 'Mathematics', configId: 1, description: 'Calculus identities and the odd proof.', counts: { new: 4, learn: 0, review: 6 }, totalCards: 158 },
];

export const PRESET_SEEDS: Array<{ id: number; name: string; config: DeckConfig }> = [
  { id: 1, name: 'Default', config: STANDARD_CONFIG },
  { id: 2, name: 'Languages', config: { ...STANDARD_CONFIG, newPerDay: 15, minutesPerDay: 25, desiredRetention: 0.88, fsrsParams: [0.2172, 1.1771, 3.2602, 16.1507, 7.0114, 0.57, 2.0966, 0.0069, 1.5261, 0.112, 1.0178, 1.849, 0.1133, 0.3127, 2.2934, 0.2191, 3.0004, 0.7536, 0.3332, 0.1437, 0.2] } },
  { id: 3, name: 'Exam cram', config: { ...STANDARD_CONFIG, newPerDay: 40, minutesPerDay: 45, desiredRetention: 0.93, learnSteps: [1, 10, 60] } },
];

const svgUrl = (svg: string) => `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;

/** A floor plan of a small memory palace (image + occlusion cards). */
export const PALACE_SVG = svgUrl(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 480 300" font-family="Georgia, serif" font-size="15">
<rect width="480" height="300" fill="oklch(96% 0.015 85)"/>
<g fill="none" stroke="oklch(35% 0.03 260)" stroke-width="3">
<rect x="20" y="20" width="440" height="260"/><path d="M180 20v120M180 180v100M320 20v70M320 130v150M20 140h120M180 140h40M260 140h60"/>
</g>
<g fill="oklch(40% 0.03 260)"><text x="60" y="85">Hall</text><text x="60" y="215">Library</text><text x="225" y="85">Atrium</text><text x="225" y="225">Garden</text><text x="370" y="60">Study</text><text x="365" y="215">Kitchen</text></g>
<circle cx="250" cy="110" r="6" fill="oklch(62% 0.13 70)"/>
</svg>`);

const HEART_SVG = svgUrl(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 360 240" font-family="Georgia, serif" font-size="14">
<rect width="360" height="240" fill="oklch(97% 0.01 20)"/>
<path d="M180 214C88 150 40 104 64 62c18-32 70-36 116 4 46-40 98-36 116-4 24 42-24 88-116 152z" fill="oklch(70% 0.12 20)" stroke="oklch(45% 0.12 20)" stroke-width="3"/>
<path d="M180 66v140M90 120h180" stroke="oklch(45% 0.12 20)" stroke-width="2" fill="none"/>
<g fill="oklch(25% 0.03 20)"><text x="110" y="104">RA</text><text x="228" y="104">LA</text><text x="120" y="160">RV</text><text x="222" y="160">LV</text></g>
</svg>`);

export type NoteSeed = { deck: number; ntid: number; fields: string[]; tags: string[] };

const m = (tex: string, display = false) => `<span class="loci-math" data-display="${display ? 1 : 0}">${tex}</span>`;
const ar = (s: string) => `<span dir="rtl" lang="ar">${s}</span>`;

export const NOTE_SEEDS: NoteSeed[] = [
  // Languages (parent): 4
  { deck: 1001, ntid: 1, fields: ['Where does the word <i>mnemonic</i> come from?', 'Greek <i>mnēmonikos</i>, “of memory”, from Mnemosyne, mother of the Muses.'], tags: ['etymology'] },
  { deck: 1001, ntid: 1, fields: ['What is the <b>method of loci</b>?', 'Placing images of things to remember along a route through a familiar building, then walking it in the mind.'], tags: ['memory'] },
  { deck: 1001, ntid: 4, fields: ['The forgetting curve was first measured by {{c1::Hermann Ebbinghaus}} in {{c2::1885}}.', 'He memorised nonsense syllables and tested himself.'], tags: ['memory', 'history'] },
  { deck: 1001, ntid: 5, fields: ['{{c1::image-occlusion:rect:left=.05:top=.08:width=.3:height=.38}}{{c2::image-occlusion:rect:left=.4:top=.08:width=.28:height=.38}}{{c3::image-occlusion:rect:left=.68:top=.08:width=.28:height=.24}}', `<img src="${PALACE_SVG}">`, 'Rooms of the palace', '', ''], tags: ['memory', 'occlusion'] },
  // Arabic: 8
  { deck: 1002, ntid: 2, fields: [ar('كِتَاب'), 'book'], tags: ['arabic', 'noun'] },
  { deck: 1002, ntid: 2, fields: [ar('مَدْرَسَة'), 'school'], tags: ['arabic', 'noun'] },
  { deck: 1002, ntid: 1, fields: [ar('ذَهَبْتُ إِلَى السُّوقِ صَبَاحًا'), 'I went to the market in the morning.'], tags: ['arabic', 'sentence'] },
  { deck: 1002, ntid: 1, fields: [ar('شَمْس'), 'sun <span class="hint">(feminine)</span>'], tags: ['arabic', 'noun'] },
  { deck: 1002, ntid: 1, fields: [ar('قَمَر'), 'moon'], tags: ['arabic', 'noun'] },
  { deck: 1002, ntid: 4, fields: [`${ar('{{c1::السَّلَامُ}} عَلَيْكُمْ')}`, 'Peace be upon you (greeting).'], tags: ['arabic', 'phrase'] },
  { deck: 1002, ntid: 1, fields: [ar('يَتَذَكَّرُ'), 'he remembers'], tags: ['arabic', 'verb'] },
  { deck: 1002, ntid: 1, fields: [ar('مَاء'), 'water'], tags: ['arabic', 'noun'] },
  // Spanish: 8
  { deck: 1003, ntid: 3, fields: ['How do you say <i>to remember</i> in Spanish?', 'recordar'], tags: ['spanish', 'verb'] },
  { deck: 1003, ntid: 2, fields: ['el árbol', 'the tree'], tags: ['spanish', 'noun'] },
  { deck: 1003, ntid: 2, fields: ['la ventana', 'the window'], tags: ['spanish', 'noun'] },
  { deck: 1003, ntid: 4, fields: ['Ayer {{c1::fui}} al cine con {{c2::mis amigos}}.', 'Preterite of <i>ir</i>.'], tags: ['spanish', 'grammar'] },
  { deck: 1003, ntid: 1, fields: ['¿Dónde está la estación?', 'Where is the station?'], tags: ['spanish', 'travel'] },
  { deck: 1003, ntid: 3, fields: ['Spanish for <i>the key</i> (to a door)', 'la llave'], tags: ['spanish', 'noun'] },
  { deck: 1003, ntid: 1, fields: ['olvidar', 'to forget'], tags: ['spanish', 'verb'] },
  { deck: 1003, ntid: 1, fields: ['Tengo que irme.', 'I have to go.'], tags: ['spanish', 'phrase'] },
  // Medicine: 4
  { deck: 1004, ntid: 5, fields: ['{{c1::image-occlusion:rect:left=.27:top=.36:width=.14:height=.12}}{{c2::image-occlusion:rect:left=.6:top=.36:width=.14:height=.12}}{{c3::image-occlusion:rect:left=.3:top=.6:width=.14:height=.12}}{{c4::image-occlusion:rect:left=.58:top=.6:width=.14:height=.12}}', `<img src="${HEART_SVG}">`, 'Chambers of the heart', '', ''], tags: ['anatomy', 'occlusion'] },
  { deck: 1004, ntid: 1, fields: ['Normal adult resting heart rate', '60–100 beats per minute'], tags: ['physiology'] },
  { deck: 1004, ntid: 4, fields: ['The {{c1::sinoatrial node}} is the heart’s natural pacemaker.', ''], tags: ['physiology'] },
  { deck: 1004, ntid: 1, fields: ['Which nerve carries parasympathetic supply to the heart?', 'The vagus nerve (CN X).'], tags: ['anatomy'] },
  // Pharmacology: 8
  { deck: 1005, ntid: 4, fields: ['{{c1::Warfarin}} inhibits vitamin K epoxide reductase; reverse it with {{c2::vitamin K}} and prothrombin complex.', 'Monitor with INR.'], tags: ['pharm', 'anticoagulant'] },
  { deck: 1005, ntid: 1, fields: ['Mechanism of action of <b>metformin</b>', 'Decreases hepatic gluconeogenesis; increases insulin sensitivity.'], tags: ['pharm', 'endocrine'] },
  { deck: 1005, ntid: 1, fields: ['Drug-name ending for β-blockers', '<b>-olol</b> (e.g. propranolol, metoprolol)'], tags: ['pharm', 'cardio'] },
  { deck: 1005, ntid: 1, fields: ['Antidote for acetaminophen overdose', 'N-acetylcysteine'], tags: ['pharm', 'tox', 'leech'] },
  { deck: 1005, ntid: 4, fields: ['ACE inhibitors cause a dry cough by raising {{c1::bradykinin}}.', ''], tags: ['pharm', 'cardio'] },
  { deck: 1005, ntid: 1, fields: ['First-line drug for anaphylaxis', 'Intramuscular adrenaline (epinephrine), 0.5 mg in adults'], tags: ['pharm', 'emergency'] },
  { deck: 1005, ntid: 1, fields: ['Class of drug: <i>omeprazole</i>', 'Proton-pump inhibitor'], tags: ['pharm', 'gi'] },
  { deck: 1005, ntid: 1, fields: ['Reversal agent for heparin', 'Protamine sulfate'], tags: ['pharm', 'anticoagulant'] },
  // Mathematics: 8
  { deck: 1006, ntid: 1, fields: [`Derivative of ${m('x^n')}`, m('\\frac{d}{dx}x^n = n\\,x^{n-1}')], tags: ['calculus'] },
  { deck: 1006, ntid: 1, fields: ['The quotient rule', m("\\left(\\frac{a}{b}\\right)' = \\frac{a'b - ab'}{b^2}", true)], tags: ['calculus'] },
  { deck: 1006, ntid: 4, fields: [`Euler’s identity: ${m('e^{i\\pi} + {{c1::1}} = 0')}`, 'Links five fundamental constants.'], tags: ['analysis'] },
  { deck: 1006, ntid: 1, fields: [`${m('\\int_0^1 x^2\\,dx')}`, m('\\frac{1}{3}')], tags: ['calculus'] },
  { deck: 1006, ntid: 1, fields: [`Sum of the first ${m('n')} odd numbers`, m('n^2')], tags: ['number-theory'] },
  { deck: 1006, ntid: 4, fields: ['A prime has exactly {{c1::two}} positive divisors.', ''], tags: ['number-theory'] },
  { deck: 1006, ntid: 1, fields: [`${m('\\frac{a}{b} + \\frac{c}{d}')}`, m('\\frac{ad + bc}{bd}')], tags: ['algebra'] },
  { deck: 1006, ntid: 3, fields: ['Name of the constant ≈ 2.71828', 'e'], tags: ['analysis'] },
];

export const SAVED_SEARCHES: Array<{ name: string; query: string }> = [
  { name: 'Struggling pharmacology', query: 'deck:"Medicine::Pharmacology" tag:leech' },
  { name: 'Arabic verbs', query: 'deck:Languages::Arabic tag:verb' },
];
