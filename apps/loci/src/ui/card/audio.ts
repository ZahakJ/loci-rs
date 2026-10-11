/**
 * One shared HTMLAudioElement for card audio: plays a queue in order, one at a time.
 * Buttons that started a clip get `data-playing` while it plays (styled by the card base CSS).
 */
let player: HTMLAudioElement | null = null;
let queue: Array<{ url: string; button?: HTMLElement | null }> = [];
let current: { url: string; button?: HTMLElement | null } | null = null;
let generation = 0;

function audio(): HTMLAudioElement {
  if (!player) {
    player = new Audio();
    player.preload = 'auto';
    player.addEventListener('ended', () => playNext());
    player.addEventListener('error', () => playNext());
  }
  return player;
}

function markPlaying(on: boolean) {
  if (current?.button) {
    if (on) current.button.setAttribute('data-playing', '');
    else current.button.removeAttribute('data-playing');
  }
}

function playNext() {
  markPlaying(false);
  current = queue.shift() ?? null;
  if (!current) return;
  const el = audio();
  const gen = generation;
  el.src = current.url;
  markPlaying(true);
  el.play().catch(() => {
    // Autoplay may be blocked before the first user gesture; move on quietly.
    if (gen === generation) playNext();
  });
}

/** Stops whatever plays and clears the queue. */
export function stopAudio(): void {
  generation++;
  queue = [];
  markPlaying(false);
  current = null;
  if (player) { player.pause(); player.removeAttribute('src'); player.load(); }
}

/** Replaces the queue and starts playing. Empty URLs are skipped. */
export function playAudio(items: Array<{ url: string; button?: HTMLElement | null }>): void {
  stopAudio();
  queue = items.filter((i) => i.url);
  playNext();
}

export function isAudioPlaying(): boolean { return current !== null; }
